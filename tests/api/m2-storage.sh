#!/usr/bin/env bash
# tests/api/m2-storage.sh — M2 smoke test for storage-backed endpoints.
#
# Auth-gated as of M4 (cookie session). The runner scrapes the seeded
# admin password into $ADMIN_PASSWORD so we can log in.

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

# POST /api/v1/audit/_test (M2 leftover, deleted in M5) — auth-gated.
post_status=$(curl -sS -o /tmp/m2_post -w "%{http_code}" -b "$COOKIE_JAR" \
  -X POST "$BASE_URL/api/v1/audit/_test")
if [ "$post_status" = "201" ]; then
  _pass_msg "POST /api/v1/audit/_test -> 201"
else
  _fail_msg "POST /api/v1/audit/_test" "expected 201, got $post_status"
fi
audit_id=$(jq -r '.audit_id' /tmp/m2_post)
if [[ "$audit_id" == debug-* ]]; then
  _pass_msg "audit_id format ok ($audit_id)"
else
  _fail_msg "audit_id format" "got '$audit_id'"
fi

# GET /api/v1/audit/_test — should list at least one event after the POST.
count=$(auth_curl "$BASE_URL/api/v1/audit/_test" | jq -r '.count')
if [ "$count" -ge 1 ]; then
  _pass_msg "GET /api/v1/audit/_test count=$count (>=1 after POST)"
else
  _fail_msg "GET /api/v1/audit/_test count" "expected >=1, got $count"
fi

finish
