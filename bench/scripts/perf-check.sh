#!/usr/bin/env bash
# Compatibility name for older local notes and automation.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ "$#" -gt 0 ] && [[ "$1" != --* ]]; then
  exec "$SCRIPT_DIR/perf.sh" quick --candidate "$1"
fi
exec "$SCRIPT_DIR/perf.sh" quick "$@"
