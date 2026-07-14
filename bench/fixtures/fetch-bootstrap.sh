#!/bin/bash
# Fetch the Bootstrap v5.0.2 workload used for perf A/B sessions (see
# docs/design/performance-roadmap.md). It's a legacy @import-heavy, @each
# loop-heavy stylesheet — structurally very different from the USWDS fixture
# used by perf-check.sh — and is deliberately not vendored into git. This
# script replaces the ad-hoc "clone into /private/tmp" step with a permanent,
# reproducible home under bench/fixtures/ (already covered by the `bootstrap*`
# .gitignore pattern).
#
# Usage: ./fetch-bootstrap.sh [target-dir]
# Entry point after fetching: <target-dir>/scss/bootstrap.scss

set -e

TARGET="${1:-$(cd "$(dirname "$0")" && pwd)/bootstrap-bench}"

if [ -d "$TARGET" ]; then
  echo "Bootstrap already present at $TARGET"
  exit 0
fi

git clone --depth=1 --branch v5.0.2 https://github.com/twbs/bootstrap.git "$TARGET"

echo ""
echo "Bootstrap v5.0.2 fetched to $TARGET"
echo "Entry point: $TARGET/scss/bootstrap.scss"
