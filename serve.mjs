// Local static server that behaves like the deploy target.
//
// python -m http.server lied to me: it sends no Cache-Control, so the browser applies a
// heuristic cache and I spent a cycle debugging CSS that was never loaded. This serves
// dist/ with the same rules as _headers - no-store on HTML, immutable on versioned assets,
// and real brotli negotiation from the precompressed .br files.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';

const ROOT = process.argv[2] || 'dist';
const PORT = Number(process.argv[3] || 8901);

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.bin': 'application/octet-stream',
  '.txt': 'text/plain; charset=utf-8',
  '.ics': 'text/calendar; charset=utf-8',
};

const server = createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost');
  let path = decodeURIComponent(url.pathname);
  if (path.endsWith('/')) path += 'index.html';
  const file = join(ROOT, normalize(path).replace(/^(\.\.[/\\])+/, ''));

  try {
    await stat(file);
    const accept = req.headers['accept-encoding'] || '';
    let body;
    let br = false;
    if (accept.includes('br')) {
      try {
        body = await readFile(`${file}.br`);
        br = true;
      } catch { /* fall through to raw */ }
    }
    if (!body) body = await readFile(file);

    const ext = extname(file);
    const mutable = path === '/sw.js' || path === '/asset-manifest.json' || path === '/';
    const immutable = !mutable && (/^(\/pkg\/|\/js\/|\/style\/|\/data\/)/.test(path) || path.endsWith('.bin'));
    res.writeHead(200, {
      'content-type': TYPES[ext] || 'application/octet-stream',
      'cache-control': immutable ? 'public, max-age=31536000, immutable' : 'no-store',
      ...(br ? { 'content-encoding': 'br' } : {}),
      'x-content-type-options': 'nosniff',
      'content-length': body.length,
    });
    res.end(body);
  } catch {
    res.writeHead(404, { 'content-type': 'text/plain', 'cache-control': 'no-store' });
    res.end('404');
  }
});

server.listen(PORT, () => console.log(`serving ${ROOT} on http://localhost:${PORT}`));
