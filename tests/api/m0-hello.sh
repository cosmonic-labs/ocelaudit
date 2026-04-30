#!/usr/bin/env bash
# tests/api/m0-hello.sh — M0 smoke test, slightly evolved.
#
# `/` returns 200. In M0 the body was "ocelaudit booting"; from M6 on
# the gateway serves the SPA's index.html when the bundle is staged at
# /data/static. Either is acceptable as long as we get a 200.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

if ! wait_for "$BASE_URL/" 2; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\`."
  exit 1
fi

expect_status "$BASE_URL/" 200

body=$(curl -fsS -m 5 "$BASE_URL/")
if printf '%s' "$body" | grep -qE 'ocelaudit booting|<title>OcelAudit'; then
  _pass_msg "/ body identifies OcelAudit"
else
  _fail_msg "/ body" "neither plaintext booting line nor SPA title found"
fi

finish
