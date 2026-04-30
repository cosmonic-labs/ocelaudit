#!/usr/bin/env bash
# tests/api/m4-search.sh — /api/v1/search end-to-end.
#
# Refreshes CSL from the fixture (re-auth as admin), then runs a series
# of POST /api/v1/search queries against known fixture entries and
# asserts top-1 + tlp expectations. Verifies the search appears in the
# audit log.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

# Re-seed the index by ingesting the fixture. (Earlier scripts in the
# runner already did this, but be defensive in case this is run alone.)
login_as "admin" "${ADMIN_PASSWORD:?}"
auth_curl -X POST "$BASE_URL/api/v1/csl/refresh?source=seed" >/dev/null

login_as "compliance" "${COMPLIANCE_PASSWORD:?}"

# Helper: POST /api/v1/search with a JSON body, capture body + status.
search() {
  local payload="$1"
  curl -sS -b "$COOKIE_JAR" -H 'content-type: application/json' \
    -X POST "$BASE_URL/api/v1/search" --data "$payload"
}

# Exact name match against an SDN entry → RED, top hit OFAC-12345.
body=$(search '{"q":"ACME HOLDINGS PYONGYANG"}')
top_id=$(echo "$body" | jq -r '.hits[0].entry_id')
top_tlp=$(echo "$body" | jq -r '.tlp')
top_decision=$(echo "$body" | jq -r '.decision')
audit_id=$(echo "$body" | jq -r '.audit_id')
if [ "$top_id" = "OFAC-12345" ]; then _pass_msg "search exact-name top1 = OFAC-12345"
else _fail_msg "search exact-name top1" "got $top_id"; fi
if [ "$top_tlp" = "red" ]; then _pass_msg "search exact-name tlp=red"
else _fail_msg "search exact-name tlp" "got $top_tlp"; fi
# Exact match goes to auto-block (no review needed) per the M12 split.
if [ "$top_decision" = "auto-block" ]; then _pass_msg "exact-name decision=auto-block"
else _fail_msg "exact-name decision" "expected auto-block, got $top_decision"; fi
if [[ "$audit_id" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[0-9a-f]{4}-[0-9a-f]{12}$ ]]; then
  _pass_msg "audit_id is UUIDv7-shaped ($audit_id)"
else
  _fail_msg "audit_id format" "expected UUIDv7, got $audit_id"
fi

# X-OcelAudit-Source defaults to "api" when not set; verify it lands on
# the audit row.
event=$(auth_curl "$BASE_URL/api/v1/audit/$audit_id")
event_source=$(echo "$event" | jq -r '.source')
if [ "$event_source" = "api" ]; then _pass_msg "audit_id reflects source=api default"
else _fail_msg "audit source" "expected api, got $event_source"; fi

# Sending X-OcelAudit-Source: ui marks the event as SPA-origin.
ui_audit_id=$(curl -sS -b "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -H 'x-ocelaudit-source: ui' \
  -X POST "$BASE_URL/api/v1/search" --data '{"q":"Volga Shipping LLC"}' | jq -r '.audit_id')
ui_source=$(auth_curl "$BASE_URL/api/v1/audit/$ui_audit_id" | jq -r '.source')
if [ "$ui_source" = "ui" ]; then _pass_msg "X-OcelAudit-Source: ui propagates to audit row"
else _fail_msg "ui source propagation" "expected ui, got $ui_source"; fi

# Alias match → still RED (alias "Mueller, Hans" on the diacritic record).
body=$(search '{"q":"Mueller, Hans"}')
top_id=$(echo "$body" | jq -r '.hits[0].entry_id')
top_tlp=$(echo "$body" | jq -r '.tlp')
if [ "$top_id" = "OFAC-SDN-6789" ]; then _pass_msg "search alias-match top1"
else _fail_msg "search alias-match" "got $top_id"; fi
if [ "$top_tlp" = "red" ]; then _pass_msg "search alias-match tlp=red"
else _fail_msg "search alias-match tlp" "got $top_tlp"; fi

# No-match → GREEN, empty hits or all low-score.
body=$(search '{"q":"Buttercup Daydream Industries"}')
green_tlp=$(echo "$body" | jq -r '.tlp')
if [ "$green_tlp" = "green" ]; then _pass_msg "no-match tlp=green"
else _fail_msg "no-match tlp" "expected green, got $green_tlp"; fi

# Source filter restricts the result set.
body=$(search '{"q":"Acme","sources":["EL"]}')
sdn_count=$(echo "$body" | jq '[.hits[] | select(.entry_id == "OFAC-12345")] | length')
if [ "$sdn_count" = "0" ]; then _pass_msg "source-filtered search excludes other lists"
else _fail_msg "source filter" "OFAC-12345 (SDN) leaked in"; fi

# /api/v1/search/autocomplete for "Acm" finds the surface form.
auto=$(auth_curl "$BASE_URL/api/v1/search/autocomplete?q=Acm" | jq 'length')
if [ "$auto" -ge 1 ]; then _pass_msg "autocomplete returned $auto suggestions"
else _fail_msg "autocomplete" "expected >=1, got $auto"; fi

# /api/v1/search without auth → 401.
unauth_search=$(curl -sS -o /dev/null -w "%{http_code}" -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" --data '{"q":"x"}')
if [ "$unauth_search" = "401" ]; then _pass_msg "/search unauth -> 401"
else _fail_msg "/search unauth" "expected 401, got $unauth_search"; fi

finish
