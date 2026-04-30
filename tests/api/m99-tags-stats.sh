#!/usr/bin/env bash
# tests/api/m99-tags-stats.sh — M13 surface: per-hit tags on /search,
# top_hits snapshot on /audit/{id}, and the /api/v1/csl/stats endpoint.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

login_as "admin" "${ADMIN_PASSWORD:?}"
auth_curl -X POST "$BASE_URL/api/v1/csl/refresh?source=seed" >/dev/null

# 1. /api/v1/csl/stats reports the seed corpus with by-source / by-entity-type
#    breakdowns.
stats=$(auth_curl "$BASE_URL/api/v1/csl/stats")
count=$(echo "$stats" | jq -r '.count')
if [ "$count" -ge 12 ]; then _pass_msg "/csl/stats count = $count (>=12)"
else _fail_msg "/csl/stats count" "expected >=12, got $count"; fi

src_codes=$(echo "$stats" | jq -r '.by_source[].code' | sort -u | paste -sd, -)
if echo "$src_codes" | grep -q "SDN"; then _pass_msg "/csl/stats by_source includes SDN"
else _fail_msg "/csl/stats by_source" "missing SDN, got $src_codes"; fi

ent_types=$(echo "$stats" | jq -r '.by_entity_type[].entity_type' | sort -u | paste -sd, -)
if [[ "$ent_types" == *"individual"* && "$ent_types" == *"entity"* ]]; then
  _pass_msg "/csl/stats by_entity_type covers individual + entity"
else
  _fail_msg "/csl/stats by_entity_type" "got $ent_types"
fi

# Sources have agency_url for known codes (SDN should resolve).
sdn_url=$(echo "$stats" | jq -r '.by_source[] | select(.code=="SDN") | .agency_url')
if [[ "$sdn_url" == *"ofac.treasury.gov"* ]]; then
  _pass_msg "/csl/stats by_source[SDN].agency_url is OFAC"
else
  _fail_msg "/csl/stats SDN agency_url" "got $sdn_url"
fi

# 2. /api/v1/search hits carry .tags{source_list, entity_type, programs, nationalities}.
login_as "compliance" "${COMPLIANCE_PASSWORD:?}"
search_body=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" --data '{"q":"ACME HOLDINGS PYONGYANG"}')
top_tags_src=$(echo "$search_body" | jq -r '.hits[0].tags.source_list')
top_tags_ent=$(echo "$search_body" | jq -r '.hits[0].tags.entity_type')
top_tags_progs=$(echo "$search_body" | jq -r '.hits[0].tags.programs | length')
top_tags_nats=$(echo "$search_body" | jq -r '.hits[0].tags.nationalities | length')
if [ "$top_tags_src" = "SDN" ]; then _pass_msg "search hit.tags.source_list = SDN"
else _fail_msg "tags.source_list" "got $top_tags_src"; fi
if [ "$top_tags_ent" = "entity" ]; then _pass_msg "search hit.tags.entity_type = entity"
else _fail_msg "tags.entity_type" "got $top_tags_ent"; fi
if [ "$top_tags_progs" -ge 1 ]; then _pass_msg "search hit.tags.programs has $top_tags_progs entry(ies)"
else _fail_msg "tags.programs" "expected >=1, got $top_tags_progs"; fi
if [ "$top_tags_nats" -ge 1 ]; then _pass_msg "search hit.tags.nationalities has $top_tags_nats entry(ies)"
else _fail_msg "tags.nationalities" "expected >=1, got $top_tags_nats"; fi

# 3. The same hit data is persisted in the audit row's top_hits snapshot.
audit_id=$(echo "$search_body" | jq -r '.audit_id')
audit_event=$(auth_curl "$BASE_URL/api/v1/audit/$audit_id")
top_hits_count=$(echo "$audit_event" | jq -r '.top_hits | length')
if [ "$top_hits_count" -ge 1 ]; then _pass_msg "/audit/{id}.top_hits has $top_hits_count snapshot(s)"
else _fail_msg "audit top_hits" "expected >=1, got $top_hits_count"; fi
audit_top_src=$(echo "$audit_event" | jq -r '.top_hits[0].tags.source_list')
if [ "$audit_top_src" = "SDN" ]; then _pass_msg "/audit/{id}.top_hits[0].tags.source_list = SDN"
else _fail_msg "audit top_hits source" "got $audit_top_src"; fi

# 4. The review queue surfaces top_hits inline (so reviewers don't need
#    a second round-trip per item).
near_audit=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" --data '{"q":"ACME HOLDING PYONGYANG"}' | jq -r '.audit_id')
queue=$(auth_curl "$BASE_URL/api/v1/review")
queue_top_hits=$(echo "$queue" | jq -r ".items[] | select(.audit_id == \"$near_audit\") | .top_hits | length")
if [ "$queue_top_hits" -ge 1 ]; then
  _pass_msg "/review queue item has top_hits inlined ($queue_top_hits)"
else
  _fail_msg "/review top_hits inline" "expected >=1, got $queue_top_hits"
fi

finish
