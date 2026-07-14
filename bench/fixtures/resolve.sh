#!/usr/bin/env bash
# Print the root of a benchmark fixture after applying the documented order:
# PERF_FIXTURE_DIR, the real-world corpus cache, fetched pinned trees, then
# the legacy hand-managed tree.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

resolve_fixture_root() {
  local kind="${1:?fixture kind is required}" root
  if [ -n "${PERF_FIXTURE_DIR:-}" ]; then
    root="$PERF_FIXTURE_DIR"
    if [ "$kind" = uswds ] && [ -d "$root/packages/uswds" ]; then
      printf '%s\n' "$root"
      return 0
    fi
    if [ "$kind" = bootstrap ] && { [ -f "$root/bootstrap-bench/scss/bootstrap.scss" ] || [ -f "$root/scss/bootstrap.scss" ]; }; then
      printf '%s\n' "$root"
      return 0
    fi
    echo "ERROR: PERF_FIXTURE_DIR=$root has no $kind fixture" >&2
    return 2
  fi

  if [ "$kind" = uswds ] && [ -d "$REPO_ROOT/bench/real-world/.cache/uswds/packages/uswds" ]; then
    printf '%s\n' "$REPO_ROOT/bench/real-world/.cache/uswds"
    return 0
  fi
  if [ "$kind" = bootstrap ] && [ -f "$REPO_ROOT/bench/real-world/.cache/bootstrap/scss/bootstrap.scss" ]; then
    printf '%s\n' "$REPO_ROOT/bench/real-world/.cache/bootstrap"
    return 0
  fi

  if [ "$kind" = uswds ] && [ -d "$REPO_ROOT/bench/fixtures/fetched/uswds/packages/uswds" ]; then
    printf '%s\n' "$REPO_ROOT/bench/fixtures/fetched/uswds"
    return 0
  fi
  if [ "$kind" = bootstrap ] && [ -f "$REPO_ROOT/bench/fixtures/fetched/bootstrap/scss/bootstrap.scss" ]; then
    printf '%s\n' "$REPO_ROOT/bench/fixtures/fetched/bootstrap"
    return 0
  fi

  if [ "$kind" = uswds ] && [ -d "$REPO_ROOT/bench/fixtures/packages/uswds" ]; then
    printf '%s\n' "$REPO_ROOT/bench/fixtures"
    return 0
  fi
  if [ "$kind" = bootstrap ] && [ -f "$REPO_ROOT/bench/fixtures/bootstrap-bench/scss/bootstrap.scss" ]; then
    printf '%s\n' "$REPO_ROOT/bench/fixtures"
    return 0
  fi

  echo "ERROR: $kind fixture is unavailable" >&2
  echo "Run: bash bench/fixtures/fetch.sh $kind" >&2
  return 2
}
