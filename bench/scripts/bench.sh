#!/bin/bash
# Thin compatibility wrapper for the consolidated cross-engine benchmark.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
for engine in sass-embedded wasm napi native; do
  node "$SCRIPT_DIR/cross-engine.mjs" --engine "$engine" --fixture uswds
done
