#!/bin/bash
# Collect attribution profiles for the grass compiler on the USWDS fixture.
# Run from the repository root or pass a path to the script.

set -euo pipefail

MODE="${1:-}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GRASS="${GRASS:-$REPO_ROOT/target/release/grass}"
source "$REPO_ROOT/bench/fixtures/resolve.sh"
FIXTURE_DIR="$(resolve_fixture_root uswds)"
LOAD_PATH="$FIXTURE_DIR/packages"
ARTIFACT_DIR="${PROFILE_ARTIFACT_DIR:-/tmp/grass-profile}"
BENCH_FILE="$ARTIFACT_DIR/_grass_profile.scss"
RESTORE_NEEDED=0

restore_plain_binary() {
  if [ "$RESTORE_NEEDED" -eq 1 ]; then
    RESTORE_NEEDED=0
    echo "Restoring plain target/release/grass after instrumented profiling build..."
    if ! ~/.cargo/bin/cargo build --release -p grass; then
      echo "ERROR: failed to restore the plain release binary" >&2
      exit 1
    fi
    echo "Restored plain target/release/grass."
  fi
}
trap restore_plain_binary EXIT

usage() {
  echo "Usage: $0 cpu|heap"
  exit 64
}

fixture_error() {
  echo "ERROR: USWDS fixture not found at $LOAD_PATH/uswds" >&2
  exit 2
}

case "$MODE" in
  cpu|heap) ;;
  *) usage ;;
esac

if [ ! -d "$LOAD_PATH/uswds" ]; then
  fixture_error
fi

mkdir -p "$ARTIFACT_DIR"
echo '@use "uswds";' > "$BENCH_FILE"

case "$MODE" in
  cpu)
    if ! command -v samply >/dev/null 2>&1; then
      echo "ERROR: samply not found on PATH."
      echo "Install it with: ~/.cargo/bin/cargo install samply"
      exit 1
    fi

    RESTORE_NEEDED=1
    CARGO_PROFILE_RELEASE_STRIP=none \
    CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
      ~/.cargo/bin/cargo build --release -p grass

    if [ ! -x "$GRASS" ]; then
      echo "ERROR: grass binary not found at $GRASS"
      exit 1
    fi

    CPU_PROFILE="$ARTIFACT_DIR/grass-uswds-cpu.json.gz"
    rm -f "$CPU_PROFILE"
    samply record \
      --output "$CPU_PROFILE" \
      "$GRASS" "$BENCH_FILE" --style=expanded -I "$LOAD_PATH"
    echo "CPU profile: $CPU_PROFILE"
    echo "The samply profile UI was opened by samply record."
    ;;
  heap)
    RESTORE_NEEDED=1
    CARGO_PROFILE_RELEASE_STRIP=none \
      ~/.cargo/bin/cargo build --release --features dhat-heap -p grass

    if [ ! -x "$GRASS" ]; then
      echo "ERROR: grass binary not found at $GRASS"
      exit 1
    fi

    HEAP_DIR="$ARTIFACT_DIR/heap"
    mkdir -p "$HEAP_DIR"
    rm -f "$HEAP_DIR/dhat-heap.json"
    (
      cd "$HEAP_DIR"
      "$GRASS" "$BENCH_FILE" --style=expanded -I "$LOAD_PATH" > /dev/null
    )
    HEAP_PROFILE="$HEAP_DIR/dhat-heap.json"
    if [ ! -f "$HEAP_PROFILE" ]; then
      echo "ERROR: dhat did not produce $HEAP_PROFILE"
      exit 1
    fi
    echo "Heap profile: $HEAP_PROFILE"
    echo "dhat viewer: https://nnethercote.github.io/dh_view/dh_view.html"
    ;;
esac
