#!/bin/bash
# Cross-engine USWDS compilation benchmark using hyperfine.
# Compares: grass native (CLI) vs grass napi vs grass WASM vs sass-embedded (dart-sass)
#
# Usage: ./bench.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GRASS="${SCRIPT_DIR}/../target/release/grass"
NAPI="${SCRIPT_DIR}/../crates/napi/grass.darwin-arm64.node"
BENCH_FILE="/tmp/_grass_bench.scss"
LOAD_PATH="${SCRIPT_DIR}/packages"

echo '@use "uswds";' > "$BENCH_FILE"

COMMANDS=()
NAMES=()

# Always include sass-embedded and WASM
COMMANDS+=("node ${SCRIPT_DIR}/bench-sass.js")
NAMES+=("sass-embedded")

COMMANDS+=("node ${SCRIPT_DIR}/bench-wasm.js")
NAMES+=("grass WASM")

# Add napi if the .node file exists
if [ -f "$NAPI" ]; then
  COMMANDS+=("node ${SCRIPT_DIR}/bench-napi.js")
  NAMES+=("grass napi")
else
  echo "NOTE: napi addon not found at $NAPI — skipping napi benchmark"
  echo "  Run: ~/.cargo/bin/cargo build --release -p grass_napi && cp -f target/release/libgrass_napi.dylib crates/napi/grass.darwin-arm64.node && codesign -s - -f crates/napi/grass.darwin-arm64.node"
  echo ""
fi

# Add native CLI if binary exists
if [ -x "$GRASS" ]; then
  COMMANDS+=("${GRASS} ${BENCH_FILE} --style=expanded -I ${LOAD_PATH} > /dev/null 2>&1")
  NAMES+=("grass native")
else
  echo "NOTE: grass binary not found at $GRASS — skipping native benchmark"
  echo "  Run: ~/.cargo/bin/cargo build --release"
  echo ""
fi

hyperfine \
  --warmup 1 \
  --runs 5 \
  --export-markdown /tmp/bench-results.md \
  "${COMMANDS[@]}"

rm -f "$BENCH_FILE"

echo ""
echo "Results saved to /tmp/bench-results.md"
