#!/bin/bash
# Quick performance check for grass compiler.
# Compiles USWDS with the release binary and reports the median time.
# Uses hyperfine when it's on PATH (--warmup 5 --runs 15) for reliable
# numbers; falls back to a 3-run smoke test otherwise.
# Run from the repository root or from any directory.
#
# Usage: ./perf-check.sh [path/to/grass]
#
# Fixture resolution: by default looks for packages/uswds next to this
# script. The fixture is untracked, so fresh git worktrees won't have it —
# set PERF_FIXTURE_DIR to point at a directory containing packages/.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GRASS="${1:-$REPO_ROOT/target/release/grass}"
BENCH_FILE="/tmp/_grass_perf_check.scss"
if [ -n "${PERF_FIXTURE_DIR:-}" ]; then
  FIXTURE_DIR="$PERF_FIXTURE_DIR"
elif [ -d "$REPO_ROOT/bench/fixtures/packages/uswds" ]; then
  FIXTURE_DIR="$REPO_ROOT/bench/fixtures"
else
  FIXTURE_DIR="$REPO_ROOT/prototype"
fi
LOAD_PATH="$FIXTURE_DIR/packages"

if [ ! -x "$GRASS" ]; then
  echo "ERROR: grass binary not found at $GRASS"
  echo "Run: ~/.cargo/bin/cargo build --release"
  exit 1
fi

if [ ! -d "$LOAD_PATH/uswds" ]; then
  echo "ERROR: USWDS fixture not found at $LOAD_PATH/uswds"
  echo ""
  echo "This fixture (bench/fixtures/packages/uswds) is untracked and won't exist in a"
  echo "fresh git worktree. Either:"
  echo "  - run this from the primary checkout, where the fixture is already populated, or"
  echo "  - set PERF_FIXTURE_DIR to a directory containing packages/uswds, e.g.:"
  echo "      PERF_FIXTURE_DIR=/path/to/checkout/with/packages ./bench/scripts/perf-check.sh"
  exit 2
fi

echo '@use "uswds";' > "$BENCH_FILE"

median=""
if command -v hyperfine >/dev/null 2>&1; then
  JSON_FILE="/tmp/_grass_perf_check.json"
  hyperfine --warmup 5 --runs 15 \
    --export-json "$JSON_FILE" \
    "$GRASS $BENCH_FILE --style=expanded -I $LOAD_PATH"
  median_secs=$(python3 -c "import json; print(json.load(open('$JSON_FILE'))['results'][0]['median'])")
  median=$(python3 -c "print(int($median_secs * 1000))")
  rm -f "$JSON_FILE"
else
  echo "NOTE: hyperfine not found on PATH — falling back to a 3-run smoke test."
  echo "      This is smoke-test-only; install hyperfine for reliable numbers."
  times=()
  for i in 1 2 3; do
    start=$(python3 -c "import time; print(time.time())")
    "$GRASS" "$BENCH_FILE" --style=expanded -I "$LOAD_PATH" > /dev/null 2>&1
    end=$(python3 -c "import time; print(time.time())")
    ms=$(python3 -c "print(int(($end - $start) * 1000))")
    times+=("$ms")
    echo "  Run $i: ${ms}ms"
  done

  # Sort and take median
  IFS=$'\n' sorted=($(sort -n <<<"${times[*]}")); unset IFS
  median="${sorted[1]}"
fi

rm -f "$BENCH_FILE"

echo ""
echo "PERF: grass native USWDS compile: ${median}ms (median)"

# Compare against baseline if available
if [ -f "$REPO_ROOT/bench/.perf-baseline" ]; then
  BASELINE_FILE="$REPO_ROOT/bench/.perf-baseline"
else
  BASELINE_FILE="$REPO_ROOT/prototype/.perf-baseline"
fi
if [ -f "$BASELINE_FILE" ]; then
  baseline=$(cat "$BASELINE_FILE")
  delta=$(python3 -c "
b=$baseline; m=$median
pct = (m - b) / b * 100
sign = '+' if pct > 0 else ''
print(f'{sign}{pct:.1f}% vs baseline ({b}ms)')
")
  echo "PERF: $delta"

  # Fail if >5% regression
  regression=$(python3 -c "print('yes' if ($median - $baseline) / $baseline > 0.05 else 'no')")
  if [ "$regression" = "yes" ]; then
    echo ""
    echo "WARNING: >5% performance regression detected!"
    echo "    Baseline: ${baseline}ms -> Current: ${median}ms"
    echo "    Review changes before committing."
    exit 1
  fi
else
  echo "PERF: No baseline found. Saving current as baseline."
  echo "$median" > "$BASELINE_FILE"
fi
