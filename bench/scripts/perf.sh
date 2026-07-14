#!/usr/bin/env bash
# Executable, same-toolchain, interleaved performance gate for grass.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$REPO_ROOT/bench/fixtures/resolve.sh"

CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
TIME_BIN="/usr/bin/time"
RUNS=10
THRESHOLD=1.0
WORKLOAD_ARG="uswds,bootstrap,extend"
BASE_ARG=""
CANDIDATE_ARG=""
TMP_ROOT=""
BASE_WORKTREE=""
BASE_BINARY=""
CANDIDATE_BINARY=""

die() { echo "ERROR: $*" >&2; exit 1; }

usage() {
  cat >&2 <<'EOF'
Usage:
  bench/scripts/perf.sh compare --base <git-rev|path> [--candidate <path>]
                                [--workload uswds,bootstrap,extend|all]
                                [--threshold 1.0] [--runs N]
  bench/scripts/perf.sh quick [--candidate <path>]
EOF
  exit 64
}

cleanup() {
  if [ -n "$BASE_WORKTREE" ] && [ -d "$BASE_WORKTREE" ]; then
    git -C "$REPO_ROOT" worktree remove --force "$BASE_WORKTREE" >/dev/null 2>&1 || true
  fi
  if [ -n "$TMP_ROOT" ]; then rm -rf "$TMP_ROOT"; fi
}
trap cleanup EXIT

require_command() { command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"; }

binary_version() {
  "$1" --version 2>&1 | head -1 || true
}

rustc_metadata() {
  local toolchain="$1" output
  if [ "$toolchain" = default ]; then
    output="$(rustc -Vv 2>/dev/null || true)"
  else
    output="$(rustc "+$toolchain" -Vv 2>/dev/null || true)"
  fi
  printf '%s\n' "$output"
}

binary_fingerprint() {
  local binary="$1" hashes hash count toolchain metadata version
  hashes="$(strings "$binary" 2>/dev/null | awk -F'/rustc/' 'NF > 1 { split($2, p, "/"); if (length(p[1]) == 40 && p[1] ~ /^[0-9a-f]+$/) print p[1] }' | sort -u)"
  count="$(printf '%s\n' "$hashes" | sed '/^$/d' | wc -l | tr -d ' ')"
  if [ "$count" -ne 1 ]; then
    echo "unknown (could not find exactly one rustc build fingerprint in $binary)" >&2
    return 1
  fi
  hash="$hashes"
  while read -r toolchain; do
    [ -n "$toolchain" ] || continue
    metadata="$(rustc_metadata "$toolchain")"
    if printf '%s\n' "$metadata" | grep -q "^commit-hash: $hash$"; then
      version="$(printf '%s\n' "$metadata" | awk '$1 == "release:" { print $2; exit }')"
      printf '%s|%s|%s\n' "$hash" "$version" "$toolchain"
      return 0
    fi
  done < <(rustup toolchain list 2>/dev/null | awk '{print $1}')
  metadata="$(rustc_metadata default)"
  if printf '%s\n' "$metadata" | grep -q "^commit-hash: $hash$"; then
    version="$(printf '%s\n' "$metadata" | awk '$1 == "release:" { print $2; exit }')"
    printf '%s|%s|default\n' "$hash" "$version"
    return 0
  fi
  echo "unknown (rustc fingerprint $hash is not provided by an installed toolchain)" >&2
  return 1
}

resolve_binary_path() {
  local value="$1"
  if [[ "$value" = /* ]]; then printf '%s\n' "$value"; else printf '%s/%s\n' "$PWD" "$value"; fi
}

build_candidate_if_needed() {
  CANDIDATE_BINARY="${CANDIDATE_ARG:-$REPO_ROOT/target/release/grass}"
  if [[ "$CANDIDATE_BINARY" != /* ]]; then CANDIDATE_BINARY="$(resolve_binary_path "$CANDIDATE_BINARY")"; fi
  if [ ! -x "$CANDIDATE_BINARY" ]; then
    echo "Candidate binary not found; building with the default toolchain: $CARGO build --release -p grass"
    (cd "$REPO_ROOT" && "$CARGO" build --release -p grass)
  fi
  [ -x "$CANDIDATE_BINARY" ] || die "candidate binary not found or not executable: $CANDIDATE_BINARY"
}

prepare_base() {
  local value="$1" resolved
  if [ -f "$value" ]; then
    resolved="$(resolve_binary_path "$value")"
    [ -x "$resolved" ] || die "base path is not executable: $resolved"
    BASE_BINARY="$resolved"
    return
  fi
  if ! git -C "$REPO_ROOT" rev-parse --verify "$value^{commit}" >/dev/null 2>&1; then
    die "--base is neither an executable binary nor a git revision: $value"
  fi
  [ -n "$TMP_ROOT" ] || TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/grass-perf.XXXXXX")"
  BASE_WORKTREE="$TMP_ROOT/base-worktree"
  echo "Building base revision $value with the default toolchain in a temporary worktree"
  git -C "$REPO_ROOT" worktree add --detach "$BASE_WORKTREE" "$value" >/dev/null
  (cd "$BASE_WORKTREE" && "$CARGO" build --release -p grass)
  BASE_BINARY="$BASE_WORKTREE/target/release/grass"
  [ -x "$BASE_BINARY" ] || die "base build did not produce $BASE_BINARY"
}

ensure_extend_fixture() {
  local path="$REPO_ROOT/bench/fixtures/extend-synth.scss"
  if [ ! -f "$path" ]; then node "$REPO_ROOT/bench/fixtures/gen-extend-synth.mjs" "$path" >/dev/null; fi
}

ensure_uswds_entry() {
  [ -n "$TMP_ROOT" ] || TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/grass-perf.XXXXXX")"
  local entry_dir="$TMP_ROOT/uswds-entry"
  mkdir -p "$entry_dir"
  printf '@use "uswds";\n' > "$entry_dir/input.scss"
  ENTRY="$entry_dir/input.scss"
}

workload_paths() {
  local kind="$1" root
  case "$kind" in
    uswds)
      root="$(resolve_fixture_root uswds)"
      ensure_uswds_entry; LOAD_PATH="$root/packages" ;;
    bootstrap)
      root="$(resolve_fixture_root bootstrap)"
      if [ -f "$root/scss/bootstrap.scss" ]; then ENTRY="$root/scss/bootstrap.scss"; LOAD_PATH="$root/scss"; else ENTRY="$root/bootstrap-bench/scss/bootstrap.scss"; LOAD_PATH="$root/bootstrap-bench/scss"; fi ;;
    extend)
      ensure_extend_fixture
      ENTRY="$REPO_ROOT/bench/fixtures/extend-synth.scss"; LOAD_PATH="$REPO_ROOT/bench/fixtures" ;;
    *) die "unknown workload: $kind" ;;
  esac
  [ -f "$ENTRY" ] || die "$kind entry is missing: $ENTRY"
}

measure_one() {
  local binary="$1" entry="$2" load_path="$3" stderr_file="$4" stdout_file="$5"
  local seconds instructions
  if ! "$TIME_BIN" -l "$binary" "$entry" --style=expanded --no-source-map -I "$load_path" >"$stdout_file" 2>"$stderr_file"; then
    echo "ERROR: compiler failed; stderr captured at $stderr_file" >&2
    sed -n '1,12p' "$stderr_file" >&2 || true
    return 1
  fi
  instructions="$(awk '/instructions retired/{print $1; exit}' "$stderr_file")"
  seconds="$(awk '/ real/{print $1; exit}' "$stderr_file")"
  [ -n "$instructions" ] || { echo "ERROR: /usr/bin/time did not report instructions retired ($stderr_file)" >&2; return 1; }
  [ -n "$seconds" ] || { echo "ERROR: /usr/bin/time did not report wall time ($stderr_file)" >&2; return 1; }
  MEASURE_INSTR="$instructions"
  MEASURE_WALL_MS="$(python3 - "$seconds" <<'PY'
import sys
print(f"{float(sys.argv[1]) * 1000:.3f}")
PY
)"
}

shell_command() { printf '%q ' "$1" "$2" --style=expanded --no-source-map -I "$3"; }

run_workload() {
  local kind="$1" run_dir="$TMP_ROOT/$kind" data="$TMP_ROOT/$kind.tsv" pair order base_instr cand_instr base_ms cand_ms
  local base_cmd cand_cmd wall_json wall_base wall_cand summary_status
  mkdir -p "$run_dir"; : > "$data"; workload_paths "$kind"
  echo ""; echo "=== $kind ($ENTRY) ==="
  echo "Measurement rule: discard pair 1 as the cold pair; summarize pairs 2..$RUNS."
  for ((pair=1; pair<=RUNS; pair++)); do
    if (( pair % 2 == 1 )); then order="base,candidate"; else order="candidate,base"; fi
    if [ "$order" = "base,candidate" ]; then
      measure_one "$BASE_BINARY" "$ENTRY" "$LOAD_PATH" "$run_dir/pair-$pair-base.stderr" "$run_dir/pair-$pair-base.stdout"
      base_instr="$MEASURE_INSTR"; base_ms="$MEASURE_WALL_MS"
      measure_one "$CANDIDATE_BINARY" "$ENTRY" "$LOAD_PATH" "$run_dir/pair-$pair-candidate.stderr" "$run_dir/pair-$pair-candidate.stdout"
      cand_instr="$MEASURE_INSTR"; cand_ms="$MEASURE_WALL_MS"
    else
      measure_one "$CANDIDATE_BINARY" "$ENTRY" "$LOAD_PATH" "$run_dir/pair-$pair-candidate.stderr" "$run_dir/pair-$pair-candidate.stdout"
      cand_instr="$MEASURE_INSTR"; cand_ms="$MEASURE_WALL_MS"
      measure_one "$BASE_BINARY" "$ENTRY" "$LOAD_PATH" "$run_dir/pair-$pair-base.stderr" "$run_dir/pair-$pair-base.stdout"
      base_instr="$MEASURE_INSTR"; base_ms="$MEASURE_WALL_MS"
    fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$pair" "$base_instr" "$cand_instr" "$base_ms" "$cand_ms" >> "$data"
    echo "RAW: workload=$kind pair=$pair order=$order base_instr=$base_instr candidate_instr=$cand_instr base_wall_ms=$base_ms candidate_wall_ms=$cand_ms stderr_base=$run_dir/pair-$pair-base.stderr stderr_candidate=$run_dir/pair-$pair-candidate.stderr"
  done
  if python3 - "$data" "$THRESHOLD" "$kind" <<'PY'
import statistics, sys
path, threshold, kind = sys.argv[1], float(sys.argv[2]), sys.argv[3]
rows = []
with open(path) as f:
    for line in f:
        pair, base, cand, *_ = line.split()
        if int(pair) > 1:
            rows.append((int(base), int(cand)))
base = statistics.median(x[0] for x in rows)
cand = statistics.median(x[1] for x in rows)
delta = (cand - base) / base * 100
ns = lambda x: f"{x / 1_000_000:.1f}M"
verdict = "PASS" if delta <= threshold else "FAIL"
sign = "+" if delta > 0 else ""
print(f"SUMMARY: {kind} kept_pairs={len(rows)} base_instr={base:.1f} ({ns(base)}) candidate_instr={cand:.1f} ({ns(cand)}) delta={sign}{delta:.2f}%")
print(f"PERF: {kind} instr {sign}{delta:.2f}% (base {ns(base)}, cand {ns(cand)}) — {verdict} (<{threshold:.1f}%)")
sys.exit(0 if verdict == "PASS" else 10)
PY
  then summary_status=0; else summary_status=$?; fi

  wall_json="$run_dir/hyperfine.json"; wall_base="$run_dir/hyperfine-base.stderr"; wall_cand="$run_dir/hyperfine-candidate.stderr"
  base_cmd="$(shell_command "$BASE_BINARY" "$ENTRY" "$LOAD_PATH") > /dev/null 2>$(printf '%q' "$wall_base")"
  cand_cmd="$(shell_command "$CANDIDATE_BINARY" "$ENTRY" "$LOAD_PATH") > /dev/null 2>$(printf '%q' "$wall_cand")"
  echo "WALL: hyperfine --warmup 3 --runs $RUNS (one invocation; stderr files: $wall_base, $wall_cand)"
  hyperfine --warmup 3 --runs "$RUNS" --export-json "$wall_json" --command-name base "$base_cmd" --command-name candidate "$cand_cmd"
  python3 - "$wall_json" <<'PY'
import json, sys
for result in json.load(open(sys.argv[1]))["results"]:
    print(f"WALL: {result['command']} median_ms={result['median'] * 1000:.3f} runs={len(result['times'])}")
PY
  return "$summary_status"
}

load_check() {
  local line load cpus
  line="$(uptime)"; echo "LOAD: $line"
  load="$(printf '%s\n' "$line" | sed -nE 's/.*load averages?: ([0-9.]+).*/\1/p')"
  cpus="$(sysctl -n hw.ncpu 2>/dev/null || echo 1)"
  if [ -n "$load" ] && awk "BEGIN { exit !($load > $cpus) }"; then
    echo "WARNING: one-minute load $load exceeds $cpus logical CPUs; measurement may be noisy" >&2
  fi
}

quick() {
  if [ -n "$CANDIDATE_ARG" ]; then build_candidate_if_needed; else build_candidate_if_needed; fi
  workload_paths uswds; load_check
  local command json
  command="$(shell_command "$CANDIDATE_BINARY" "$ENTRY" "$LOAD_PATH") > /dev/null 2>$(printf '%q' "$TMP_ROOT/quick.stderr")"
  json="$(mktemp -t grass-quick)"
  hyperfine --warmup 3 --runs 10 --export-json "$json" "$command"
  python3 - "$json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))["results"][0]
print(f"PERF: quick uswds wall median {r['median'] * 1000:.3f}ms (warmup=3 runs={len(r['times'])}; no verdict)")
PY
  rm -f "$json"
}

compare() {
  [ -n "$BASE_ARG" ] || usage
  require_command strings; require_command hyperfine
  [ -x "$TIME_BIN" ] || die "$TIME_BIN is required for instruction counts"
  [ "$RUNS" -ge 10 ] || die "--runs must be at least 10"
  TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/grass-perf.XXXXXX")"
  build_candidate_if_needed; prepare_base "$BASE_ARG"
  local base_meta cand_meta base_hash cand_hash base_version cand_version status=0 kind
  if ! base_meta="$(binary_fingerprint "$BASE_BINARY")"; then
    echo "REFUSING COMPARISON: cannot determine the base binary's rustc toolchain ($BASE_BINARY)" >&2; exit 2
  fi
  if ! cand_meta="$(binary_fingerprint "$CANDIDATE_BINARY")"; then
    echo "REFUSING COMPARISON: cannot determine the candidate binary's rustc toolchain ($CANDIDATE_BINARY)" >&2; exit 2
  fi
  base_hash="${base_meta%%|*}"; cand_hash="${cand_meta%%|*}"
  base_version="${base_meta#*|}"; base_version="${base_version%%|*}"
  cand_version="${cand_meta#*|}"; cand_version="${cand_version%%|*}"
  echo "BASE: $(binary_version "$BASE_BINARY") [rustc $base_version, fingerprint $base_hash]"
  echo "CANDIDATE: $(binary_version "$CANDIDATE_BINARY") [rustc $cand_version, fingerprint $cand_hash]"
  if [ "$base_hash" != "$cand_hash" ]; then
    echo ""; echo "REFUSING CROSS-TOOLCHAIN COMPARISON: rustc fingerprints differ." >&2
    echo "  base:      rustc $base_version ($base_hash)" >&2
    echo "  candidate: rustc $cand_version ($cand_hash)" >&2
    echo "A cross-toolchain result is not a performance measurement." >&2; exit 2
  fi
  load_check; ensure_extend_fixture
  local kinds=()
  if [ "$WORKLOAD_ARG" = all ]; then kinds=(uswds bootstrap extend); else IFS=',' read -r -a kinds <<< "$WORKLOAD_ARG"; fi
  for kind in "${kinds[@]}"; do
    case "$kind" in uswds|bootstrap|extend) ;; *) die "unknown workload: $kind" ;; esac
    if run_workload "$kind"; then :; else status=1; fi
  done
  return "$status"
}

[ "$#" -gt 0 ] || usage
MODE="$1"; shift
while [ "$#" -gt 0 ]; do
  case "$1" in
    --base) [ "$#" -ge 2 ] || usage; BASE_ARG="$2"; shift 2 ;;
    --candidate) [ "$#" -ge 2 ] || usage; CANDIDATE_ARG="$2"; shift 2 ;;
    --workload) [ "$#" -ge 2 ] || usage; WORKLOAD_ARG="$2"; shift 2 ;;
    --threshold) [ "$#" -ge 2 ] || usage; THRESHOLD="$2"; shift 2 ;;
    --runs) [ "$#" -ge 2 ] || usage; RUNS="$2"; shift 2 ;;
    *) usage ;;
  esac
done
case "$MODE" in compare) compare ;; quick) quick ;; *) usage ;; esac
