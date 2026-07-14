# grass benchmarking

`bench/` contains performance tooling and parity corpora. It is separate from
`ci/`, which owns spec conformance and the USWDS byte-zero gate. Source trees
are fetched at pinned commits; the three custom USWDS entries and the extend
synthetic are tracked.

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
bash bench/scripts/perf.sh compare --base "$(git merge-base HEAD main)" --workload all
```

For a one-binary smoke measurement with no verdict:

```sh
bash bench/scripts/perf.sh quick
```

Fixture resolution is `PERF_FIXTURE_DIR` → fetched pinned tree → legacy
`bench/fixtures/packages` (or `bootstrap-bench`) for users who still have it.

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

## Reference values

These are plain-release, same-session measurements on the pinned fixtures.
They are documentation only; future gates compare base and candidate binaries
in the same run.

| Workload | Base instructions | Candidate instructions | Base wall median | Candidate wall median |
|---|---:|---:|---:|---:|
| USWDS direct | 1,825.9M | 1,826.0M (+0.01%) | 174.876 ms | 174.153 ms |
| Bootstrap | 627.6M | 627.4M (-0.03%) | 56.390 ms | 56.396 ms |
| Extend synthetic | 33.3M | 33.0M (-0.90%) | 5.936 ms | 5.649 ms |

Measured with 10 interleaved pairs, pair 1 discarded, three hyperfine warmups,
and 10 hyperfine wall runs on 2026-07-13. The machine load was elevated but
below the logical-CPU warning threshold; rerun on a quiet machine before using
these as an adjudication.

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

- `fixtures/`: pinned-source fetcher/resolvers, tracked custom USWDS entries,
  and the deterministic extend synthetic.
- `scripts/`: executable perf gate, profiling, the compatibility wrapper, and the
  consolidated cross-engine runner.
- `diagnostics/`: memory, repeated-compile, and persistent-WASM spec tools.
- `real-world/`: manifest, runner, committed baseline, and generated results.
- `package.json`: local sass-embedded and package benchmark dependencies.

The corpus attribution is to sasso’s `bench/real-world/projects.json`; its
license metadata and exclusions are preserved in the manifest rather than
silently copied into the working tree.
