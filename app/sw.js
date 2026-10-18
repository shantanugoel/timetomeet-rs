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
      const fetched = fetch(req)
        .then((res) => {
          if (res && res.ok && res.type === 'basic') {
            const copy = res.clone();
            caches.open(VERSION).then((c) => c.put(req, copy));
          }
          return res;
        })
        .catch(() => hit);
      return hit || fetched;
    })
  );
});
