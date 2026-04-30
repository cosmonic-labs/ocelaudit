#!/usr/bin/env bash
# tests/api/m3-csl.sh — M3 smoke test for CSL ingest + queries.
#
# Walks the M3 endpoints against a wash dev that has tests/fixtures/csl/sample.json
# pre-staged at the volume's /data/csl/seed.json (done by _runner.sh).
#
# Asserts:
#   - GET  /api/v1/csl/sources          returns the static known-source map
#   - GET  /api/v1/csl/metadata         pre-refresh: count == 0
#   - POST /api/v1/csl/refresh          ingests the fixture (>= 12 records)
#   - GET  /api/v1/csl/metadata         post-refresh: count >= 12, sources[] non-empty
#   - GET  /api/v1/csl/entries/{id}     known fixture id resolves
#   - GET  /api/v1/csl/entries/missing  404

set -euo pipefail

source "$(dirname "$0")/_lib.sh"

if ! wait_for "$BASE_URL/healthz" 5; then
  echo "!! $BASE_URL is not reachable. Run \`make test-api\`."
  exit 1
fi

# /api/v1/csl/sources — should always include SDN (static map).
expect_status "$BASE_URL/api/v1/csl/sources" 200
known_sdn=$(curl -fsS "$BASE_URL/api/v1/csl/sources" | jq -r '.known[] | select(.code == "SDN") | .code')
if [ "$known_sdn" = "SDN" ]; then
  _pass_msg "/api/v1/csl/sources known includes SDN"
else
  _fail_msg "/api/v1/csl/sources" "SDN missing from known map"
fi

# Pre-refresh metadata: count = 0 (fresh data dir each test run).
pre_count=$(curl -fsS "$BASE_URL/api/v1/csl/metadata" | jq -r '.count')
if [ "$pre_count" = "0" ]; then
  _pass_msg "pre-refresh /api/v1/csl/metadata count == 0"
else
  _fail_msg "/api/v1/csl/metadata pre-refresh" "expected 0, got $pre_count"
fi

# POST /api/v1/csl/refresh — should ingest the fixture.
refresh_status=$(curl -sS -o /tmp/m3_refresh -w "%{http_code}" -X POST "$BASE_URL/api/v1/csl/refresh")
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

# Post-refresh metadata: should reflect the ingest.
post_count=$(curl -fsS "$BASE_URL/api/v1/csl/metadata" | jq -r '.count')
if [ "$post_count" -ge 12 ]; then
  _pass_msg "post-refresh /api/v1/csl/metadata count = $post_count"
else
  _fail_msg "post-refresh count" "expected >=12, got $post_count"
fi

source_count=$(curl -fsS "$BASE_URL/api/v1/csl/metadata" | jq -r '.sources | length')
if [ "$source_count" -ge 6 ]; then
  _pass_msg "post-refresh sources[] = $source_count entries"
else
  _fail_msg "post-refresh sources[]" "expected >=6, got $source_count"
fi

# Known fixture entry resolves.
expect_status "$BASE_URL/api/v1/csl/entries/OFAC-12345" 200
expect_json_field "$BASE_URL/api/v1/csl/entries/OFAC-12345" '.name' 'ACME HOLDINGS PYONGYANG'
expect_json_field "$BASE_URL/api/v1/csl/entries/OFAC-12345" '.source_list' 'SDN'

# Missing entry returns 404.
expect_status "$BASE_URL/api/v1/csl/entries/does-not-exist" 404

finish
