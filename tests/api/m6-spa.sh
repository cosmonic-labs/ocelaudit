#!/usr/bin/env bash
# tests/api/m6-spa.sh — M6 static-asset serving.
#
# Asserts:
#   - GET /                         200, text/html, contains <title>OcelAudit
#   - GET /assets/<bundle>.js       200, application/javascript
#   - GET /assets/<bundle>.css      200, text/css
#   - GET /brand/ocelot.svg         200, image/svg+xml
#   - GET /dashboard                200, falls back to SPA index.html
#   - Content-Security-Policy       header present on /

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/" 5 || { echo "!! gateway unreachable"; exit 1; }

# Sanity: the runner staged ui/dist into /data/static.
if [ ! -d "$(dirname "$0")/../../ui/dist" ]; then
  _fail_msg "ui/dist exists" "run \`pnpm --dir ui build\` first"
  finish
fi

# /
status_and_ct() {
  local url="$1" want_status="$2" want_ct="$3"
  local out; out=$(curl -sS -D /tmp/m6_h -o /tmp/m6_b -w "%{http_code}" "$url")
  if [ "$out" = "$want_status" ]; then _pass_msg "GET $url -> $want_status"
  else _fail_msg "GET $url" "expected $want_status, got $out"; fi
  # Use grep -F (fixed) on the want_ct so `image/svg+xml` doesn't get
  # parsed as a regex (the `+` would mean one-or-more).
  if grep -i '^content-type:' /tmp/m6_h | grep -qF "$want_ct"; then
    _pass_msg "$url content-type matches $want_ct"
  else
    _fail_msg "$url content-type" "expected $want_ct, got: $(grep -i '^content-type:' /tmp/m6_h | head -1)"
  fi
}

status_and_ct "$BASE_URL/" 200 "text/html"
if printf '%s' "$(cat /tmp/m6_b)" | grep -q "<title>OcelAudit"; then
  _pass_msg "/ body has SPA title"
else
  _fail_msg "/ body" "missing <title>OcelAudit"
fi

# CSP header.
if grep -qiE "^content-security-policy:" /tmp/m6_h; then
  _pass_msg "/ has Content-Security-Policy header"
else
  _fail_msg "/ CSP" "header missing"
fi

# Find the actual asset filenames in the dist (they have content hashes).
js_path=$(ls "$(dirname "$0")/../../ui/dist/assets/"*.js | head -1 | sed -e 's|.*/dist||')
css_path=$(ls "$(dirname "$0")/../../ui/dist/assets/"*.css | head -1 | sed -e 's|.*/dist||')
status_and_ct "$BASE_URL$js_path" 200 "application/javascript"
status_and_ct "$BASE_URL$css_path" 200 "text/css"

# Brand SVG.
status_and_ct "$BASE_URL/brand/ocelot.svg" 200 "image/svg+xml"

# SPA fallback: an unknown client-side route should serve index.html.
fb=$(curl -sS "$BASE_URL/dashboard")
if printf '%s' "$fb" | grep -q "<title>OcelAudit"; then
  _pass_msg "SPA fallback for /dashboard returns index.html"
else
  _fail_msg "SPA fallback" "expected SPA HTML, got: $(printf '%s' "$fb" | head -c 80)"
fi

finish
