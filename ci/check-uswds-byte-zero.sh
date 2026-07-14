#!/usr/bin/env bash
# Compare the local Grass binary with Dart Sass for the resolved USWDS fixture.
# A CI checkout that has not fetched it is an intentional skip.

set -euo pipefail

source "$(dirname "$0")/../bench/fixtures/resolve.sh"
if [[ -n "${USWDS_FIXTURE_DIR:-}" && -z "${PERF_FIXTURE_DIR:-}" ]]; then
  PERF_FIXTURE_DIR="$USWDS_FIXTURE_DIR"
fi
if ! fixture_dir="$(resolve_fixture_root uswds 2>/dev/null)"; then
  echo "SKIP: USWDS fixture not fetched or present"
  exit 0
fi
binary="${GRASS_BINARY:-target/debug/grass}"
load_path="$fixture_dir/packages"

if [[ ! -d "$load_path/uswds" ]]; then
  echo "SKIP: USWDS fixture not found at $load_path/uswds"
  exit 0
fi

if [[ ! -x "$binary" ]]; then
  echo "ERROR: Grass binary not found at $binary" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
printf '@use "uswds";\n' > "$tmp_dir/input.scss"

"$binary" "$tmp_dir/input.scss" --style=expanded --no-source-map -I "$load_path" \
  > "$tmp_dir/grass.css"
npx -y sass@1.101.0 --style=expanded --no-source-map -I "$load_path" \
  "$tmp_dir/input.scss" "$tmp_dir/dart.css"

if diff -u "$tmp_dir/dart.css" "$tmp_dir/grass.css"; then
  echo "USWDS byte-zero: PASS"
else
  echo "USWDS byte-zero: FAIL" >&2
  exit 1
fi
