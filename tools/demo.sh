#!/usr/bin/env bash
# tools/demo.sh — cold-start bootstrap for OcelAudit. Per PLAN.md M10:
# from cold-clone to working login in under 5 minutes on a clean machine.
#
# What it does (in order):
#   1. Verify prereqs (wash, wkg, cargo, pnpm).
#   2. Build all components + the SPA.
#   3. Wipe + recreate the volume host_path so seeds run fresh.
#   4. Stage the CSL fixture + SPA bundle into the volume.
#   5. Boot `wash dev` in the background, capture stderr.
#   6. Wait for /healthz, scrape the seed credentials.
#   7. POST /api/v1/csl/refresh as admin (so search has data).
#   8. Print the login URL + the freshly seeded credentials.
#   9. Optionally open the browser. Wait for Ctrl-C.

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

# ---------- volume layout ----------

DATA="$(pwd)/.cache/ocelaudit-data"
LOG="$(pwd)/.cache/wash-dev.log"
PID="$(pwd)/.cache/wash-dev.pid"

mkdir -p .cache
rm -rf "$DATA"
mkdir -p "$DATA/csl" "$DATA/static"

cp tests/fixtures/csl/sample.json "$DATA/csl/seed.json"
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

# ---------- scrape creds + load CSL ----------

seed_line=$(grep -m1 "ocelaudit: seeded users.json" "$LOG" || true)
if [ -z "$seed_line" ]; then
  echo "!! seeded-credentials line not found in wash dev log"
  exit 1
fi
admin_pw=$(printf "%s" "$seed_line" | sed -E 's/.*admin password: ([^ ]+) .*/\1/')
compl_pw=$(printf "%s" "$seed_line" | sed -E 's/.*compliance password: ([^ ]+).*/\1/')

echo "==> ingesting CSL fixture …"
jar=$(mktemp)
trap "rm -f \"$jar\"; cleanup" EXIT INT TERM
curl -fsS -c "$jar" -H 'content-type: application/json' \
  -X POST "http://127.0.0.1:8000/api/v1/auth/login" \
  --data "$(printf '{"username":"admin","password":"%s"}' "$admin_pw")" >/dev/null
ingest=$(curl -fsS -b "$jar" -X POST "http://127.0.0.1:8000/api/v1/csl/refresh" | jq -r '.ingested')

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
  │   CSL records : $ingest (from tests/fixtures/csl/sample.json)
  │
  │   cold-start  : ${elapsed}s   (budget: ${budget}s)
  │
  │   Walkthrough : docs/demo-script.md
  │   Stop demo   : Ctrl-C
  │
  └───────────────────────────────────────────────────────────────┘

EOF

# Optionally open the browser. macOS has 'open', Linux has 'xdg-open'.
if [ "${NO_BROWSER:-}" != "1" ]; then
  if command -v open >/dev/null 2>&1; then
    (sleep 1 && open "http://127.0.0.1:8000/") &
  elif command -v xdg-open >/dev/null 2>&1; then
    (sleep 1 && xdg-open "http://127.0.0.1:8000/") &
  fi
fi

wait
