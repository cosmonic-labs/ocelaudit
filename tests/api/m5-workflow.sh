#!/usr/bin/env bash
# tests/api/m5-workflow.sh — workflow scenarios per PLAN.md M5.
#
# 1. /screen/ofac filters to OFAC source codes only (no EL/UVL leakage).
# 2. /screen/pep returns the disclaimer note in the body.
# 3. RED search → decision = pending-block in /audit/{id}.
# 4. Compliance reviews and blocks → /audit/{id}.decision flips to blocked,
#    history reflects the change.
# 5. YELLOW search → pending-review → cleared.
# 6. /review queue includes pending items, excludes decided ones.
# 7. Citations attach the agency_url to each hit.

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

# Re-seed CSL (other scripts may have run already; idempotent).
login_as "admin" "${ADMIN_PASSWORD:?}"
auth_curl -X POST "$BASE_URL/api/v1/csl/refresh?source=seed" >/dev/null

login_as "compliance" "${COMPLIANCE_PASSWORD:?}"

# 1. /screen/ofac filters to OFAC source lists.
ofac=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/screen/ofac" \
  --data '{"q":"Tehran"}')
non_ofac_in_results=$(echo "$ofac" | jq '[.hits[] | .citation.source_code | select(. == "EL" or . == "UVL" or . == "ITAR-DPL" or . == "DPL")] | length')
if [ "$non_ofac_in_results" = "0" ]; then
  _pass_msg "/screen/ofac excludes non-OFAC sources"
else
  _fail_msg "/screen/ofac filter" "got $non_ofac_in_results non-OFAC hits"
fi
ofac_note=$(echo "$ofac" | jq -r '.note')
if [[ "$ofac_note" == *"OFAC"* ]]; then
  _pass_msg "/screen/ofac includes scope note"
else
  _fail_msg "/screen/ofac note" "got '$ofac_note'"
fi

# 2. /screen/pep returns the disclaimer.
pep=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/screen/pep" \
  --data '{"q":"PLC"}')
pep_note=$(echo "$pep" | jq -r '.note')
if [[ "$pep_note" == *"DISCLAIMER"* ]]; then
  _pass_msg "/screen/pep ships the not-a-real-pep disclaimer"
else
  _fail_msg "/screen/pep disclaimer" "got '$pep_note'"
fi

# 3. RED with exact name match → auto-block initial decision (no review
#    needed). High-similarity-but-not-exact RED would be pending-block;
#    we pick an exact-name query here.
red=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"ACME HOLDINGS PYONGYANG"}')
red_audit=$(echo "$red" | jq -r '.audit_id')
red_decision=$(echo "$red" | jq -r '.decision')
if [ "$red_decision" = "auto-block" ]; then
  _pass_msg "RED exact-match initial decision = auto-block"
else
  _fail_msg "RED initial decision" "expected auto-block, got $red_decision"
fi

# 3b. RED with high-similarity but not-exact → pending-block.
near=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"ACME HOLDING PYONGYANG"}')
near_decision=$(echo "$near" | jq -r '.decision')
if [ "$near_decision" = "pending-block" ]; then
  _pass_msg "RED near-match initial decision = pending-block"
else
  _fail_msg "RED near-match decision" "expected pending-block, got $near_decision"
fi

# Citation on the top hit.
red_top_citation=$(echo "$red" | jq -r '.hits[0].citation.source_code')
red_top_url=$(echo "$red" | jq -r '.hits[0].citation.agency_url')
if [ "$red_top_citation" = "SDN" ]; then _pass_msg "RED top hit citation.source_code = SDN"
else _fail_msg "RED citation" "got $red_top_citation"; fi
if [[ "$red_top_url" == *"ofac.treasury.gov"* ]]; then _pass_msg "RED citation.agency_url present"
else _fail_msg "RED agency_url" "got $red_top_url"; fi

# 4. Use the near-match audit (pending-block) as the review subject —
#    auto-block doesn't go through review by design.
pending_audit="$(echo "$near" | jq -r '.audit_id')"
pre=$(auth_curl "$BASE_URL/api/v1/audit/$pending_audit")
pre_decision=$(echo "$pre" | jq -r '.decision')
pre_history_len=$(echo "$pre" | jq '.history | length')
if [ "$pre_decision" = "pending-block" ]; then _pass_msg "/audit pre-review decision = pending-block"
else _fail_msg "/audit pre-review" "got $pre_decision"; fi
if [ "$pre_history_len" = "0" ]; then _pass_msg "/audit history empty before review"
else _fail_msg "/audit history pre-review" "expected 0, got $pre_history_len"; fi

# Compliance reviews and blocks.
decide_status=$(curl -sS -o /tmp/m5_decide -w "%{http_code}" -b "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/review/$pending_audit/decide" \
  --data '{"decision":"blocked","note":"sanctioned entity confirmed"}')
if [ "$decide_status" = "200" ]; then _pass_msg "POST /review/{near}/decide -> 200"
else _fail_msg "POST /review/{near}/decide" "expected 200, got $decide_status"; fi

# /audit reflects the new decision.
post=$(auth_curl "$BASE_URL/api/v1/audit/$pending_audit")
post_decision=$(echo "$post" | jq -r '.decision')
post_initial=$(echo "$post" | jq -r '.initial_decision')
post_history_len=$(echo "$post" | jq '.history | length')
if [ "$post_decision" = "blocked" ]; then _pass_msg "/audit post-review decision = blocked"
else _fail_msg "/audit post-review" "got $post_decision"; fi
if [ "$post_initial" = "pending-block" ]; then _pass_msg "/audit initial_decision preserved"
else _fail_msg "/audit initial_decision" "got $post_initial"; fi
if [ "$post_history_len" = "1" ]; then _pass_msg "/audit history has 1 entry"
else _fail_msg "/audit history post-review" "got $post_history_len"; fi
post_decided_by=$(echo "$post" | jq -r '.history[0].decided_by')
if [ "$post_decided_by" = "compliance" ]; then _pass_msg "/audit history records decided_by=compliance"
else _fail_msg "/audit decided_by" "got $post_decided_by"; fi

# 5. YELLOW search → pending-review → cleared.
yellow=$(auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"Tehran Aerospace"}')
yellow_audit=$(echo "$yellow" | jq -r '.audit_id')
yellow_decision=$(echo "$yellow" | jq -r '.decision')
if [ "$yellow_decision" = "pending-review" ]; then
  _pass_msg "YELLOW search initial decision = pending-review"
else
  _fail_msg "YELLOW initial decision" "expected pending-review, got $yellow_decision"
fi

curl -sS -o /dev/null -b "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/review/$yellow_audit/decide" \
  --data '{"decision":"cleared","note":"different entity"}'
yellow_post=$(auth_curl "$BASE_URL/api/v1/audit/$yellow_audit")
yellow_post_decision=$(echo "$yellow_post" | jq -r '.decision')
if [ "$yellow_post_decision" = "cleared" ]; then
  _pass_msg "/audit/{yellow} post-cleared decision = cleared"
else
  _fail_msg "/audit/{yellow} post-cleared" "got $yellow_post_decision"
fi

# 6. /review queue includes only pending items. auto-block + auto-green
#    do NOT enter the queue; only pending-review + pending-block do.
queue=$(auth_curl "$BASE_URL/api/v1/review")
pending_count=$(echo "$queue" | jq -r '.count')
auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"ACME HOLDING PYONGYANG"}' >/dev/null
queue2=$(auth_curl "$BASE_URL/api/v1/review")
pending_count2=$(echo "$queue2" | jq -r '.count')
if [ "$pending_count2" -gt "$pending_count" ]; then
  _pass_msg "/review queue grew when a pending item was added ($pending_count -> $pending_count2)"
else
  _fail_msg "/review queue growth" "expected >, got $pending_count -> $pending_count2"
fi

# 6b. auto-block does NOT enter the queue.
queue3_before=$(auth_curl "$BASE_URL/api/v1/review" | jq -r '.count')
auth_curl -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/search" \
  --data '{"q":"ACME HOLDINGS PYONGYANG"}' >/dev/null
queue3_after=$(auth_curl "$BASE_URL/api/v1/review" | jq -r '.count')
if [ "$queue3_after" = "$queue3_before" ]; then
  _pass_msg "/review queue does NOT grow on auto-block ($queue3_before -> $queue3_after)"
else
  _fail_msg "/review queue auto-block" "expected unchanged, got $queue3_before -> $queue3_after"
fi

# 7. Bad decide payload: 400 on garbage decision string.
bad_status=$(curl -sS -o /dev/null -w "%{http_code}" -b "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/review/$pending_audit/decide" \
  --data '{"decision":"frobnicate"}')
if [ "$bad_status" = "400" ]; then _pass_msg "decide with bad decision -> 400"
else _fail_msg "decide bad decision" "expected 400, got $bad_status"; fi

# 8. Decide on unknown audit_id: 404.
nf_status=$(curl -sS -o /dev/null -w "%{http_code}" -b "$COOKIE_JAR" \
  -H 'content-type: application/json' \
  -X POST "$BASE_URL/api/v1/review/no-such/decide" \
  --data '{"decision":"cleared"}')
if [ "$nf_status" = "404" ]; then _pass_msg "decide on unknown audit_id -> 404"
else _fail_msg "decide unknown id" "expected 404, got $nf_status"; fi

finish
