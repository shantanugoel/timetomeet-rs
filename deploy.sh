#!/usr/bin/env bash
# Deploy dist/ to Cloudflare Pages.
#
#   ./deploy.sh            preview deployment (branch preview) -> a review URL
#   ./deploy.sh --prod     production deployment (branch main)  -> timetomeet-42n.pages.dev
#
# Production is never touched by the default path. The custom hostname
# timetomeet.shantanugoel.com is attached to the production branch only, and the rollback
# plan is to move the DNS record back to GitHub Pages - which is why the old build stays
# deployed on GitHub Pages until the new one has been live for a week.
set -euo pipefail
cd "$(dirname "$0")"

PROJECT=timetomeet
BRANCH=preview
[ "${1:-}" = "--prod" ] && BRANCH=main

./build.sh

say() { printf '\033[1;36m==\033[0m %s\n' "$*"; }
say "deploying dist/ to $PROJECT (branch $BRANCH)"

# _headers carries the cache policy; wrangler uploads it as a real file, and Pages applies
# it as response headers rather than serving it.
OUT=$(npx --yes wrangler pages deploy dist --project-name "$PROJECT" --branch "$BRANCH" --commit-dirty=true 2>&1)
printf '%s\n' "$OUT"
# The apex domain is the one with a provisioned certificate straight away; per-deployment
# *.pages.dev hostnames can take a minute to get one, which would fail the smoke test for
# the wrong reason.
URL=$(printf '%s' "$OUT" | grep -oE 'https://[a-z0-9.-]+\.pages\.dev' | tail -1)
if [ "$BRANCH" = main ]; then URL="https://$PROJECT-42n.pages.dev"; fi
[ -n "$URL" ] || { echo "could not parse a deployment URL from wrangler output" >&2; exit 1; }

say "smoke-testing $URL"
node tools/smoke.mjs "$URL"

say "done"
if [ "$BRANCH" = preview ]; then
  say "preview URL is printed above; production is unchanged"
fi
