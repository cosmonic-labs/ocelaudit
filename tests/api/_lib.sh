# Shared helpers for OcelAudit API integration tests.
# Source from each tests/api/*.sh script:  source "$(dirname "$0")/_lib.sh"

: "${BASE_URL:=http://127.0.0.1:8000}"
: "${VERBOSE:=0}"

_pass=0
_fail=0

CHECK="\xe2\x9c\x93"   # ✓
CROSS="\xe2\x9c\x97"   # ✗

_pass_msg() { _pass=$((_pass+1)); printf "  ${CHECK} %s\n" "$1"; }
_fail_msg() { _fail=$((_fail+1)); printf "  ${CROSS} %s\n     %s\n" "$1" "$2"; }

# wait_for URL [timeout-seconds]
wait_for() {
  local url="$1"; local timeout="${2:-30}"
  local deadline=$(( $(date +%s) + timeout ))
  while [ "$(date +%s)" -lt "$deadline" ]; do
    if curl -fsS -o /dev/null -m 1 "$url"; then return 0; fi
    sleep 0.5
  done
  return 1
}

# expect_status URL EXPECTED [METHOD] [BODY]
expect_status() {
  local url="$1"; local expected="$2"; local method="${3:-GET}"; local body="${4:-}"
  local actual
  if [ -n "$body" ]; then
    actual=$(curl -sS -o /dev/null -w "%{http_code}" -X "$method" -H 'content-type: application/json' --data "$body" "$url" || echo 000)
  else
    actual=$(curl -sS -o /dev/null -w "%{http_code}" -X "$method" "$url" || echo 000)
  fi
  if [ "$actual" = "$expected" ]; then
    _pass_msg "$method $url -> $expected"
  else
    _fail_msg "$method $url" "expected $expected, got $actual"
  fi
}

# expect_body_contains URL NEEDLE
expect_body_contains() {
  local url="$1"; local needle="$2"
  local body
  body=$(curl -fsS -m 5 "$url" || true)
  if printf '%s' "$body" | grep -qF -- "$needle"; then
    _pass_msg "$url body contains '$needle'"
  else
    _fail_msg "$url body" "did not contain '$needle' (got: $(printf '%s' "$body" | head -c 80)...)"
  fi
}

# expect_json_field URL JQ-FILTER EXPECTED
expect_json_field() {
  local url="$1"; local filter="$2"; local expected="$3"
  local actual
  actual=$(curl -fsS -m 5 "$url" | jq -r "$filter" 2>/dev/null || echo "<jq-error>")
  if [ "$actual" = "$expected" ]; then
    _pass_msg "$url $filter == $expected"
  else
    _fail_msg "$url $filter" "expected $expected, got $actual"
  fi
}

# Print summary and exit non-zero on any failure.
finish() {
  echo
  if [ "$_fail" -eq 0 ]; then
    printf "${CHECK} %s passed\n" "$_pass"
    exit 0
  else
    printf "${CROSS} %s passed, %s failed\n" "$_pass" "$_fail"
    exit 1
  fi
}
