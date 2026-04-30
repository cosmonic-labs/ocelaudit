#!/usr/bin/env bash
# tools/build-component.sh — invoked by `.wash/config.yaml`'s build.command.
#
# Two-stage build:
#   1. `wkg wit fetch -t wit` from the component's directory so it
#      resolves the local WIT path overrides (wash 2.0.x's bundled wkg
#      mis-decodes text-WIT, so we use the standalone wkg here).
#   2. `cargo auditable build` from the repo root so the cargo workspace
#      is in scope.
#
# The artefact lands at target/wasm32-wasip2/release/ocelaudit_api_gateway.wasm
# regardless of the invocation directory (cargo always writes to the
# workspace target dir).

set -euo pipefail

cd "$(dirname "$0")/.."

# Stage 1 — fetch WIT deps for the component.
( cd components/api-gateway \
  && rm -f wkg.lock \
  && wkg wit fetch -t wit ) >/dev/null

# Stage 2 — build the wasm.
cargo auditable build --target wasm32-wasip2 --release -p ocelaudit-api-gateway
