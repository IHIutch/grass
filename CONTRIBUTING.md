# Contributing to grass

grass is a Sass compiler written in Rust, targeting feature parity with
[dart-sass](https://github.com/sass/dart-sass). This guide covers the
day-to-day workflow for human contributors.

## Prerequisites

- Rust, MSRV 1.88 (see `rust-version` in `crates/*/Cargo.toml`)
- Cargo on `PATH` (if not, use `~/.cargo/bin/cargo` explicitly)
- Python 3 (for the sass-spec test runner)
- Node.js + `npx` (only if you need to check expected output against dart-sass)

## Project Structure

- `crates/compiler/` — core compiler (`grass_compiler` crate)
- `crates/lib/` — public library + CLI binary (`grass` crate)
- `crates/lib/pkg-publish/` — npm package (WASM + napi-rs fallback)
- `crates/napi/` — napi-rs native Node.js addon (`grass_napi` crate)
- `crates/include_sass/` — proc macro crate
- `crates/lib/tests/` — integration tests, organized by feature
- `bench/` — benchmark scripts, fixtures, diagnostics, and perf baseline
- `sass-spec/` — git submodule of the official Sass spec test suite

## Build, Test, Lint

```bash
cargo build --release
cargo test --features=macro
cargo clippy --features=macro -- -D warnings
```

Iterate against sass-spec first when working on spec compliance — it's much
faster than the full `cargo test` suite. Run `cargo test --features=macro`
as the final gate before committing, to catch regressions across the whole
suite.

## Running the sass-spec Suite

The submodule needs to be initialized once:

```bash
git submodule update --init sass-spec
```

Then run the local test runner (untracked, lives at the repo root):

```bash
python3 run-sass-specs.py
```

This builds against `sass-spec/spec` by default; pass `--spec-dir` to point
at a different checkout. Before starting work on a feature or bug, search
`sass-spec/spec/` for related tests — grass aims to pass anything dart-sass
does, so this surfaces expected behavior and edge cases up front.

For the checked-in failure baseline, build the release binary and run:

```bash
cargo build --release
python3 ci/check-sass-spec.py
```

For a quick smoke test of the release binary:

```bash
echo "a { b: c }" | ./target/release/grass --stdin --style=expanded
```

## dart-sass Parity Conventions

- **Never change a test's expected output based on reasoning alone.** Verify
  against dart-sass first:
  ```bash
  echo 'a { color: rgb(1.5, 1.5, 1.5); }' | npx sass@1.101.0 --stdin --style=expanded
  ```
  Use that exact output as the expected value, and note in the commit
  message that expectations were verified against dart-sass (which version).
- Error message text and span positions are allowed to differ from
  dart-sass — only the resulting CSS (and warnings/errors' presence) needs
  to match.
- If you find a sass-spec test whose expected output doesn't match
  dart-sass 1.101.0, don't spend time chasing it — it's likely a stale
  fixture. File it as an issue (see below) instead.

## Performance Gate

For a compiler performance comparison, build the candidate with the default
toolchain and compare it with the merge base. The gate fetches pinned USWDS
and Bootstrap fixtures when needed:

```bash
bash bench/fixtures/fetch.sh all
~/.cargo/bin/cargo build --release -p grass
bash bench/scripts/perf.sh compare --base "$(git merge-base HEAD origin/HEAD)" --workload all
```

The gate rejects prebuilt binaries whose rustc fingerprints differ or cannot
be determined, alternates base/candidate order in one run set, reports raw
instruction counts from `/usr/bin/time -l`, and uses hyperfine wall medians as
the secondary signal. It discards the first pair as a documented cold-start
rule. There is no absolute performance baseline to rewrite.

For a single-binary smoke measurement, use `bash bench/scripts/perf.sh quick`.
For a full cross-engine benchmark (native vs. WASM vs. sass-embedded), see
`bench/scripts/bench.sh`.

The no-callback N-API `compileAsync` path is threadpool-bound. On the Bootstrap
fixture, the distinct-input N=8 speedup was 3.85x with the default
`UV_THREADPOOL_SIZE=4` and 6.85x with `UV_THREADPOOL_SIZE=8`. Node consumers
issuing many independent compiles can set `UV_THREADPOOL_SIZE` before the first
async work, subject to the machine's available CPU.

Callback-bearing compiles are bound by callback density (callbacks per ms of
compile work): each callback is a blocking round-trip serialized through the
single JS thread. A realistic file importer made 87 callbacks over a roughly
46ms Bootstrap compile (roughly 1.9 callbacks/ms), left the threadpool as the
constraint, and measured 6.43x speedup at N=8 with
`UV_THREADPOOL_SIZE=8`. A synthetic workload with 2,000 custom-function calls
over a roughly 17ms compile (roughly 118 callbacks/ms) capped near 1.5x, and
raising `UV_THREADPOOL_SIZE` barely helped.

This is not explained by callback CPU: callback bodies measured 0.3–0.5% of
wall time, and event-loop lag measured the JS thread at 8.5% busy during the
capped workload. The precise mechanism, including N-API ThreadsafeFunction
dispatch versus cross-thread wakeup latency, is not established.

## Profiling

Use profiling to rank performance candidates before changing code; use
`bench/scripts/perf.sh compare` for acceptance, and always run the performance gate
for a change that affects the compiler. The profiling harness uses the same
USWDS fixture and compile invocation as the performance gate:

```bash
PERF_FIXTURE_DIR=/path/to/checkout/with/packages ./bench/scripts/profile.sh cpu
PERF_FIXTURE_DIR=/path/to/checkout/with/packages ./bench/scripts/profile.sh heap
```

The `cpu` mode records a samply profile and opens its local profile UI. Install
samply once with:

```bash
~/.cargo/bin/cargo install samply
```

The `heap` mode records dhat allocation call sites and can be loaded at
<https://nnethercote.github.io/dh_view/dh_view.html>. Dhat runs are roughly
10–40× slower than a normal compile, so use only relative allocation counts
when comparing runs. The existing `fuzz/src/bin/leak_probe.rs` reports
teardown allocation deltas; dhat complements it with call-site attribution.

Follow the measurement rules from the performance gate: compare a same-moment
control, use both workloads, and never compare absolute milliseconds across
separate sessions. Profiling ranks where to investigate; `perf.sh compare`
remains the authoritative acceptance check.

### PGO reference numbers (2026-07-11)

The shipped CLI is PGO-built (`build-pgo.sh`, `v*` tags), while all tracked
perf numbers come from plain `--release` builds. Measured at `428608e2`
(quiet machine, interleaved hyperfine, USWDS trained):

- Wall time: PGO 1.15× faster on USWDS (196 → 171 ms mean), 1.10× on
  Bootstrap (60.3 → 54.8 ms). Output is byte-identical on both workloads.
- Instructions retired (median of 3 interleaved `/usr/bin/time -l` runs):
  USWDS 1,904M → 1,676M (−12.0%), Bootstrap 597M → 518M (−13.1%).
- The CPU profile ranking materially changes under PGO: `eval_args`,
  `Scopes::get_var`, and the lexer disappear from the top symbols (inlined
  into callers), and `visit_expr_ref` / `run_function_callable_with_maybe_evaled`
  roughly double their self-sample share. Allocator (`mi_free` +
  `mi_malloc_aligned`, ~11% combined) and fs syscalls (`__getattrlist`,
  `__open`, `write`) take a relatively larger share of what remains.

Rule of thumb: deep-optimization decisions (what to target next) should
consult PGO profiles — that's the binary users run, and PGO already
restructures leaf-level inlining. Acceptance gates stay on plain release
builds for comparability with history. To profile a
PGO build, run `build-pgo.sh` with `CARGO_PROFILE_RELEASE_STRIP=none` (the
release profile strips symbols otherwise) and point `samply record` at the
resulting binary directly — `profile.sh cpu` rebuilds a plain binary and
would overwrite it.

## Package and N-API Checks

When changing the native Node.js addon or npm package, run the relevant local
checks:

```bash
(cd crates/napi && npm run build-debug && node test.mjs)
(cd crates/lib/pkg-publish && npm test)
```

## Profile-Guided Optimization / Releases

`build-pgo.sh` (repo root) produces a PGO-optimized release binary, trained
on a representative workload. CI builds PGO binaries automatically for
`v*` tags (see `.github/workflows/release_cli.yml`) — you don't need to run
this locally unless you're benchmarking a PGO build yourself.

## Code Conventions

- Tests use a `test!` macro comparing Sass input to expected CSS output
  (see any file under `crates/lib/tests/` for examples).
- Mark known-failing tests with `#[ignore = "reason"]` rather than deleting
  or commenting them out — the reason string should explain why it fails
  and, where relevant, what would need to change to fix it.
- Don't add abstractions, error handling, or validation beyond what a
  change actually needs.

## Edition / MSRV Policy

All crates are pinned to `edition = "2021"` and `rust-version = "1.88"`.
This is a deliberate floor, not an oversight — treat any bump to edition
2024 or a newer MSRV as a decision to make explicitly, not a drive-by
change.

## Filing Issues

Bugs and feature requests are tracked on GitHub — open an issue against
this repository. Include a minimal Sass input, the output you got, and
(if relevant) the output dart-sass produces for the same input.
