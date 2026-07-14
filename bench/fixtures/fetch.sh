#!/usr/bin/env bash
# Fetch the pinned source trees used by the executable performance gate and
# the default Bootstrap-only PGO training set. Individual project names and
# `all` remain available for rerunning the training-set experiment.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FETCH_ROOT="$SCRIPT_DIR/fetched"
USWDS_URL="https://github.com/uswds/uswds.git"
USWDS_PIN="636338773b3c296e9b9454f3840e5a7791bcf56d"
BOOTSTRAP_URL="https://github.com/twbs/bootstrap.git"
BOOTSTRAP_PIN="b37afd77f69b97ae67722a9c3edb5ec5339544f3"
TABLER_URL="https://github.com/tabler/tabler.git"
TABLER_PIN="62178f8ef10977a38f5ed8b951874ce5a6eb9e7a"
FONT_AWESOME_URL="https://github.com/FortAwesome/Font-Awesome.git"
FONT_AWESOME_PIN="70fb2dd154b617f62fc4ae5b0b7e2943bfd2aa96"

usage() {
  echo "Usage: $0 [uswds|bootstrap|tabler|font-awesome|pgo|all]" >&2
  exit 64
}

fetch_repo() {
  local name="$1" url="$2" pin="$3" destination="$FETCH_ROOT/$1"
  mkdir -p "$FETCH_ROOT"

  if [ -d "$destination/.git" ]; then
    local current
    current="$(git -C "$destination" rev-parse HEAD 2>/dev/null || true)"
    if [ "$current" != "$pin" ]; then
      git -C "$destination" fetch --depth=1 --no-tags origin "$pin"
      git -C "$destination" checkout --detach "$pin"
    fi
  else
    if [ -e "$destination" ]; then
      echo "ERROR: fixture destination exists but is not a git checkout: $destination" >&2
      exit 1
    fi
    git init -q "$destination"
    git -C "$destination" remote add origin "$url"
    git -C "$destination" fetch -q --depth=1 --no-tags origin "$pin"
    git -C "$destination" checkout -q --detach "$pin"
  fi

  if [ "$(git -C "$destination" rev-parse HEAD)" != "$pin" ]; then
    echo "ERROR: $name fixture is not pinned to $pin" >&2
    exit 1
  fi
  echo "$name fixture ready at $destination ($pin)"
}

case "${1:-all}" in
  uswds)
    fetch_repo uswds "$USWDS_URL" "$USWDS_PIN"
    ;;
  bootstrap)
    fetch_repo bootstrap "$BOOTSTRAP_URL" "$BOOTSTRAP_PIN"
    ;;
  tabler)
    fetch_repo tabler "$TABLER_URL" "$TABLER_PIN"
    ;;
  font-awesome)
    fetch_repo font-awesome "$FONT_AWESOME_URL" "$FONT_AWESOME_PIN"
    ;;
  pgo)
    fetch_repo bootstrap "$BOOTSTRAP_URL" "$BOOTSTRAP_PIN"
    ;;
  all)
    fetch_repo uswds "$USWDS_URL" "$USWDS_PIN"
    fetch_repo bootstrap "$BOOTSTRAP_URL" "$BOOTSTRAP_PIN"
    fetch_repo tabler "$TABLER_URL" "$TABLER_PIN"
    fetch_repo font-awesome "$FONT_AWESOME_URL" "$FONT_AWESOME_PIN"
    ;;
  *) usage ;;
esac
