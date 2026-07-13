#!/usr/bin/env bash
# Compare the local Grass binary with Dart Sass for the untracked USWDS fixture.
# Fresh CI checkouts don't contain prototype/packages, so absence is an
# intentional skip rather than a failed build.

set -euo pipefail

fixture_dir="${USWDS_FIXTURE_DIR:-prototype}"
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
