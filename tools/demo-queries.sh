#!/usr/bin/env bash
# tools/demo-queries.sh — demo script template.
#
# Walks the API surface end-to-end with one or two queries per scenario.
# Drop in your own query strings under each section — the script is
# structural, not data. Keep the comments next to each call to remind
# yourself what you're showing the audience.
#
# Prereqs:
#   - `make demo` is running in another terminal (URL printed in its banner).
#   - The CSL is loaded (you can verify via `make demo`'s "CSL records: N"
#     line or by running this script and watching the metadata block).
#
# Usage:
#   bash tools/demo-queries.sh                # default demo flow
#   USER=admin bash tools/demo-queries.sh     # log in as admin instead

set -euo pipefail

BASE="${BASE_URL:-http://127.0.0.1:8000}"
USER_NAME="${USER:-compliance}"
PASSWORD="${PASSWORD:-OcelAudit}"
JAR="$(mktemp -t ocelaudit-demo-jar.XXXXXX)"
trap 'rm -f "$JAR"' EXIT

# ---------- helpers ----------

# Pretty separator.
section() { printf "\n\033[1;36m── %s ─────────────────────────────────\033[0m\n" "$1"; }

# search "<query>" [extra-json-fragment]
# Posts to /api/v1/search and prints {tlp, decision, top hit, score, source, audit_id}.
search() {
  local q="$1"
  local extra="${2:-}"
  local body
  if [ -n "$extra" ]; then
    body=$(jq -nc --arg q "$q" --argjson extra "$extra" '$extra + {q:$q}')
  else
    body=$(jq -nc --arg q "$q" '{q:$q}')
  fi
  printf "  → %s\n" "$q"
  curl -fsS -b "$JAR" -H 'content-type: application/json' \
    -X POST "$BASE/api/v1/search" --data "$body" \
    | jq -rc '"      tlp=\(.tlp) · decision=\(.decision) · top=\(.hits[0].snippet // "—") · src=\(.hits[0].tags.source_list // "—") · score=\(.hits[0].score // 0 | tostring | .[0:5])"'
}

# screen ofac|pep "<query>" — convenience wrapper for /screen/{ofac,pep}.
screen() {
  local kind="$1"; local q="$2"
  printf "  → /screen/%s %s\n" "$kind" "$q"
  curl -fsS -b "$JAR" -H 'content-type: application/json' \
    -X POST "$BASE/api/v1/screen/$kind" --data "$(jq -nc --arg q "$q" '{q:$q}')" \
    | jq -rc '"      tlp=\(.tlp) · decision=\(.decision) · sources=\(.hits | map(.tags.source_list) | unique | join(","))"'
}

# autocomplete "<prefix>" — GET /search/autocomplete?q=<prefix>.
autocomplete() {
  local prefix="$1"
  printf "  → autocomplete '%s'\n" "$prefix"
  curl -fsS -G -b "$JAR" "$BASE/api/v1/search/autocomplete" \
    --data-urlencode "q=$prefix" | jq -rc '. | join(", ")' | sed 's/^/      /'
}

# ---------- preflight ----------

curl -fsS -m 2 -o /dev/null "$BASE/healthz" \
  || { echo "!! gateway unreachable at $BASE — is \`make demo\` running?"; exit 1; }

section "1) authenticate as $USER_NAME"
curl -fsS -c "$JAR" -H 'content-type: application/json' \
  -X POST "$BASE/api/v1/auth/login" \
  --data "$(jq -nc --arg u "$USER_NAME" --arg p "$PASSWORD" '{username:$u, password:$p}')" \
  | jq -c .

# ---------- CSL state ----------

section "2) what is the database loaded with?"
curl -fsS -b "$JAR" "$BASE/api/v1/csl/metadata" \
  | jq '{count, version, sources: (.sources | length), top_3_sources: (.sources | sort_by(-.count) | .[:3])}'

section "3) deeper stats: by source / by entity type / top programs"
curl -fsS -b "$JAR" "$BASE/api/v1/csl/stats" \
  | jq '{count, by_entity_type, top_5_sources: (.by_source | sort_by(-.count) | .[:5] | map({code, count})), top_5_programs: (.top_programs | .[:5])}'

# ---------- GREEN scenarios (mostly auto-green) ----------
#
# Replace these with your own queries. Goal: real-life screening traffic
# is ~99% miss; show several non-matches in a row before any hit fires.

section "4) GREEN — mostly clean traffic"
search "REPLACE_ME_GREEN_NAME_1"          # e.g. an unrelated company
search "REPLACE_ME_GREEN_NAME_2"          # individual not on any list
search "REPLACE_ME_GREEN_NAME_3"          # vessel name unlikely to hit

# ---------- YELLOW scenarios (pending-review) ----------
#
# Pick queries that share tokens with real CSL entries but aren't exact
# matches. Typos, partial names, or common surnames usually land here.

section "5) YELLOW — partial / fuzzy matches that need review"
search "REPLACE_ME_YELLOW_TYPO"           # near-miss spelling of a known entry
search "REPLACE_ME_YELLOW_SURNAME"        # common surname → multiple loose matches
search "REPLACE_ME_YELLOW_PARTIAL"        # partial of an entity name

# ---------- RED scenarios (auto-block / pending-block) ----------
#
# These should be exact matches on the CSL. The decision will be
# auto-block (exact name/alias) or pending-block (high-similarity).

section "6) RED — definitive matches"
search "REPLACE_ME_RED_EXACT_SDN"         # exact SDN name → auto-block
search "REPLACE_ME_RED_EXACT_BIS"         # exact Entity-List name → auto-block

# ---------- /screen/* convenience endpoints ----------

section "7) /screen/ofac — same query, OFAC-only filter"
screen ofac "REPLACE_ME_FILTER_TARGET"

section "8) /screen/pep — disclaimer + PLC-filtered"
screen pep "REPLACE_ME_PEP_NAME"

# ---------- autocomplete ----------

section "9) autocomplete — surface-form suggestions"
autocomplete "REPLACE"
autocomplete "REPL"

# ---------- audit + review ----------

section "10) recent audit events (last 5)"
curl -fsS -b "$JAR" "$BASE/api/v1/audit?limit=5" \
  | jq -c '.events[] | {audit_id: (.audit_id | .[0:8]), tlp, decision, source, query}'

section "11) review queue depth"
curl -fsS -b "$JAR" "$BASE/api/v1/review" \
  | jq '{count, sample: (.items | .[0:3] | map({audit_id: (.audit_id | .[0:8]), tlp, decision, query}))}'

section "12) /metrics — TLP histogram + counts"
curl -fsS -b "$JAR" "$BASE/api/v1/metrics" \
  | jq '{csl_count, queries_recent, tlp_histogram, last_csl_refresh, queue_depth}'

# ---------- one full audit-detail walk-through ----------

section "13) drill into the most recent audit event (with hit snapshots + history)"
last_id=$(curl -fsS -b "$JAR" "$BASE/api/v1/audit?limit=1" | jq -r '.events[0].audit_id')
if [ -n "$last_id" ] && [ "$last_id" != "null" ]; then
  curl -fsS -b "$JAR" "$BASE/api/v1/audit/$last_id" \
    | jq '{audit_id, tlp, decision, source, top_hits: (.top_hits | map({entry_id, score, snippet, tags: .tags})), history}'
else
  echo "  (no events yet — run more searches)"
fi

printf "\n\033[1;32mdone.\033[0m  Open %s/audit in the browser to see everything we just generated.\n" "$BASE"
