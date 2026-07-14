# grass benchmarking

`bench/` contains performance tooling and parity corpora. It is separate from
`ci/`, which owns spec conformance and the USWDS byte-zero gate. Source trees
are fetched at pinned commits; the extend synthetic is tracked.

## Measurement rules

- Run on a quiet machine and record a same-session control when comparing
  numbers. Do not compare absolute milliseconds across sessions.
- `perf.sh compare` builds git-revision bases with the default toolchain,
  refuses unknown or mismatched rustc fingerprints, alternates base/candidate
  pairs in both orders, and reports raw `/usr/bin/time -l` instruction counts.
  It drops pair 1 as the deterministic cold-start rule and uses hyperfine
  (`--warmup 3 --runs 10`) for secondary wall medians. It has no absolute
  baseline.
- `cross-engine.mjs` preserves the old engine semantics: the native, napi,
  WASM, and sass-embedded modes use a fresh worker under hyperfine (1 warmup,
  5 runs), `wasm-string` performs one warmup followed by five in-process timed
  compiles, and `breakdown` uses two warmups plus ten timed reps per engine.
- The real-world runner interleaves each project’s two engines, performs two
  warmups and five measured runs, captures stderr to files, uses quiet machine
  output, disables source maps, and compares raw bytes with no
  canonicalization. Wall-time medians are Grass and Dart Sass separately;
  Dart/Grass is only a ratio, not a parity criterion.

| Question | Tool |
|---|---|
| Did this compiler change regress against its base? | `perf.sh compare` |
| Does one binary compile the smoke workload? | `perf.sh quick` |
| Where do native/WASM/N-API timings differ? | `cross-engine.mjs` |
| Do real projects still compile and match bytes? | `real-world/run.mjs` |
| Where is compiler time or memory spent? | `profile.sh` and `diagnostics/` |

## Commands

Build the release CLI first:

```sh
~/.cargo/bin/cargo build --release
```

Fetch the pinned sources, then run the base-vs-candidate gate:

```sh
bash bench/fixtures/fetch.sh all
bash bench/scripts/perf.sh compare --base "$(git merge-base HEAD origin/HEAD)" --workload all
```

For a one-binary smoke measurement with no verdict:

```sh
bash bench/scripts/perf.sh quick
```

Fixture resolution is `PERF_FIXTURE_DIR` → the matching pinned tree in the
real-world corpus cache → fetched pinned tree → legacy tree. The USWDS
workload always generates a temporary `input.scss` containing `@use "uswds";`
and compiles it through the upstream package entry; no custom benchmark entry
is committed.

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

The pinned sources can be recreated with `bash bench/fixtures/fetch.sh all`.

## Fixtures

The performance fixtures intentionally cover different Sass workload shapes:

- **USWDS** is the upstream `@use "uswds";` entry, `@use`-module-heavy, with a deep graph of roughly 90 partials;
  it stresses import-graph and filesystem resolution.
- **Bootstrap v5.0.2** is legacy `@import`-heavy and `@each`-heavy, generating
  CSS through evaluator, value, and serialization paths.

Using both prevents a performance change from being tuned to only one import
graph shape. `bench/fixtures/fetch.sh all` recreates the pinned source trees.

## PGO training and held-out validation

`build-pgo.sh` defaults to a four-project profile from the pinned corpus:
USWDS (`@use`/`@forward` module depth), Bootstrap (legacy `@import` and
`@each` control flow), Tabler (`@extend` and selector machinery), and Font
Awesome (value/string interpolation and serialization-heavy icon content).
The profile collection runs each project three times and merges all generated
profiles. Set `PGO_TRAINING_SET=uswds` or another comma-separated subset to
reproduce a single-project regime; `PGO_WORKLOAD` and `PGO_WORKLOAD_FLAGS`
remain single-entry escape hatches.

For a held-out corpus project, use the same interleaved gate with its manifest
entry and load path. This preserves the gate's same-toolchain check, paired
ordering, instruction-primary result, and cold-pair discard:

```sh
bash bench/scripts/perf.sh compare \
  --base /path/to/plain-release/grass \
  --candidate /path/to/pgo/grass \
  --entry bench/real-world/.cache/vuetify/packages/vuetify/src/styles/main.sass \
  --load-path bench/real-world/.cache/vuetify/packages/vuetify/src/styles \
  --label vuetify
```

The `--entry`/`--load-path` form is intentionally an escape hatch for corpus
entries that are not one of the three built-in synthetic workloads. Record the
raw `perf.sh` `RAW:`, `SUMMARY:`, `PERF:`, and `WALL:` lines for each regime;
do not substitute standalone shell timing.

The 2026-07-14 measurement used the pinned plain-release reference binary and
the same default rustc fingerprint on a loaded machine. Values below are the
raw `perf.sh` median summaries after pair 1 was discarded; wall values are the
secondary hyperfine medians.

| Project | In training set? | Plain instructions / wall | PGO instructions / wall | Delta |
|---|---|---:|---:|---:|
| Mastodon | No | 188.7M / 19.634 ms | 160.5M / 18.214 ms | −14.97% |
| Vuetify | No | 365.5M / 33.352 ms | 310.0M / 29.919 ms | −15.19% |
| Grafana | No | 86.1M / 10.758 ms | 76.4M / 9.513 ms | −11.35% |

The trained entries measured as follows:

| Project | In training set? | Plain instructions / wall | PGO instructions / wall | Delta |
|---|---|---:|---:|---:|
| USWDS | Yes | 1910.6M / 169.982 ms | 1663.3M / 150.444 ms | −12.95% |
| Bootstrap | Yes | 628.0M / 51.942 ms | 539.7M / 45.563 ms | −14.06% |
| Tabler | Yes | 1165.4M / 109.341 ms | 1044.8M / 100.542 ms | −10.35% |
| Font Awesome | Yes | 117.9M / 11.809 ms | 100.9M / 10.803 ms | −14.38% |

For the old-regime comparison on the same held-out entries, USWDS-only was
Mastodon −14.54% (188.6M→161.2M), Vuetify −15.36% (365.2M→309.0M), and
Grafana −10.77% (86.0M→76.7M). Bootstrap-only was Mastodon −15.90%
(188.6M→158.6M), Vuetify −14.42% (365.3M→312.6M), and Grafana −11.58%
(86.0M→76.1M). Their wall medians were lower than plain in every case.

The PGO binary passed the USWDS byte-zero gate. The corpus runner produced
13 PASS projects; `govuk-frontend` was ERROR because its project-local `npm
ci` failed, so that project is unverified rather than counted as a byte-pass.
No corpus byte-diff was reported. The CI workflow was not executed locally;
its added fetch/build wall time is therefore unverified. The local default
multi-project build took 175.43 s wall, including both release compilations.

The held-out gains are within the trained-entry range, and all three held-out
projects were faster on instructions and wall time in this run. This supports
generalization for this corpus and machine, but is not evidence for every Sass
project.

CI fetches the same four pinned projects with `bash bench/fixtures/fetch.sh
pgo` before invoking the default `build-pgo.sh` path, so CI and local release
profiles use the same training set.

## Reference values

These are plain-release, same-session measurements on the pinned fixtures.
They are documentation only; future gates compare base and candidate binaries
in the same run.

| Workload | Base instructions | Candidate instructions | Base wall median | Candidate wall median |
|---|---:|---:|---:|---:|
| USWDS upstream | 1,911.7M | 1,911.0M (-0.04%) | 183.205 ms | 182.342 ms |
| Bootstrap | 627.9M | 628.3M (+0.07%) | 56.525 ms | 56.510 ms |
| Extend synthetic | 33.3M | 33.0M (-0.85%) | 5.766 ms | 5.733 ms |

Measured with 10 interleaved pairs, pair 1 discarded, three hyperfine warmups,
and 10 hyperfine wall runs on 2026-07-13. The machine load was elevated but
below the logical-CPU warning threshold; rerun on a quiet machine before using
these as an adjudication.

The previous USWDS reference used the retired component-flat entry: 1,825.9M
base instructions / 1,826.0M candidate instructions and 174.876 ms / 174.153
ms wall medians. The upstream-entry values above are a correction to measure
the workload users actually compile, not a regression.

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

On a passing run, `results.md` contains Project, Commit, Grass median (ms), Dart
median (ms), and Speedup. If any project fails, it adds a `## Failures` section
above the table with each project's status and error signature.

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
versus Grass’s interleaved order. The four ERROR details are preserved in
`BASELINE.json` and the filed discovery todos, while their `results.md` cells
remain empty; Dart Sass compiles each of those four entries, so they are
discovery candidates rather than manifest drops. No compiler code was changed.
Dropped projects and reasons are recorded in the manifest; there are no silent
corpus caps.

To re-pin, update each project’s commit from its repository’s default HEAD,
verify the detached checkout, rerun `node bench/real-world/run.mjs all` twice,
inspect the raw diff signatures and errors, then update `BASELINE.json` only
when the change is understood. Do not add this corpus to the per-PR pipeline;
it is a local/manual gate.

## Layout and ownership

- `fixtures/`: pinned-source fetcher/resolvers and the deterministic extend
  synthetic. Workloads use upstream project entry points; no hand-modified
  entries are benchmark fixtures.
- `scripts/`: executable perf gate, profiling, the compatibility wrapper, and the
  consolidated cross-engine runner.
- `diagnostics/`: memory, repeated-compile, and persistent-WASM spec tools.
- `real-world/`: manifest, runner, committed baseline, and generated results.
- `package.json`: local sass-embedded and package benchmark dependencies.

The corpus attribution is to sasso’s `bench/real-world/projects.json`; its
license metadata and exclusions are preserved in the manifest rather than
silently copied into the working tree.
