#!/usr/bin/env bash
# tests/api/m4-audit-metrics.sh — /api/v1/audit + /api/v1/metrics.
#
# Runs a few searches first to populate the audit log, then exercises:
#   - GET /api/v1/audit         paginated list
#   - GET /api/v1/audit/{id}    fetch by id
#   - GET /api/v1/metrics       counts + tlp histogram

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

login_as "admin" "${ADMIN_PASSWORD:?}"
auth_curl -X POST "$BASE_URL/api/v1/csl/refresh?source=seed" >/dev/null

login_as "compliance" "${COMPLIANCE_PASSWORD:?}"

# Run three searches with distinct TLP outcomes to populate audit.
red_id=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"ACME HOLDINGS PYONGYANG"}' | jq -r '.audit_id')
green_id=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"Buttercup Daydream Industries"}' | jq -r '.audit_id')
yellow_id=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"Tehran Aerospace"}' | jq -r '.audit_id')

# Fetch the red one back.
body=$(auth_curl "$BASE_URL/api/v1/audit/$red_id")
got_tlp=$(echo "$body" | jq -r '.tlp')
got_who=$(echo "$body" | jq -r '.who')
got_decision=$(echo "$body" | jq -r '.decision')
if [ "$got_tlp" = "red" ]; then _pass_msg "/audit/{red} tlp=red"
else _fail_msg "/audit/{red} tlp" "got $got_tlp"; fi
if [ "$got_who" = "compliance" ]; then _pass_msg "/audit/{red} who=compliance"
else _fail_msg "/audit/{red} who" "got $got_who"; fi
if [ "$got_decision" = "auto-block" ]; then _pass_msg "/audit/{red exact-match} decision=auto-block"
else _fail_msg "/audit/{red} decision" "expected auto-block, got $got_decision"; fi

# /audit list, default pagination, returns at least 3 events newest-first.
body=$(auth_curl "$BASE_URL/api/v1/audit?limit=10")
total=$(echo "$body" | jq '.events | length')
if [ "$total" -ge 3 ]; then _pass_msg "/audit list >= 3 events ($total)"
else _fail_msg "/audit list" "expected >=3, got $total"; fi
newest=$(echo "$body" | jq -r '.events[0].audit_id')
if [ "$newest" = "$yellow_id" ]; then _pass_msg "/audit list newest-first"
else _fail_msg "/audit list ordering" "newest=$newest, expected $yellow_id"; fi

# Missing audit id → 404.
expect_authed_status() {
  local url="$1" want="$2"
  local got; got=$(auth_get_status "$url")
  if [ "$got" = "$want" ]; then _pass_msg "GET $url -> $want"
  else _fail_msg "GET $url" "expected $want, got $got"; fi
}
expect_authed_status "$BASE_URL/api/v1/audit/no-such-id" 404

# /metrics returns counts + tlp histogram.
body=$(auth_curl "$BASE_URL/api/v1/metrics")
csl_count=$(echo "$body" | jq -r '.csl_count')
if [ "$csl_count" -ge 12 ]; then _pass_msg "/metrics csl_count = $csl_count"
else _fail_msg "/metrics csl_count" "expected >=12, got $csl_count"; fi
red=$(echo "$body" | jq -r '.tlp_histogram.red')
yellow=$(echo "$body" | jq -r '.tlp_histogram.yellow')
green=$(echo "$body" | jq -r '.tlp_histogram.green')
sum=$((red + yellow + green))
if [ "$sum" -ge 3 ]; then _pass_msg "/metrics tlp_histogram sums to $sum (>=3)"
else _fail_msg "/metrics tlp_histogram sum" "expected >=3, got $sum"; fi

finish
