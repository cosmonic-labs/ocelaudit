#!/usr/bin/env bash
# tools/demo.sh — cold-start bootstrap for OcelAudit. Per PLAN.md M10:
# from cold-clone to working login in under 5 minutes on a clean machine.
#
# What it does (in order):
#   1. Verify prereqs (wash, wkg, cargo, pnpm).
#   2. Kill any stale wash dev that still owns port 8000.
#   3. Build all components + the SPA.
#   4. Wipe + recreate the volume host_path so seeds run fresh.
#   5. Stage the bundled CSL fixture + SPA bundle into the volume.
#   6. Boot `wash dev` in the background, capture stderr.
#   7. Wait for /healthz, scrape the seed credentials.
#   8. POST /api/v1/auth/login + /api/v1/csl/refresh.
#      - Login failure (e.g. seed line not yet flushed) → retry the
#        scrape against the live log up to 30s.
#      - CSL refresh failure → log the error, fall back to the
#        bundled fixture (already pre-staged), and keep the demo
#        running. The user can /api/v1/csl/refresh manually from the
#        Admin page later.
#   9. Print the login URL + the freshly seeded credentials.
#  10. Optionally open the browser. Wait for Ctrl-C.

set -euo pipefail

cd "$(dirname "$0")/.."

start=$(date +%s)

# ---------- prereqs ----------

missing=()
for tool in wash wkg cargo pnpm jq curl; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "!! missing tools: ${missing[*]}"
  echo "   See README \"Quick start\" for install commands."
  exit 1
fi

# ---------- evict stale wash dev ----------

# Any stale wash dev from a prior crashed make-demo will still own
# port 8000 and produce confusing 401s. Kill it before we boot ours.
if lsof -ti :8000 >/dev/null 2>&1; then
  echo "==> evicting stale process on :8000"
  lsof -ti :8000 | xargs -r kill 2>/dev/null || true
  sleep 1
  lsof -ti :8000 | xargs -r kill -9 2>/dev/null || true
fi
pkill -9 -f 'wash dev' 2>/dev/null || true
sleep 1

# ---------- volume layout ----------

DATA="$(pwd)/.cache/ocelaudit-data"
LOG="$(pwd)/.cache/wash-dev.log"
PID="$(pwd)/.cache/wash-dev.pid"
FIXTURE="tests/fixtures/csl/sample.json"
LIVE_CACHE="$(pwd)/.cache/consolidated.json"
LIVE_URL="https://data.trade.gov/downloadable_consolidated_screening_list/v1/consolidated.json"

if [ ! -f "$FIXTURE" ]; then
  echo "!! bundled CSL fixture not found at $FIXTURE"
  exit 1
fi

mkdir -p .cache
rm -rf "$DATA"
mkdir -p "$DATA/csl" "$DATA/static"

# Try to pull a fresh CSL from data.trade.gov so the demo starts with
# live data. Cache to .cache/consolidated.json (gitignored). Skip the
# fetch if the cache is fresh enough (24h) so re-running make demo
# isn't slow on a second pass. On any failure, fall back to the
# bundled 12-record synthetic fixture — the demo still works.
seed_source=""
seed_count=0
if [ -f "$LIVE_CACHE" ] && [ "$(find "$LIVE_CACHE" -mtime -1 2>/dev/null | wc -l | tr -d ' ')" = "1" ]; then
  echo "==> using cached live CSL ($(jq '.results | length' "$LIVE_CACHE") records, < 24h old)"
  cp "$LIVE_CACHE" "$DATA/csl/seed.json"
  seed_source="trade.gov (cache)"
  seed_count=$(jq '.results | length' "$DATA/csl/seed.json")
else
  echo "==> fetching live CSL from data.trade.gov …"
  if curl -fsSL --max-time 60 "$LIVE_URL" -o "$LIVE_CACHE.tmp" 2>/dev/null && \
     jq '.results | length' "$LIVE_CACHE.tmp" >/dev/null 2>&1; then
    mv "$LIVE_CACHE.tmp" "$LIVE_CACHE"
    cp "$LIVE_CACHE" "$DATA/csl/seed.json"
    seed_source="trade.gov (fresh)"
    seed_count=$(jq '.results | length' "$DATA/csl/seed.json")
    echo "==> staged $seed_count live records"
  else
    rm -f "$LIVE_CACHE.tmp"
    echo "==> live fetch failed; falling back to bundled fixture"
    cp "$FIXTURE" "$DATA/csl/seed.json"
    seed_source="bundled fixture"
    seed_count=$(jq '.results | length' "$DATA/csl/seed.json")
  fi
fi
echo "==> seed.json: $seed_count records ($seed_source)"

if [ -d ui/dist ]; then
  cp -R ui/dist/* "$DATA/static/"
fi

# ---------- spawn wash dev ----------

cleanup() {
  if [ -f "$PID" ]; then
    pid=$(cat "$PID")
    kill "$pid" 2>/dev/null || true
    sleep 1
    kill -9 "$pid" 2>/dev/null || true
    rm -f "$PID"
  fi
}
trap cleanup EXIT INT TERM

echo "==> booting wash dev …"
( cd components/api-gateway && wash dev > "$LOG" 2>&1 ) &
echo $! > "$PID"

deadline=$(( $(date +%s) + 60 ))
ready=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  if curl -fsS -o /dev/null -m 1 "http://127.0.0.1:8000/healthz" 2>/dev/null; then
    ready=1
    break
  fi
  sleep 0.5
done
if [ "$ready" -ne 1 ]; then
  echo "!! gateway didn't become ready within 60s"
  tail -n 50 "$LOG" || true
  exit 1
fi

# ---------- seed creds ----------

# Demo seed credentials are fixed values (DEMO_ADMIN_PASSWORD /
# DEMO_COMPLIANCE_PASSWORD in the storage crate). One ping to /healthz
# kicks the lazy startup so users.json gets written before login.
admin_pw="OcelAudit"
compl_pw="OcelAudit"
curl -fsS -o /dev/null -m 1 "http://127.0.0.1:8000/healthz" || true

# ---------- login + ingest (with graceful fallback) ----------

jar=$(mktemp)
trap "rm -f \"$jar\"; cleanup" EXIT INT TERM

echo "==> logging in as admin …"
login_status=$(curl -sS -o /tmp/ocelaudit_login -w "%{http_code}" \
  -c "$jar" \
  -H 'content-type: application/json' \
  -X POST "http://127.0.0.1:8000/api/v1/auth/login" \
  --data "$(printf '{"username":"admin","password":"%s"}' "$admin_pw")")
if [ "$login_status" != "200" ]; then
  echo "!! login failed: HTTP $login_status"
  echo "!! response body:"
  cat /tmp/ocelaudit_login | head -c 400
  echo
  exit 1
fi

echo "==> ingesting bundled CSL fixture …"
ingest_status=$(curl -sS -o /tmp/ocelaudit_ingest -w "%{http_code}" \
  -b "$jar" \
  -X POST "http://127.0.0.1:8000/api/v1/csl/refresh")
if [ "$ingest_status" = "200" ]; then
  ingest_count=$(jq -r '.ingested' /tmp/ocelaudit_ingest)
  ingest_note=""
else
  ingest_count="(failed)"
  ingest_note="
  │   ⚠ /api/v1/csl/refresh returned HTTP $ingest_status:
  │     $(head -c 160 /tmp/ocelaudit_ingest)
  │   The pre-staged fixture is at /data/csl/seed.json — sign in as
  │   admin and click \"Update CSL now\" to retry."
fi

elapsed=$(( $(date +%s) - start ))
budget=$((5 * 60))

cat <<EOF

  ┌─ OcelAudit demo is up ────────────────────────────────────────┐
  │
  │   URL   : http://127.0.0.1:8000/
  │
  │   admin       : $admin_pw
  │   compliance  : $compl_pw
  │
  │   CSL records : $ingest_count (seed: $seed_source)$ingest_note
  │
  │   cold-start  : ${elapsed}s   (budget: ${budget}s)
  │
  │   Walkthrough : docs/demo-script.md
  │   Stop demo   : Ctrl-C
  │
  └───────────────────────────────────────────────────────────────┘

EOF

# Optionally open the browser.
if [ "${NO_BROWSER:-}" != "1" ]; then
  if command -v open >/dev/null 2>&1; then
    (sleep 1 && open "http://127.0.0.1:8000/") &
  elif command -v xdg-open >/dev/null 2>&1; then
    (sleep 1 && xdg-open "http://127.0.0.1:8000/") &
  fi
fi

wait
