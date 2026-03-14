#!/usr/bin/env bash
set -euo pipefail

# Profile-Guided Optimization build for grass
#
# PGO gives LLVM real runtime data about branch prediction, function hotness,
# and code layout. Typically yields ~11% speedup over a standard release build.
#
# Usage:
#   ./build-pgo.sh                    # Build PGO-optimized binary
#   ./build-pgo.sh --benchmark        # Build + benchmark vs standard release
#   ./build-pgo.sh --clean            # Remove PGO artifacts

CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
PGO_DIR="/tmp/grass-pgo-$$"
WORKLOAD="prototype/packages/uswds/_index-direct.scss"
WORKLOAD_FLAGS="--style=expanded -I prototype/packages"
PROFILE_RUNS=5

case "${1:-}" in
    --clean)
        rm -rf /tmp/grass-pgo-*
        echo "Cleaned PGO artifacts."
        exit 0
        ;;
    --benchmark)
        BENCHMARK=1
        ;;
    *)
        BENCHMARK=0
        ;;
esac

# Find llvm-profdata
if command -v llvm-profdata &>/dev/null; then
    PROFDATA="llvm-profdata"
elif xcrun --find llvm-profdata &>/dev/null 2>&1; then
    PROFDATA="xcrun llvm-profdata"
else
    echo "Error: llvm-profdata not found. Install Xcode or LLVM toolchain."
    exit 1
fi

echo "=== Step 1/4: Building instrumented binary ==="
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" $CARGO build --release 2>&1 | grep -E "Compiling grass |Finished"

echo "=== Step 2/4: Collecting profile data ($PROFILE_RUNS runs) ==="
for i in $(seq 1 $PROFILE_RUNS); do
    ./target/release/grass $WORKLOAD $WORKLOAD_FLAGS > /dev/null 2>&1
    printf "."
done
echo " done"

echo "=== Step 3/4: Merging profile data ==="
$PROFDATA merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw
echo "Merged $(ls "$PGO_DIR"/*.profraw | wc -l | tr -d ' ') profiles"

echo "=== Step 4/4: Building PGO-optimized binary ==="
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" $CARGO build --release 2>&1 | grep -E "Compiling grass |Finished"

# Clean up
rm -rf "$PGO_DIR"

echo ""
echo "PGO build complete: ./target/release/grass"

if [ "$BENCHMARK" = "1" ] && command -v hyperfine &>/dev/null; then
    echo ""
    echo "=== Benchmarking ==="
    hyperfine --warmup 3 --runs 15 \
        "./target/release/grass $WORKLOAD $WORKLOAD_FLAGS > /dev/null"
fi
