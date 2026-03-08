#!/bin/bash
# Quick performance check for grass compiler.
# Compiles USWDS 3 times with the release binary and reports median time.
# Run from the prototype/ directory.
#
# Usage: ./perf-check.sh [path/to/grass]

set -e

GRASS="${1:-../target/release/grass}"
BENCH_FILE="/tmp/_grass_perf_check.scss"
LOAD_PATH="$(cd "$(dirname "$0")/packages" && pwd)"

if [ ! -x "$GRASS" ]; then
  echo "ERROR: grass binary not found at $GRASS"
  echo "Run: ~/.cargo/bin/cargo build --release"
  exit 1
fi

if [ ! -d "$LOAD_PATH/uswds" ]; then
  echo "ERROR: USWDS packages not found at $LOAD_PATH"
  exit 1
fi

echo '@use "uswds";' > "$BENCH_FILE"

times=()
for i in 1 2 3; do
  start=$(python3 -c "import time; print(time.time())")
  "$GRASS" "$BENCH_FILE" --style=expanded -I "$LOAD_PATH" > /dev/null 2>&1
  end=$(python3 -c "import time; print(time.time())")
  ms=$(python3 -c "print(int(($end - $start) * 1000))")
  times+=("$ms")
  echo "  Run $i: ${ms}ms"
done

rm -f "$BENCH_FILE"

# Sort and take median
IFS=$'\n' sorted=($(sort -n <<<"${times[*]}")); unset IFS
median="${sorted[1]}"

echo ""
echo "PERF: grass native USWDS compile: ${median}ms (median of 3)"

# Compare against baseline if available
BASELINE_FILE="$(dirname "$0")/.perf-baseline"
if [ -f "$BASELINE_FILE" ]; then
  baseline=$(cat "$BASELINE_FILE")
  delta=$(python3 -c "
b=$baseline; m=$median
pct = (m - b) / b * 100
sign = '+' if pct > 0 else ''
print(f'{sign}{pct:.1f}% vs baseline ({b}ms)')
")
  echo "PERF: $delta"

  # Warn if >5% regression
  regression=$(python3 -c "print('yes' if ($median - $baseline) / $baseline > 0.05 else 'no')")
  if [ "$regression" = "yes" ]; then
    echo ""
    echo "⚠️  WARNING: >5% performance regression detected!"
    echo "    Baseline: ${baseline}ms → Current: ${median}ms"
    echo "    Review changes before committing."
  fi
else
  echo "PERF: No baseline found. Saving current as baseline."
  echo "$median" > "$BASELINE_FILE"
fi
