#!/usr/bin/env bash
# Build the deployable bundle into dist/.
#
# No bundler on purpose: the app is plain HTML/CSS/ESM plus one wasm module, and every
# byte that a bundler would add is a byte we would have to defend. What this script does
# instead is the things that actually move load time - wasm-bindgen glue, fat LTO, and
# precompressed brotli next to every text asset so the CDN never compresses on the fly.
set -euo pipefail
cd "$(dirname "$0")"

GIT_REV="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
# APP_VERSION is computed after the engine and the dataset are built - see below.
DIST=dist
PKG_MIN=180000   # wasm budget, bytes brotli (measured 157 KB; headroom for M4-M7 features)
TOTAL_MIN=260000 # first-paint budget, brotli, excluding the lazy dataset

say() { printf '\033[1;36m==\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

say "engine (release, fat LTO)"
cargo build --release -q -p tt-wasm --target wasm32-unknown-unknown

say "wasm-bindgen glue"
rm -rf app/pkg
wasm-bindgen --target web --out-dir app/pkg \
  target/wasm32-unknown-unknown/release/tt_wasm.wasm >/dev/null

say "dataset"
mkdir -p app
cargo run -q --release -p tt-dataset -- \
  --in third_party/GeoNames_cities15000.txt --out app/tt-dataset.bin --min-pop 50000

say "contracts"
node tools/check-styles.mjs
node tools/check-wasm-abi.mjs

# Content-addressed asset version. It has to cover the *built* artefacts, not just the
# sources: /js/*, /style/* and /pkg/* are served immutable, so a Rust-only change that
# produces a new wasm under an unchanged URL would be invisible to every returning client.
# _headers and sw.js are inputs too, for the same reason.
SRC_HASH="$(cat app/pkg/tt_wasm.js app/pkg/tt_wasm_bg.wasm app/tt-dataset.bin \
    <(find app tools -type f \( -name '*.js' -o -name '*.css' -o -name '*.html' -o -name '_headers' -o -name '*.mjs' \) -not -path 'app/pkg/*' -print0 | sort -z | xargs -0 cat) \
    | sha256sum | cut -c1-8)"
APP_VERSION="$GIT_REV-$SRC_HASH"

say "assembling $DIST"
rm -rf "$DIST"
mkdir -p "$DIST"
cp -R app/. "$DIST/"
# The dev copy of the pkg is unminified glue; strip comments/sourcemaps from the bundle.
rm -f "$DIST/pkg/"*.rs "$DIST/pkg/.gitignore" "$DIST/pkg"/*.d.ts.map 2>/dev/null || true
printf 'APP_VERSION=%s\n' "$APP_VERSION" > "$DIST/version.txt"
# The service worker must change identity when the bundle does, or a stale cache outlives
# the deploy that fixed it.
find "$DIST" -type f \( -name '*.html' -o -name '*.js' -o -name '*.webmanifest' \) -exec sed -i "s/APP_VERSION/$APP_VERSION/g" {} +

# The version goes into the filename, not the query string: Cloudflare's cache key is
# host+path and ignores the query, so ?v= does nothing at the edge and an immutable asset
# keeps its old bytes for a year. This also fails the build if any reference is left
# unversioned or points at a file that is not in dist.
say "versioning asset paths"
node tools/version-assets.mjs "$DIST" "$APP_VERSION"
[ -f "$DIST/icon-192.png" ] || rsvg-convert -w 192 -h 192 "$DIST/icon.svg" -o "$DIST/icon-192.png"
[ -f "$DIST/icon-512.png" ] || rsvg-convert -w 512 -h 512 "$DIST/icon.svg" -o "$DIST/icon-512.png"

say "precompressing (brotli q11)"
find "$DIST" -type f \( -name '*.js' -o -name '*.css' -o -name '*.html' -o -name '*.wasm' -o -name '*.bin' -o -name '*.svg' -o -name '*.json' -o -name '*.txt' \) -print0 |
  xargs -0 -P"$(nproc)" -I{} sh -c 'brotli -q 11 -k "{}"'

# The dataset is lazy-loaded, so it must not count against first paint.
FIRST=$(find "$DIST" -type f \( -name '*.html' -o -name '*.css' -o -name '*.js' -o -name '*.wasm' \) ! -name '*.bin' -exec stat -c '%n %s' {} + | sort -k2 -n)
WASM_BR=$(stat -c '%s' "$(ls "$DIST"/pkg/tt_wasm_bg.*.wasm.br | head -1)")
TOTAL_BR=$(find "$DIST" -type f \( -name '*.html.br' -o -name '*.css.br' -o -name '*.js.br' -o -name '*.wasm.br' \) -printf '%s\n' | awk '{s+=$1} END {print s+0}')

echo
printf '%-34s %10s %10s\n' FILE RAW BROTLI
# Unquoted on purpose: the glob list must word-split, and no asset name contains a space.
for f in "$DIST/index.html" $(ls "$DIST"/style/app.*.css "$DIST"/js/app.*.js "$DIST"/js/engine.*.js "$DIST"/pkg/tt_wasm.*.js "$DIST"/pkg/tt_wasm_bg.*.wasm "$DIST"/data/dataset.*.bin); do
  raw=$(stat -c '%s' "$f"); br=$(stat -c '%s' "$f.br" 2>/dev/null || echo -)
  printf '%-34s %10s %10s\n' "${f#$DIST/}" "$raw" "$br"
done
echo

say "first-paint transfer (brotli): $TOTAL_BR bytes   wasm: $WASM_BR bytes"
[ "$WASM_BR" -le "$PKG_MIN" ] || die "wasm ${WASM_BR}B exceeds budget ${PKG_MIN}B - see 'cargo bloat -p tt-wasm --release --target wasm32-unknown-unknown'"
[ "$TOTAL_BR" -le "$TOTAL_MIN" ] || die "first paint ${TOTAL_BR}B exceeds budget ${TOTAL_MIN}B"
say "budgets OK · v$APP_VERSION"
