#!/usr/bin/env bash
# tests/api/m9-branding.sh — /api/v1/branding endpoint.
#
# Asserts:
#   - GET /api/v1/branding (no auth) returns the default JSON shape
#   - All five fields are present
#   - When a config file is dropped under /data/static/, the response
#     reflects the override; missing keys still fall back to defaults

set -euo pipefail

source "$(dirname "$0")/_lib.sh"
wait_for "$BASE_URL/healthz" 5 || { echo "!! gateway unreachable"; exit 1; }

# /api/v1/branding is public.
expect_status "$BASE_URL/api/v1/branding" 200
body=$(curl -fsS "$BASE_URL/api/v1/branding")
for key in logo_url wordmark video_url primary_color accent_color; do
  has=$(echo "$body" | jq "has(\"$key\")")
  if [ "$has" = "true" ]; then _pass_msg "/branding has '$key'"
  else _fail_msg "/branding key" "missing '$key'"; fi
done

# Default wordmark.
default_wm=$(echo "$body" | jq -r '.wordmark')
if [ "$default_wm" = "OcelAudit" ]; then _pass_msg "default wordmark is OcelAudit"
else _fail_msg "default wordmark" "got '$default_wm'"; fi

# Drop an override.
data_dir="$(dirname "$0")/../../.cache/ocelaudit-data/static"
mkdir -p "$data_dir"
cat > "$data_dir/ocelaudit.config.json" <<'EOF'
{ "wordmark": "TestBrand", "accent_color": "#ff00ff" }
EOF

# Re-fetch — overrides apply, missing keys fall back.
overridden=$(curl -fsS "$BASE_URL/api/v1/branding")
got_wm=$(echo "$overridden" | jq -r '.wordmark')
got_accent=$(echo "$overridden" | jq -r '.accent_color')
got_logo=$(echo "$overridden" | jq -r '.logo_url')
if [ "$got_wm" = "TestBrand" ]; then _pass_msg "override wordmark applied"
else _fail_msg "override wordmark" "got '$got_wm'"; fi
if [ "$got_accent" = "#ff00ff" ]; then _pass_msg "override accent_color applied"
else _fail_msg "override accent_color" "got '$got_accent'"; fi
if [ "$got_logo" = "/brand/ocelot.svg" ]; then _pass_msg "missing logo_url falls back to default"
else _fail_msg "logo fallback" "got '$got_logo'"; fi

# Restore.
rm -f "$data_dir/ocelaudit.config.json"

finish
