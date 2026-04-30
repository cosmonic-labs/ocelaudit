#!/usr/bin/env bash
# tests/api/m2-storage.sh — M2 smoke test for storage-backed endpoints.
#
# Asserts:
#   - GET  /healthz           returns 200 with ok:true (storage opened)
#   - GET  /api/v1/me         returns 200 with role:admin (users.json seeded)
#   - POST /api/v1/audit/_test returns 201 with audit_id (jsonl append)
#   - GET  /api/v1/audit/_test count >= 1 after the POST (jsonl read)

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

if ! wait_for "$BASE_URL/healthz" 5; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\`."
  exit 1
fi

# /healthz
expect_status "$BASE_URL/healthz" 200
expect_json_field "$BASE_URL/healthz" '.ok' 'true'

# /api/v1/me — should return the seeded admin (no auth in M2; M4 wires real auth).
expect_status "$BASE_URL/api/v1/me" 200
expect_json_field "$BASE_URL/api/v1/me" '.username' 'admin'
expect_json_field "$BASE_URL/api/v1/me" '.role' 'admin'

# POST /api/v1/audit/_test — write a synthetic event.
post_status=$(curl -sS -o /tmp/m2_post -w "%{http_code}" -X POST "$BASE_URL/api/v1/audit/_test")
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
count=$(curl -fsS "$BASE_URL/api/v1/audit/_test" | jq -r '.count')
if [ "$count" -ge 1 ]; then
  _pass_msg "GET /api/v1/audit/_test count=$count (>=1 after POST)"
else
  _fail_msg "GET /api/v1/audit/_test count" "expected >=1, got $count"
fi

finish
