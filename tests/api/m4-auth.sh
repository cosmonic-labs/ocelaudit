#!/usr/bin/env bash
# tests/api/m4-auth.sh — auth happy + sad paths.
#
#   - POST /api/v1/auth/login bad creds -> 401
#   - POST /api/v1/auth/login good creds -> 200, sets HttpOnly cookie
#   - GET  /api/v1/me with cookie -> 200 with role
#   - POST /api/v1/auth/logout -> clears cookie
#   - GET  /api/v1/me after logout -> 401

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

# Bad credentials.
bad_status=$(curl -sS -o /dev/null -w "%{http_code}" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/auth/login" \
  --data '{"username":"admin","password":"definitely-wrong"}')
if [ "$bad_status" = "401" ]; then _pass_msg "bad creds -> 401"
else _fail_msg "bad creds" "expected 401, got $bad_status"; fi

# Good credentials → 200 + Set-Cookie.
rm -f "$COOKIE_JAR"
good_headers=$(curl -sS -D /tmp/m4_login_h -o /tmp/m4_login_b -w "%{http_code}" \
  -c "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/auth/login" \
  --data "$(printf '{"username":"admin","password":"%s"}' "${ADMIN_PASSWORD:?}")")
if [ "$good_headers" = "200" ]; then _pass_msg "good creds -> 200"
else _fail_msg "good creds" "expected 200, got $good_headers"; fi

# Set-Cookie carries HttpOnly + SameSite=Strict.
sc=$(grep -i '^set-cookie:' /tmp/m4_login_h | tr -d '\r' | head -1)
if echo "$sc" | grep -q 'HttpOnly'; then _pass_msg "Set-Cookie has HttpOnly"
else _fail_msg "Set-Cookie HttpOnly" "got: $sc"; fi
if echo "$sc" | grep -q 'SameSite=Strict'; then _pass_msg "Set-Cookie has SameSite=Strict"
else _fail_msg "Set-Cookie SameSite" "got: $sc"; fi

# /api/v1/me reflects the session.
me_role=$(auth_curl "$BASE_URL/api/v1/me" | jq -r '.role')
if [ "$me_role" = "admin" ]; then _pass_msg "/api/v1/me .role == admin"
else _fail_msg "/api/v1/me .role" "got $me_role"; fi

# Logout.
logout_status=$(curl -sS -o /dev/null -w "%{http_code}" -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
  -X POST "$BASE_URL/api/v1/auth/logout")
if [ "$logout_status" = "200" ]; then _pass_msg "POST /auth/logout -> 200"
else _fail_msg "POST /auth/logout" "expected 200, got $logout_status"; fi

# After logout, /me requires auth again.
post_logout_status=$(auth_get_status "$BASE_URL/api/v1/me")
if [ "$post_logout_status" = "401" ]; then _pass_msg "GET /me post-logout -> 401"
else _fail_msg "GET /me post-logout" "expected 401, got $post_logout_status"; fi

# Tampered cookie value rejected.
echo -e "127.0.0.1\tFALSE\t/\tFALSE\t0\tsession\tinvalid-token-format" > "$COOKIE_JAR"
tampered_status=$(auth_get_status "$BASE_URL/api/v1/me")
if [ "$tampered_status" = "401" ]; then _pass_msg "tampered cookie -> 401"
else _fail_msg "tampered cookie" "expected 401, got $tampered_status"; fi

finish
