// Post-deploy smoke test: assert what the CDN actually returns, not what we meant to send.
//
// A `_headers` rule can silently change a content type, a cache policy can be wrong in a
// way that only breaks on the second visit, and a build can upload an asset that 404s only
// in production. None of that is visible from `cargo test`, and one of these exact bugs
// shipped a blank page once already.
const BASE = process.argv[2];
if (!BASE) {
  console.error('usage: node tools/smoke.mjs https://<deployment-url>');
  process.exit(2);
}

const html = await fetch(`${BASE}/`).then((r) => r.text());
const version = html.match(/app\.css\?v=([\w.-]+)/)?.[1];
const V = version ? `?v=${version}` : '';

const checks = [];
const check = (name, ok, detail) => checks.push({ name, ok, detail });

async function head(path) {
  const r = await fetch(`${BASE}${path}`, { redirect: 'manual' });
  const buf = await r.arrayBuffer();
  return { status: r.status, type: r.headers.get('content-type') || '', cc: r.headers.get('cache-control') || '', len: buf.byteLength, first: new Uint8Array(buf.slice(0, 4)) };
}

const root = await head('/');
check('index is 200', root.status === 200, `status ${root.status}`);
check('index is html', root.type.includes('text/html'), root.type);
check('index is revalidated, not cached', /must-revalidate|max-age=0/.test(root.cc), root.cc || 'no cache-control');
check('build version is stamped', Boolean(version), version || 'no ?v= in index.html');

const css = await head(`/style/app.css${V}`);
check('css 200 + immutable', css.status === 200 && /immutable/.test(css.cc), `${css.status} ${css.cc}`);
check('css is css', css.type.includes('text/css'), css.type);

const app = await head(`/js/app.js${V}`);
check('app.js is javascript', app.type.includes('javascript'), app.type);

// The bug this exists for: the wasm glue is JS, and serving it as a wasm MIME type makes
// the browser refuse to import() it, which produces a blank page and one vague console error.
const glue = await head(`/pkg/tt_wasm.js${V}`);
check('wasm glue is javascript', glue.type.includes('javascript'), `${glue.status} ${glue.type}`);
check('wasm glue starts like JS', glue.first[0] === 0x2f || glue.first[0] === 0x69, `first bytes ${[...glue.first].join(',')}`);

const bin = await head(`/pkg/tt_wasm_bg.wasm${V}`);
check('wasm binary is application/wasm', bin.type.includes('wasm'), bin.type);
check('wasm binary has wasm magic', bin.first[1] === 0x61 && bin.first[0] === 0x00, `first bytes ${[...bin.first].join(',')}`);
check('wasm binary is non-trivial', bin.len > 100_000, `${bin.len} bytes`);

const ds = await head('/tt-dataset.bin');
check('city index is served', ds.status === 200 && ds.len > 100_000, `${ds.status} ${ds.len} bytes`);
check('city index has TTD1 magic', [...ds.first].map((b) => String.fromCharCode(b)).join('') === 'TTD1', [...ds.first].join(','));

const missing = await head('/definitely-not-a-page');
check('unknown routes 404', missing.status === 404, `status ${missing.status}`);

const bad = checks.filter((c) => !c.ok);
for (const c of checks) console.log(`${c.ok ? 'ok  ' : 'FAIL'}  ${c.name}${c.ok ? '' : `  <- ${c.detail}`}`);
if (bad.length) {
  console.error(`\n${bad.length}/${checks.length} smoke checks failed against ${BASE}`);
  process.exit(1);
}
console.log(`\nsmoke: ${checks.length}/${checks.length} checks passed against ${BASE}`);
