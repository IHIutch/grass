# Fuzzing and conformance quality pipeline

This is a bounded, scheduled smoke pipeline for high-risk input and runtime
boundaries. It is deliberately separate from performance measurement: a fuzz
run records executions, corpus inputs, crashes, and hangs; it does not produce
a throughput or latency baseline.

## Risk-ranked target matrix

Every target has an input model, an oracle, a bound, and an owner. A normal
Sass error is an expected result for parser and resolver targets; a panic,
process abort, hang, or unexpected bridge exception is a finding.

| Rank | Target | Input model | Oracle | Smoke bound | Owner |
| --- | --- | --- | --- | --- | --- |
| P0 | `from_string_parsing` | UTF-8 byte strings interpreted as default SCSS | Compile, reject, or report a Sass error; never panic or hang | 64 runs, 20 s, 2 s/input | compiler/parser |
| P0 | `from_string_css` | UTF-8 byte strings forced through CSS syntax | Same no-panic/no-hang oracle; CSS errors are valid | 64 runs, 20 s, 2 s/input | compiler/CSS parser |
| P0 | `from_string_sass` | UTF-8 byte strings forced through indented Sass syntax | Same no-panic/no-hang oracle; Sass errors are valid | 64 runs, 20 s, 2 s/input | compiler/Sass parser |
| P0 | `import_resolution_virtual_fs` | Seed bytes choose `@use` vs `@import`, partial vs explicit file, and a bounded virtual file value | Virtual `Fs` callbacks resolve or decline deterministically; no panic, abort, or hang | 64 runs, 20 s, 2 s/input | importer/filesystem |
| P1 | `napi_callbacks.mjs` | Deterministic seeds generate numeric/string callback values, full importers, and sync/async entry points | Native process stays alive; successful callbacks return valid Sass values and callback errors surface as JS errors | 64 cases, 30 s total | N-API bridge |
| P1 | `wasm_callbacks.mjs` | Deterministic seeds generate virtual files and exercise `is_file`, `is_dir`, `read`, `canonicalize`, and directory listing callbacks | WASM completes or returns a compile error; callback exceptions do not crash the host | 64 cases, 30 s total | WASM/runtime |

The first four rows are libFuzzer targets. The last two are JavaScript runtime
harnesses because N-API and WASM callback protocols cannot be linked into the
Rust fuzz package. The scheduled workflow builds each runtime fixture before
starting its harness.

## Scheduled smoke and artifacts

`.github/workflows/fuzz-smoke.yml` runs weekly and can also be started manually.
It uses a bounded run count and per-input timeout, then uploads the following
paths even when a target fails:

```text
fuzz/corpus/<target>/       deterministic seed corpus, if present
fuzz/artifacts/<target>/   libFuzzer crash artifacts and target logs
```

The workflow prints each target's corpus and artifact path and records the
commit SHA in its runtime summary. A finding is reproducible by running the
same target at that SHA with the retained artifact as the corpus input.

`cargo-fuzz` is installed inside the scheduled runner. Local contributors do
not need a global cargo-fuzz installation to check the targets: the nightly
toolchain can compile them with `cargo +nightly check --manifest-path
fuzz/Cargo.toml --bins`. Full local fuzz execution is optional and requires
the contributor's own cargo-fuzz installation.

## Conformance and runtime coverage

The Sass conformance signal is the blocking `sass-spec` job in `tests.yml`,
which compares the current result with `ci/sass-spec-baseline.txt`. The
baseline is versioned and visible: old failures remain listed, resolved
failures are reported by the checker, and any failure not in the baseline
fails the job. The baseline is not a fuzz result and must not be interpreted
as a performance target.

The scheduled smoke summary reports runtime coverage as target cases started,
target cases completed, expected compile errors, and crash/hang artifact
count, all attributable to `GITHUB_SHA`. It does not report execution time as
a quality score. Performance changes belong in the performance roadmap and
its separately maintained benchmark baselines.

## Triage and escalation

1. Preserve the uploaded target log, crash artifact, corpus seed, workflow
   run URL, and commit SHA. Do not delete or minimize the reproducer before
   recording it.
2. Reproduce locally with the exact target and artifact. Classify the result
   as parser, resolver/filesystem, N-API, WASM, or infrastructure failure.
3. Create a Solo todo for every confirmed product finding. Include the target,
   SHA, artifact path, minimized input (if available), first bad commit, and
   affected runtime matrix.
4. Mark the finding a release blocker when it is a reproducible crash, abort,
   hang, memory-safety signal, or a compatibility regression in a supported
   runtime. A transient runner/toolchain failure is an infrastructure todo,
   not a product blocker.
5. A release blocker stays open until the fix, a regression test, and a
   rerun of the affected target pass. Update the seed corpus or baseline only
   when the change is reviewed and the reason is recorded.

Review seeds and baselines whenever parser, importer/filesystem, N-API, or
WASM boundaries change.
