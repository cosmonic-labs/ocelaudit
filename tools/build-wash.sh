#!/usr/bin/env bash
# build-wash.sh — escape hatch ONLY.
#
# Default path for OcelAudit: use the released `wash` 2.0.4 binary on
# $PATH plus `.wash/config.yaml` with `dev.wasip3: true`. You should
# almost never run this script.
#
# When to run this:
#   - A WASI P3 capability we depend on lives on upstream main but is
#     not in any tagged 2.0.x release.
#   - You need to reproduce a behaviour that's broken in 2.0.4 but fixed
#     upstream, before a release lands.
#
# When NOT to run this:
#   - "Just to be safe" — don't. The released 2.0.4 binary is the
#     supported path. Re-building it from source diverges your machine
#     from what CI builds against.
#
# Surface the trigger explicitly when you do invoke it.

set -euo pipefail

cd "$(dirname "$0")/.."

SHA=$(awk -F': *' '/^escape-hatch-sha/ {print $2}' tools/wash-version.txt)
if [[ -z "${SHA:-}" ]]; then
  echo "ERROR: tools/wash-version.txt is missing escape-hatch-sha." >&2
  exit 1
fi

BUILD_DIR=".cache/wasmcloud-source"
mkdir -p .cache

if [[ ! -d "$BUILD_DIR/.git" ]]; then
  git clone https://github.com/wasmCloud/wasmCloud.git "$BUILD_DIR"
fi

cd "$BUILD_DIR"
git fetch origin
git checkout "$SHA"

cargo build -p wash --features wasip3 --release

BIN="$(pwd)/target/release/wash"
echo
echo "Built wash from source at:"
echo "  $BIN"
echo
echo "To use this binary, prepend its directory to PATH for this shell:"
echo "  export PATH=\"$(dirname "$BIN"):\$PATH\""
echo
echo "Then verify:"
echo "  wash --version    # should match the SHA-embedded version"
echo "  wash config show  # should still round-trip dev.wasip3: true"
