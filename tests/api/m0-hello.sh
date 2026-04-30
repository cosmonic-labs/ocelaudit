#!/usr/bin/env bash
# tests/api/m0-hello.sh — M0 smoke test: api-gateway returns
# 200 "ocelaudit booting" on /.
#
# Independently runnable (./tests/api/m0-hello.sh) or via the runner
# (make test-api), which boots wash dev and tears it down.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

# If this script is invoked directly (not via _runner.sh), require an
# already-running wash dev on $BASE_URL.
if ! wait_for "$BASE_URL/" 2; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\` (which boots wash dev)"
  echo "   or start a dev server in another shell: \`cd components/api-gateway && wash dev\`"
  exit 1
fi

expect_status "$BASE_URL/" 200
expect_body_contains "$BASE_URL/" "ocelaudit booting"

finish
