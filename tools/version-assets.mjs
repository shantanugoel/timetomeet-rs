// Put the build version in the PATH, not the query string.
//
// Cloudflare's cache key is host + path and *ignores the query string*. `/js/app.js?v=A`
// and `/js/app.js?v=B` are one single object at the edge, so a deploy that changes app.js
// keeps serving the previous bytes to everyone the edge already cached - and /js/* is
// `immutable, max-age=31536000`, so "keeps serving" means for a year. This was observed
// directly: a URL whose version string had not existed eleven minutes earlier came back
// `cf-cache-status: HIT, age: 666` with the previous build's JavaScript.
//
// So the version goes into the filename, which changes the cache key. The tool also fails
// the build if any local asset reference is left unversioned or points at a file that does
// not exist, because "I added a new file and forgot to version it" is otherwise invisible
// until a user gets a stale one.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, renameSync, readdirSync, statSync, mkdirSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';

const [dist, version] = process.argv.slice(2);
if (!dist || !version) {
  console.error('usage: version-assets.mjs <distDir> <version>');
  process.exit(2);
}

// Shell assets carry the deploy version: they are small, and keeping them on one version
// means a change to any of them (or to the wasm they load) rotates every URL at once.
const SHELL = [
  'style/app.css',
  'js/app.js',
  'js/engine.js',
  'pkg/tt_wasm.js',
  'pkg/tt_wasm_bg.wasm',
];

const sha8 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex').slice(0, 8);
const walk = (dir, out = []) => {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
};

const versioned = (p, v) => {
  const ext = extname(p);
  return `${p.slice(0, -ext.length)}.${v}${ext}`;
};

// Each asset is rewritten in every spelling an author could have written: absolute
// ('/js/engine.js'), relative to the importer ('./engine.js'), or bare ('tt_wasm_bg.wasm',
// which is how wasm-bindgen's glue resolves it against import.meta.url). Rewriting only
// the absolute spelling silently corrupted a real `import './engine.js?v=…'` into
// './/js/engine.<v>.js': the module fetch fails, and a failed module fetch is a network
// log rather than a console error, so the app rendered nothing with a clean console.
const map = new Map();
const rewrites = [];
for (const rel of SHELL) {
  const from = join(dist, rel);
  if (!statSync(from).isFile()) {
    console.error(`version-assets: missing ${rel}`);
    process.exit(1);
  }
  const to = versioned(rel, version);
  renameSync(from, join(dist, to));
  const dir = rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : '';
  const base = rel.slice(rel.lastIndexOf('/') + 1);
  const escaped = base.replace(/\./g, '\\.');
  const prefix = dir ? `(?:/${dir}/|${dir ? `./|../${dir}/` : ''})` : '(?:./)?';
  const newBase = to.slice(to.lastIndexOf('/') + 1);
  rewrites.push({ re: new RegExp(`(${prefix})${escaped}(?![\\w.\\-])`, 'g'), repl: (m, p1) => p1 + newBase });
  map.set('/' + rel, '/' + to);
}

// The dataset is ~460 KB and only changes when the dataset build changes. Giving it the
// deploy version would make every visitor re-download it after a CSS-only deploy, so it
// carries its own content hash instead.
const dsFrom = join(dist, 'tt-dataset.bin');
if (statSync(dsFrom).isFile()) {
  mkdirSync(join(dist, 'data'), { recursive: true });
  const to = `data/dataset.${sha8(dsFrom)}.bin`;
  renameSync(dsFrom, join(dist, to));
  map.set('/tt-dataset.bin', '/' + to);
  // Always referenced by absolute path, so it replaces wholesale rather than per-spelling.
  rewrites.push({ re: /\/tt-dataset\.bin(?![\w.\-])/g, repl: () => '/' + to });
}

const TEXT = /\.(html|js|webmanifest)$/;
const files = walk(dist).filter((f) => TEXT.test(f));
for (const file of files) {
  let text = readFileSync(file, 'utf8');
  const before = text;
  for (const { re, repl } of rewrites) {
    // Shell assets keep the author's own prefix; only the filename is swapped.
    text = text.replace(re, repl);
  }
  // Drop the query-string version: it is now dead weight, and leaving it would suggest the
  // query is what busts the cache.
  text = text.replace(/(\.(?:css|js|wasm|bin))\?v=[A-Za-z0-9._-]+/g, '$1');
  if (text !== before) writeFileSync(file, text);
}

// Contract 1: no reference may still point at an unversioned shell asset.
const stragglers = [];
for (const file of files) {
  const text = readFileSync(file, 'utf8');
  for (const rel of SHELL) {
    const re = new RegExp(`/${rel}(?![.\\w-])`, 'g');
    if (re.test(text)) stragglers.push(`${file.slice(dist.length)} -> /${rel}`);
  }
  if (/\/tt-dataset\.bin(?![.\w-])/.test(text)) stragglers.push(`${file.slice(dist.length)} -> /tt-dataset.bin`);
}
if (stragglers.length) {
  console.error('version-assets: unversioned asset references:\n  ' + stragglers.join('\n  '));
  process.exit(1);
}

// Contract 2: every local asset referenced by the shell must actually exist after the
// rename. This is what catches a typo or a file that was never copied into dist.
// Relative imports are resolved against the importing file, absolute ones against dist.
// Missing the relative spelling is exactly how a broken import escaped here once.
const ASSET = /["'(](\.?\.?\/[\w./-]+\.(?:css|js|wasm|bin|svg|png|webmanifest|ico|txt))(\?[^"')\s]*)?["')]/g;
const missing = new Set();
const generated = /\/pkg\/[^/]+\.js$/;
for (const file of files) {
  // wasm-bindgen's glue is generated, and it contains a module-specifier table entry
  // "./tt_wasm_bg.js" that looks exactly like a file reference but is the name the wasm
  // binary imports - there is no such file and making one would break import resolution.
  // Generated output is not ours to police.
  if (generated.test(file)) continue;
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(ASSET)) {
    const ref = m[1];
    const target = ref.startsWith('./') || ref.startsWith('../')
      ? join(dirname(file), ref)
      : join(dist, ref);
    if (!statSync(target, { throwIfNoEntry: false })) missing.add(`${file.slice(dist.length)} -> ${ref}`);
  }
}
if (missing.size) {
  console.error('version-assets: references to files that are not in dist (absolute and relative checked):\n  ' + [...missing].join('\n  '));
  process.exit(1);
}

const manifest = { version, assets: Object.fromEntries([...map].filter(([k]) => k.startsWith('/'))) };
writeFileSync(join(dist, 'asset-manifest.json'), JSON.stringify(manifest, null, 1) + '\n');
console.log(`version-assets: ${SHELL.length + 1} assets under v${version}, manifest written`);