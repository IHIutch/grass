#!/usr/bin/env bash
set -euo pipefail

# Profile-Guided Optimization build for grass.
#
# The default profile is collected from four pinned real-world projects:
# USWDS exercises the @use/@forward module graph, Bootstrap exercises the
# legacy @import and @each paths, Tabler exercises @extend and selector
# machinery, and Font Awesome exercises value/string interpolation and CSS
# serialization. PROFILE_RUNS applies independently to each project.
# In the 2026-07-14 measurement on this checkout, the multi-project profile
# reduced instructions by 10.35%--14.38% on trained entries and
# 11.35%--15.19% on held-out Mastodon, Vuetify, and Grafana. The local build
# took 175.43 s wall; these are measurement notes, not a portable guarantee.
#
# Usage:
#   ./build-pgo.sh                    # Build PGO-optimized binary
#   ./build-pgo.sh --benchmark        # Build + benchmark the first workload
#   ./build-pgo.sh --clean            # Remove PGO artifacts
#
# PGO_TRAINING_SET is a comma-separated project list and defaults to
# uswds,bootstrap,tabler,font-awesome. Set it to one project to reproduce an
# old single-project regime. PGO_WORKLOAD/PGO_WORKLOAD_FLAGS remain a
# single-entry escape hatch for CI or experiments.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR" && pwd)"
source "$REPO_ROOT/bench/fixtures/resolve.sh"

CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
PGO_DIR="/tmp/grass-pgo-$$"
if [ "${1:-}" = "--clean" ]; then
    rm -rf /tmp/grass-pgo-*
    echo "Cleaned PGO artifacts."
    exit 0
fi

PROFILE_RUNS="${PROFILE_RUNS:-3}"
PGO_INPUT_DIR=""
CORPUS_ROOT="${PGO_CORPUS_ROOT:-$REPO_ROOT/bench/real-world/.cache}"
FETCHED_ROOT="$REPO_ROOT/bench/fixtures/fetched"
TARGET_BINARY="$REPO_ROOT/target/release/grass"

cleanup() { rm -rf "$PGO_DIR" "$PGO_INPUT_DIR"; }
trap cleanup EXIT

case "${1:-}" in
    --benchmark)
        BENCHMARK=1
        ;;
    *)
        BENCHMARK=0
        ;;
esac

if ! [[ "$PROFILE_RUNS" =~ ^[1-9][0-9]*$ ]]; then
    echo "Error: PROFILE_RUNS must be a positive integer" >&2
    exit 1
fi

fixture_root() {
    local name="$1"
    if [ -d "$CORPUS_ROOT/$name" ]; then
        printf '%s\n' "$CORPUS_ROOT/$name"
    elif [ -d "$FETCHED_ROOT/$name" ]; then
        printf '%s\n' "$FETCHED_ROOT/$name"
    else
        echo "Error: pinned corpus project '$name' is unavailable." >&2
        echo "Run: bash bench/fixtures/fetch.sh pgo" >&2
        exit 1
    fi
}

PGO_INPUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/grass-pgo-input.XXXXXX")"
TRAINING_SET="${PGO_TRAINING_SET:-uswds,bootstrap,tabler,font-awesome}"
IFS=',' read -r -a TRAINING_PROJECTS <<< "$TRAINING_SET"

ENTRY=""
LOAD_PATH=""
resolve_training_project() {
    local project="$1" root
    case "$project" in
        uswds)
            root="$(resolve_fixture_root uswds)"
            printf '@use "uswds";\n' > "$PGO_INPUT_DIR/input.scss"
            ENTRY="$PGO_INPUT_DIR/input.scss"
            LOAD_PATH="$root/packages"
            ;;
        bootstrap)
            root="$(resolve_fixture_root bootstrap)"
            if [ -f "$root/scss/bootstrap.scss" ]; then
                ENTRY="$root/scss/bootstrap.scss"
                LOAD_PATH="$root/scss"
            else
                ENTRY="$root/bootstrap-bench/scss/bootstrap.scss"
                LOAD_PATH="$root/bootstrap-bench/scss"
            fi
            ;;
        tabler)
            root="$(fixture_root tabler)"
            ENTRY="$root/core/scss/tabler.scss"
            LOAD_PATH=""
            ;;
        font-awesome)
            root="$(fixture_root font-awesome)"
            ENTRY="$root/scss/fontawesome.scss"
            LOAD_PATH=""
            ;;
        *)
            echo "Error: unknown PGO training project '$project'" >&2
            exit 1
            ;;
    esac
    [ -f "$ENTRY" ] || { echo "Error: training entry is missing: $ENTRY" >&2; exit 1; }
}

if [ -n "${PGO_WORKLOAD:-}" ]; then
    [ -f "$PGO_WORKLOAD" ] || { echo "Error: workload file '$PGO_WORKLOAD' not found." >&2; exit 1; }
else
    for project in "${TRAINING_PROJECTS[@]}"; do
        [ -n "$project" ] || { echo "Error: PGO_TRAINING_SET contains an empty project" >&2; exit 1; }
        resolve_training_project "$project"
    done
fi

# Find llvm-profdata. The rustup llvm-tools-preview copy is the safest choice
# when a system LLVM is unavailable; the existing precedence is retained for
# compatibility with the release runners.
if command -v llvm-profdata &>/dev/null; then
    PROFDATA="llvm-profdata"
elif xcrun --find llvm-profdata &>/dev/null 2>&1; then
    PROFDATA="xcrun llvm-profdata"
elif [ -x "$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/llvm-profdata" ]; then
    PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/llvm-profdata"
else
    echo "Error: llvm-profdata not found. Install Xcode, LLVM, or run: rustup component add llvm-tools-preview" >&2
    exit 1
fi

echo "=== Step 1/4: Building instrumented binary ==="
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" $CARGO build --release 2>&1 | grep -E "Compiling grass |Finished"

run_default_workload() {
    local entry="$1" load_path="$2"
    if [ -n "$load_path" ]; then
        "$TARGET_BINARY" "$entry" --style=expanded --no-source-map -I "$load_path" > /dev/null 2>&1
    else
        "$TARGET_BINARY" "$entry" --style=expanded --no-source-map > /dev/null 2>&1
    fi
}

run_override_workload() {
    local -a flags=()
    read -r -a flags <<< "${PGO_WORKLOAD_FLAGS:---style=expanded -I bench/fixtures/packages}"
    "$TARGET_BINARY" "$PGO_WORKLOAD" "${flags[@]}" > /dev/null 2>&1
}

echo "=== Step 2/4: Collecting profile data ($PROFILE_RUNS runs per project) ==="
if [ -n "${PGO_WORKLOAD:-}" ]; then
    for i in $(seq 1 "$PROFILE_RUNS"); do
        run_override_workload
        printf "."
    done
else
    for project in "${TRAINING_PROJECTS[@]}"; do
        resolve_training_project "$project"
        echo ""
        echo "Training: $project ($ENTRY)"
        for i in $(seq 1 "$PROFILE_RUNS"); do
            run_default_workload "$ENTRY" "$LOAD_PATH"
            printf "."
        done
    done
fi
echo " done"

echo "=== Step 3/4: Merging profile data ==="
$PROFDATA merge -o "$PGO_DIR/merged.profdata" "$PGO_DIR"/*.profraw
echo "Merged $(ls "$PGO_DIR"/*.profraw | wc -l | tr -d ' ') profiles"

echo "=== Step 4/4: Building PGO-optimized binary ==="
RUSTFLAGS="-Cprofile-use=$PGO_DIR/merged.profdata" $CARGO build --release 2>&1 | grep -E "Compiling grass |Finished"

echo ""
echo "PGO build complete: ./target/release/grass"

if [ "$BENCHMARK" = "1" ] && command -v hyperfine &>/dev/null; then
    echo ""
    echo "=== Benchmarking first training workload ==="
    if [ -n "${PGO_WORKLOAD:-}" ]; then
        benchmark_command=("$TARGET_BINARY" "$PGO_WORKLOAD")
        read -r -a benchmark_flags <<< "${PGO_WORKLOAD_FLAGS:---style=expanded -I bench/fixtures/packages}"
        benchmark_command+=("${benchmark_flags[@]}")
    else
        resolve_training_project "${TRAINING_PROJECTS[0]}"
        benchmark_command=("$TARGET_BINARY" "$ENTRY" --style=expanded --no-source-map)
        [ -n "$LOAD_PATH" ] && benchmark_command+=(-I "$LOAD_PATH")
    fi
    hyperfine --warmup 3 --runs 15 -- "${benchmark_command[@]}"
fi
