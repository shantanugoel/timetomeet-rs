// Post-deploy smoke test: assert what the CDN actually returns, not what we meant to send.
//
// A `_headers` rule can silently change a content type, a cache policy can be wrong in a
// way that only breaks on the second visit, and a build can upload an asset that 404s only
// in production. None of that is visible from `cargo test`, and one of these exact bugs
// shipped a blank page once already.
//
// Everything here is driven by the deployed asset-manifest.json rather than hardcoded
// paths, so it verifies the wiring that index.html and engine.js actually use. It also
// asserts the *unversioned* paths are gone: the version has to be in the path, because
// Cloudflare's cache key ignores the query string.
const BASE = process.argv[2];
if (!BASE) {
  console.error('usage: node tools/smoke.mjs https://<deployment-url>');
  process.exit(2);
}

const checks = [];
const check = (name, ok, detail) => checks.push({ name, ok, detail });

async function get(path) {
  const r = await fetch(`${BASE}${path}`, { redirect: 'manual' });
  const buf = await r.arrayBuffer();
  return {
    status: r.status,
    type: r.headers.get('content-type') || '',
    cc: r.headers.get('cache-control') || '',
    cache: r.headers.get('cf-cache-status') || '',
    text: () => new TextDecoder().decode(buf),
    len: buf.byteLength,
    first: new Uint8Array(buf.slice(0, 4)),
  };
}

const root = await get('/');
const html = root.text();
check('index is 200', root.status === 200, `status ${root.status}`);
check('index is html', root.type.includes('text/html'), root.type);
const notCached = (cc) => /no-store|must-revalidate|max-age=0/.test(cc);
check('index is revalidated, not cached', notCached(root.cc), root.cc || 'no cache-control');

const man = await get('/asset-manifest.json');
let manifest = {};
try { manifest = JSON.parse(man.text()); } catch { /* reported below */ }
const assets = manifest.assets || {};
check('asset manifest is served', man.status === 200 && Boolean(manifest.version), `${man.status} ${man.version || 'unparseable'}`);
check('manifest is not cached', notCached(man.cc), man.cc || 'no cache-control');

// The version must be in the path. A query string is not a cache key at the edge.
const cssPath = assets['/style/app.css'];
check('css path carries the version in the path', Boolean(cssPath) && cssPath.includes(manifest.version), cssPath || 'missing');
check('index.html uses the versioned css', Boolean(cssPath) && html.includes(cssPath), cssPath || 'missing');
check('index.html uses the versioned app.js', html.includes(assets['/js/app.js'] || '\u0000'), assets['/js/app.js'] || 'missing');
check('no query-string versioning left in html', !/\?v=/.test(html), 'found ?v= in index.html');
// CSP is script-src 'self': an inline script here would be a console error and a no-op.
check('no inline scripts in html', !/<script(?![^>]*\ssrc=)[^>]*>/i.test(html), 'inline <script> present');

for (const [logical, path] of Object.entries(assets)) {
  const r = await get(path);
  const wantImmutable = /immutable/.test(r.cc);
  check(`${path} is served`, r.status === 200 && r.len > 0, `${r.status} ${r.len} bytes`);
  if (logical.endsWith('.css')) check('css is text/css', r.type.includes('text/css'), r.type);
  if (logical.endsWith('.js')) check(`${path} is javascript`, r.type.includes('javascript'), `${r.status} ${r.type}`);
  if (logical.endsWith('.wasm')) {
    check('wasm binary is application/wasm', r.type.includes('wasm'), r.type);
    check('wasm binary has wasm magic', r.first[0] === 0x00 && r.first[1] === 0x61, [...r.first].join(','));
    check('wasm binary is non-trivial', r.len > 100_000, `${r.len} bytes`);
  }
  if (logical.endsWith('.bin')) {
    check('city index is served', r.status === 200 && r.len > 100_000, `${r.status} ${r.len} bytes`);
    check('city index has TTD1 magic', [...r.first].map((b) => String.fromCharCode(b)).join('') === 'TTD1', [...r.first].join(','));
  }
  if (/\.(css|js|wasm)$/.test(logical)) {
    check(`${path} is immutable`, wantImmutable, r.cc || 'no cache-control');
  }
}

// The bug this exists for: the wasm glue is JS, and serving it as a wasm MIME type makes
// the browser refuse to import() it, which produces a blank page and one vague console error.
const gluePath = assets['/pkg/tt_wasm.js'];
if (gluePath) {
  const glue = await get(gluePath);
  check('wasm glue starts like JS', glue.first[0] === 0x2f || glue.first[0] === 0x69, `first bytes ${[...glue.first].join(',')}`);
}

// End-to-end wiring: the shipped JavaScript must point at the shipped wasm and dataset.
const enginePath = assets['/js/engine.js'];
if (enginePath) {
  const engine = await get(enginePath).then((r) => r.text());
  check('engine.js loads the versioned wasm', engine.includes(assets['/pkg/tt_wasm_bg.wasm'] || '\u0000'), assets['/pkg/tt_wasm_bg.wasm']);
  check('engine.js loads the versioned dataset', engine.includes(assets['/tt-dataset.bin'] || '\u0000'), assets['/tt-dataset.bin']);
}

// Old unversioned URLs must not be served *by the origin*: nothing references them any
// more, so if the origin still has them then two builds are live at once. An edge node can
// legitimately keep answering one from cache until its old immutable TTL expires, which is
// harmless - and is reported, not hidden, because it is the same cache-key behaviour that
// makes the version have to live in the path.
for (const legacy of ['/style/app.css', '/js/app.js', '/pkg/tt_wasm_bg.wasm', '/tt-dataset.bin']) {
  const r = await get(legacy);
  const ok = r.status === 404 || (r.status === 200 && r.cache === 'HIT');
  check(`legacy unversioned ${legacy} not served by origin`, ok, `status ${r.status}, cf-cache-status ${r.cache || '-'}`);
}

// The service worker lives at a fixed URL, so it is the one asset the path-versioning
// cannot protect. `immutable` would be a disaster; a long max-age is survivable because
// everything it precaches is versioned, so a new deploy simply misses its cache. A zone
// cache rule can set a browser TTL here that _headers cannot override - reported, not
// hidden, so it is a decision rather than a surprise.
const sw = await get('/sw.js');
check('service worker is not immutable', sw.status === 200 && !/immutable/.test(sw.cc), sw.cc || 'no cache-control');
if (/max-age=[1-9]\d{3,}/.test(sw.cc)) {
  console.log(`note   /sw.js has a long max-age (${sw.cc}) - a zone cache rule, not _headers; harmless while the shell is versioned`);
}

const missing = await get('/definitely-not-a-page');
check('unknown routes 404', missing.status === 404, `status ${missing.status}`);

const bad = checks.filter((c) => !c.ok);
for (const c of checks) console.log(`${c.ok ? 'ok  ' : 'FAIL'}  ${c.name}${c.ok ? '' : `  <- ${c.detail}`}`);
if (bad.length) {
  console.error(`\n${bad.length}/${checks.length} smoke checks failed against ${BASE}`);
  process.exit(1);
}
console.log(`\nsmoke: ${checks.length}/${checks.length} checks passed against ${BASE}`);