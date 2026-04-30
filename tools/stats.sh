#!/usr/bin/env bash
# tools/stats.sh — print the per-component wasm size table.
#
# Reads the COMPONENTS list from the Makefile so it stays in sync.
# Output format is intentionally README-paste-ready.

set -euo pipefail

cd "$(dirname "$0")/.."

components=$(awk '/^COMPONENTS *:= */ {sub(/^COMPONENTS *:= */, ""); print}' Makefile)

human() {
  local n="$1"
  if [ "$n" -ge 1048576 ]; then
    printf "%.1f MB" "$(echo "scale=1; $n/1048576" | bc)"
  elif [ "$n" -ge 1024 ]; then
    printf "%.1f KB" "$(echo "scale=1; $n/1024" | bc)"
  else
    printf "%d B" "$n"
  fi
}

printf "\n%-28s %14s %14s\n" "component" "release" "release+gz"
printf "%-28s %14s %14s\n" "--------" "-------" "----------"

total_raw=0
total_gz=0
for c in $components; do
  artefact="target/wasm32-wasip2/release/ocelaudit_${c//-/_}.wasm"
  if [ -f "$artefact" ]; then
    raw=$(wc -c < "$artefact" | tr -d ' ')
    gz=$(gzip -c "$artefact" | wc -c | tr -d ' ')
    total_raw=$((total_raw + raw))
    total_gz=$((total_gz + gz))
    printf "%-28s %14s %14s\n" "$c" "$(human "$raw")" "$(human "$gz")"
  fi
done

printf "%-28s %14s %14s\n" "--------" "-------" "----------"
printf "%-28s %14s %14s\n" "total" "$(human "$total_raw")" "$(human "$total_gz")"

# Cold-start budget reminder for the README.
printf "\nCold-start target: < 10 s on a 2024-class laptop.\n"
printf "Run \`time make demo\` to measure end-to-end.\n"
