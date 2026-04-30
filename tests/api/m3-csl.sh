#!/usr/bin/env bash
# tests/api/m3-csl.sh — M3 smoke test for CSL ingest + queries.
#
# Auth-gated as of M4. /api/v1/csl/sources is intentionally public
# (the unauth login page surfaces it as a teaser).

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

if ! wait_for "$BASE_URL/healthz" 5; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\`."
  exit 1
fi

# /api/v1/csl/sources is public.
expect_status "$BASE_URL/api/v1/csl/sources" 200
known_sdn=$(curl -fsS "$BASE_URL/api/v1/csl/sources" | jq -r '.known[] | select(.code == "SDN") | .code')
if [ "$known_sdn" = "SDN" ]; then
  _pass_msg "/api/v1/csl/sources (public) known includes SDN"
else
  _fail_msg "/api/v1/csl/sources" "SDN missing from known map"
fi

# Login as admin (csl/refresh requires admin role).
login_as "admin" "${ADMIN_PASSWORD:?ADMIN_PASSWORD not set by runner}"

# Pre-refresh metadata: count = 0 (fresh data dir each test run).
pre_count=$(auth_curl "$BASE_URL/api/v1/csl/metadata" | jq -r '.count')
if [ "$pre_count" = "0" ]; then
  _pass_msg "pre-refresh /api/v1/csl/metadata count == 0"
else
  _fail_msg "/api/v1/csl/metadata pre-refresh" "expected 0, got $pre_count"
fi

# POST /api/v1/csl/refresh — should ingest the fixture.
refresh_status=$(curl -sS -o /tmp/m3_refresh -w "%{http_code}" -b "$COOKIE_JAR" \
  -X POST "$BASE_URL/api/v1/csl/refresh")
if [ "$refresh_status" = "200" ]; then
  _pass_msg "POST /api/v1/csl/refresh -> 200"
else
  _fail_msg "POST /api/v1/csl/refresh" "expected 200, got $refresh_status (body: $(cat /tmp/m3_refresh | head -c 200))"
  finish
fi
ingested=$(jq -r '.ingested' /tmp/m3_refresh)
if [ "$ingested" -ge 12 ]; then
  _pass_msg "refresh ingested $ingested records (>=12)"
else
  _fail_msg "refresh ingested" "expected >=12, got $ingested"
fi

# Post-refresh metadata.
post_count=$(auth_curl "$BASE_URL/api/v1/csl/metadata" | jq -r '.count')
if [ "$post_count" -ge 12 ]; then
  _pass_msg "post-refresh /api/v1/csl/metadata count = $post_count"
else
  _fail_msg "post-refresh count" "expected >=12, got $post_count"
fi

source_count=$(auth_curl "$BASE_URL/api/v1/csl/metadata" | jq -r '.sources | length')
if [ "$source_count" -ge 6 ]; then
  _pass_msg "post-refresh sources[] = $source_count entries"
else
  _fail_msg "post-refresh sources[]" "expected >=6, got $source_count"
fi

# Known fixture entry resolves.
expect_authed_status() {
  local url="$1" want="$2"
  local got
  got=$(auth_get_status "$url")
  if [ "$got" = "$want" ]; then _pass_msg "GET $url -> $want"
  else _fail_msg "GET $url" "expected $want, got $got"; fi
}
expect_authed_status "$BASE_URL/api/v1/csl/entries/OFAC-12345" 200
entry_name=$(auth_curl "$BASE_URL/api/v1/csl/entries/OFAC-12345" | jq -r '.name')
if [ "$entry_name" = "ACME HOLDINGS PYONGYANG" ]; then
  _pass_msg "/csl/entries/OFAC-12345 .name correct"
else
  _fail_msg "/csl/entries/OFAC-12345 .name" "got $entry_name"
fi
expect_authed_status "$BASE_URL/api/v1/csl/entries/does-not-exist" 404

# csl/refresh as compliance role should be 403.
login_as "compliance" "${COMPLIANCE_PASSWORD:?COMPLIANCE_PASSWORD not set by runner}"
forbidden_status=$(curl -sS -o /dev/null -w "%{http_code}" -b "$COOKIE_JAR" \
  -X POST "$BASE_URL/api/v1/csl/refresh")
if [ "$forbidden_status" = "403" ]; then
  _pass_msg "POST /api/v1/csl/refresh as compliance -> 403"
else
  _fail_msg "POST /api/v1/csl/refresh as compliance" "expected 403, got $forbidden_status"
fi

finish
