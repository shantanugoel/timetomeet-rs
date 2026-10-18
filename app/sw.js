/* Offline-first for a static, client-only app.
 *
 * Precache the shell, then stale-while-revalidate for the rest. The point is not "works
 * on a plane" (though it does) - it is that a repeat visit paints from cache before the
 * network is even consulted.
 */
const VERSION = 'ttm-APP_VERSION';
const SHELL = ['/', '/style/app.css', '/js/app.js', '/js/engine.js', '/pkg/tt_wasm.js', '/pkg/tt_wasm_bg.wasm', '/icon.svg', '/manifest.webmanifest'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(VERSION).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== VERSION).map((k) => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET' || new URL(req.url).origin !== location.origin) return;
  e.respondWith(
    caches.match(req, { ignoreSearch: true }).then((hit) => {
      // Started before the hit is returned, so a cache hit is served immediately and the
      // entry still refreshes for the next load - stale-while-revalidate, not cache-first.
      const fetched = fetch(req)
        .then((res) => {
          if (res && res.ok && res.type === 'basic') {
            const copy = res.clone();
            caches.open(VERSION).then((c) => c.put(req, copy));
          }
          return res;
        })
        .catch(() => null);
      if (hit) return hit;
      return fetched.then((res) => {
        if (res) return res;
        // Offline and never seen this URL. For a navigation that means a deep link or a
        // shared link typed on a plane: answer with the cached shell rather than the
        // browser's dead-page screen. The app reads its state from the fragment, and a
        // fragment is never sent to the server, so "/" is the right document for it.
        if (req.mode === 'navigate') return caches.match('/', { ignoreSearch: true });
        return res;
      });
    })
  );
});
