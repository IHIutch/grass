#!/bin/bash
# Compatibility name for the old Bootstrap-only fetch workflow. The pinned
# source now lives under fetched/bootstrap alongside the USWDS fixture.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/fetch.sh" bootstrap "$@"
