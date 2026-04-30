#!/usr/bin/env bash
# tests/api/_runner.sh — boot wash dev once, run every tests/api/m*.sh
# script against it, tear down on exit.

set -euo pipefail

cd "$(dirname "$0")/../.."

DEV_HOST_ADDR="${DEV_HOST_ADDR:-127.0.0.1:8000}"
BASE_URL="http://${DEV_HOST_ADDR}"
PID_FILE=".cache/wash-dev.pid"
LOG_FILE=".cache/wash-dev.log"

mkdir -p .cache
# wash dev's volume mount requires the host_path to exist before launch.
# api-gateway's .wash/config.yaml mounts ../../.cache/ocelaudit-data → /data.
mkdir -p .cache/ocelaudit-data
mkdir -p .cache/ocelaudit-data/csl

# Pre-stage the CSL fixture into the volume so M3's /api/v1/csl/refresh
# has something to read (the gateway expects /data/csl/seed.json).
cp tests/fixtures/csl/sample.json .cache/ocelaudit-data/csl/seed.json

# Pre-stage the SPA bundle so the gateway can serve / and /assets/* at
# the volume's /data/static/ path (M6). If the dist isn't built yet,
# this is a no-op — m6-spa.sh will report it as a setup gap.
mkdir -p .cache/ocelaudit-data/static
if [ -d ui/dist ]; then
  cp -R ui/dist/* .cache/ocelaudit-data/static/
fi

cleanup() {
  local code=$?
  if [ -f "$PID_FILE" ]; then
    pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      sleep 1
      kill -9 "$pid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE"
  fi
  exit $code
}
trap cleanup EXIT INT TERM

echo ">> booting wash dev for tests/api ..."
( cd components/api-gateway && wash dev >"$(pwd)/../../$LOG_FILE" 2>&1 ) &
echo $! > "$PID_FILE"

# Wait for the dev server to come up.
deadline=$(( $(date +%s) + 60 ))
ready=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  if curl -fsS -o /dev/null -m 1 "$BASE_URL/" 2>/dev/null; then ready=1; break; fi
  sleep 0.5
done
if [ "$ready" -ne 1 ]; then
  echo "!! wash dev did not become ready within 60s; tail of log:"
  tail -n 50 "$LOG_FILE" || true
  exit 1
fi

# Demo seed credentials are fixed values per the storage crate
# constants (DEMO_ADMIN_PASSWORD / DEMO_COMPLIANCE_PASSWORD). The
# gateway still logs them once on a fresh boot for visibility.
export ADMIN_PASSWORD="admin"
export COMPLIANCE_PASSWORD="compliance"

echo ">> running tests/api/m*.sh against $BASE_URL"
fail=0
for script in tests/api/m*.sh; do
  [ -f "$script" ] || continue
  echo
  echo "-- $script --"
  if ! BASE_URL="$BASE_URL" bash "$script"; then
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo
  echo "!! one or more tests/api/*.sh failed"
  exit 1
fi

echo
echo ">> all tests/api/*.sh passed"
