#!/usr/bin/env bash
# tests/api/m2-storage.sh — M2 smoke test for storage-backed endpoints.
#
# Auth-gated as of M4 (cookie session). The runner scrapes the seeded
# admin password into $ADMIN_PASSWORD so we can log in. The /audit/_test
# debug routes were deleted in M5; this test now uses the real /audit
# list and metrics paths.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

if ! wait_for "$BASE_URL/healthz" 5; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\`."
  exit 1
fi

# /healthz is public.
expect_status "$BASE_URL/healthz" 200
expect_json_field "$BASE_URL/healthz" '.ok' 'true'

# /api/v1/me requires auth — without a cookie we expect 401.
unauth_status=$(curl -sS -o /dev/null -w "%{http_code}" "$BASE_URL/api/v1/me")
if [ "$unauth_status" = "401" ]; then
  _pass_msg "GET /api/v1/me unauth -> 401"
else
  _fail_msg "GET /api/v1/me unauth" "expected 401, got $unauth_status"
fi

# Log in as admin using the runner-provided seed.
login_as "admin" "${ADMIN_PASSWORD:?ADMIN_PASSWORD not set by runner}"

# /api/v1/me with cookie now succeeds and reflects the session user.
me_status=$(auth_get_status "$BASE_URL/api/v1/me")
if [ "$me_status" = "200" ]; then
  _pass_msg "GET /api/v1/me authed -> 200"
else
  _fail_msg "GET /api/v1/me authed" "expected 200, got $me_status"
fi
me_user=$(auth_curl "$BASE_URL/api/v1/me" | jq -r '.username')
if [ "$me_user" = "admin" ]; then _pass_msg "/me .username == admin"
else _fail_msg "/me .username" "got $me_user"; fi

# /api/v1/audit lists the empty-or-existing audit log.
audit_status=$(auth_get_status "$BASE_URL/api/v1/audit")
if [ "$audit_status" = "200" ]; then _pass_msg "GET /api/v1/audit -> 200"
else _fail_msg "GET /api/v1/audit" "expected 200, got $audit_status"; fi

# Verify users.json was written: file should exist on disk under the volume.
if [ -f "$(dirname "$0")/../../.cache/ocelaudit-data/users.json" ]; then
  _pass_msg "users.json persisted to volume host_path"
else
  _fail_msg "users.json on disk" "not found at .cache/ocelaudit-data/users.json"
fi

# Verify session.key was persisted (M4: WASI P2 components are fresh
# per-request, so the signing key must live on disk).
if [ -f "$(dirname "$0")/../../.cache/ocelaudit-data/session.key" ]; then
  _pass_msg "session.key persisted to volume host_path"
else
  _fail_msg "session.key on disk" "not found at .cache/ocelaudit-data/session.key"
fi

finish
