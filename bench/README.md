# grass benchmarking

`bench/` contains performance tooling and parity corpora. It is separate from
`ci/`, which owns spec conformance and the USWDS byte-zero gate. Fixture trees
under `bench/fixtures/` are intentionally untracked; the supplied checkout has
USWDS and Bootstrap copies for local runs.

## Measurement rules

- Run on a quiet machine and record a same-session control when comparing
  numbers. Do not compare absolute milliseconds across sessions.
- `perf-check.sh` uses hyperfine with 5 warmups and 15 measured runs when
  available; its fallback is a 3-run smoke median. The standing baseline is
  `bench/.perf-baseline` (`205` ms); do not rewrite it casually.
- `cross-engine.mjs` preserves the old engine semantics: the native, napi,
  WASM, and sass-embedded modes use a fresh worker under hyperfine (1 warmup,
  5 runs), `wasm-string` performs one warmup followed by five in-process timed
  compiles, and `breakdown` uses two warmups plus ten timed reps per engine.
- The real-world runner interleaves each project’s two engines, performs two
  warmups and five measured runs, captures stderr to files, uses quiet machine
  output, disables source maps, and compares raw bytes with no
  canonicalization. Wall-time medians are Grass and Dart Sass separately;
  Dart/Grass is only a ratio, not a parity criterion.

## Commands

Build the release CLI first:

```sh
~/.cargo/bin/cargo build --release
```

Run the acceptance performance check (fixture resolution is
`PERF_FIXTURE_DIR` → `bench/fixtures` → legacy `prototype` fallback):

```sh
bash bench/scripts/perf-check.sh
```

Run the consolidated engines. Use `--fixture bootstrap` or `--fixture uswds`;
`breakdown` also accepts `--diagnose-fs` for its explicitly non-surface shim
diagnostic.

```sh
node bench/scripts/cross-engine.mjs --engine native --fixture uswds
node bench/scripts/cross-engine.mjs --engine sass-embedded --fixture uswds
node bench/scripts/cross-engine.mjs --engine wasm --fixture uswds
node bench/scripts/cross-engine.mjs --engine napi --fixture uswds
node bench/scripts/cross-engine.mjs --engine wasm-string --fixture uswds
node bench/scripts/cross-engine.mjs --engine breakdown --fixture bootstrap
```

`bench/scripts/bench.sh` is a compatibility wrapper for the four ordinary
USWDS engine modes. Run `npm ci` in `bench/` before sass-embedded or package
benchmarks. `profile.sh cpu|heap` retains the CPU and dhat workflows; it writes
profiles under `/tmp/grass-profile` and automatically rebuilds the plain
`target/release/grass` binary before exiting, including after a profiling
failure. The diagnostics are deliberately kept
separate: `memory-plateau-check.mjs`, `multi-compile-stress.mjs`, and
`wasm-spec-runner.mjs` are correctness/memory investigations, not published
speed numbers.

The Bootstrap fixture can be recreated with:

```sh
bash bench/fixtures/fetch-bootstrap.sh
```

## Current local numbers

On the 2026-07-13 session, the release CLI’s USWDS performance gate was
`165 ms` median versus the standing `205 ms` baseline (`-19.5%`). Consolidated
smokes completed for every mode: sass-embedded USWDS `744.1 ms` mean,
native Bootstrap `100.0 ms` mean, WASM USWDS `428.3 ms` mean, napi USWDS
`211.6 ms` mean, wasm-string Bootstrap `65 ms` median, and breakdown Bootstrap
medians of sass-embedded `321 ms`, WASM `62 ms`, napi `39 ms`, and native
`50 ms`. These are session measurements, not a new baseline.

## Real-world parity corpus

The manifest is [manifest.json](real-world/manifest.json), seeded from sasso’s
Apache-2.0/MIT project list:
<https://raw.githubusercontent.com/momiji-rs/sasso/master/bench/real-world/projects.json>.
Every active project is shallow-cloned at a commit re-pinned on 2026-07-13.
The cache is gitignored.

```sh
node bench/real-world/run.mjs all
node bench/real-world/run.mjs --project uswds
```

The runner writes [results.md](real-world/results.md) and ratchets against
[BASELINE.json](real-world/BASELINE.json). It exits non-zero only when an
existing baseline’s PASS regresses or a measured run exceeds the documented
timing review threshold. A missing baseline is created from the run; review it
before committing. Improvements print a ratchet-up reminder.

The finalized active corpus produced the same parity status on two consecutive
full runs. reveal.js is recorded as an explicit drop because its lockfile cannot
be installed with `npm ci`:

| Status | Projects |
|---|---:|
| PASS | 9 |
| DIFF | 1 (`tabler`, selector ordering; discovery todo #350) |
| ERROR | 4 (standalone or existing compiler diagnostic limitations) |

The DIFF is recorded as raw signature `672042/672042@597516`; the first visible
difference is Dart Sass’s grouped `h1`–`h6` selectors followed by `.h1`–`.h6`
versus Grass’s interleaved order. The four ERROR signatures are preserved in
`results.md` and `BASELINE.json` with their actual `Error:` lines; Dart Sass
compiles each of those four entries, so they are discovery candidates rather
than manifest drops. No compiler code was changed. Dropped projects and reasons
are recorded in the manifest; there are no silent corpus caps.

To re-pin, update each project’s commit from its repository’s default HEAD,
verify the detached checkout, rerun `node bench/real-world/run.mjs all` twice,
inspect the raw diff signatures and errors, then update `BASELINE.json` only
when the change is understood. Do not add this corpus to the per-PR pipeline;
it is a local/manual gate.

## Layout and ownership

- `fixtures/`: `_bench_bootstrap.scss`, `_variables.scss` when present, the
  fetcher, and untracked USWDS/Bootstrap trees.
- `scripts/`: acceptance perf, profiling, the compatibility wrapper, and the
  consolidated cross-engine runner.
- `diagnostics/`: memory, repeated-compile, and persistent-WASM spec tools.
- `real-world/`: manifest, runner, committed baseline, and generated results.
- `package.json`: local sass-embedded and package benchmark dependencies.

The corpus attribution is to sasso’s `bench/real-world/projects.json`; its
license metadata and exclusions are preserved in the manifest rather than
silently copied into the working tree.
