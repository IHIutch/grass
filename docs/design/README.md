# Design documents

This directory keeps design notes that still explain a live contract or
workflow. Completed spikes are retired when their implementation has shipped
or their plan has been superseded; the original documents remain recoverable
from git history.

## Live

- [`source-maps.md`](source-maps.md) — the authority for byte-exact source-map
  rules. It is cited by five source and test files, including
  `crates/compiler/src/lib.rs`, `crates/compiler/src/source_map.rs`,
  `crates/lib/src/main.rs`, and the source-map tests.
- [`fuzzing-quality-pipeline.md`](fuzzing-quality-pipeline.md) — the live
  scheduled fuzz-smoke workflow and its quality/conformance interpretation;
  the workflow is `.github/workflows/fuzz-smoke.yml`.

## Retired

- `js-api-functions-importers.md` — RETIRED 2026-07-14. This was the design
  spike for the N-API functions/importers bridge. The bridge shipped in
  production (#221), so the spike's implementation plan is no longer a
  useful source of truth. The full pre-retirement document is recoverable at
  commit `5380ef37` with `git show 5380ef37:docs/design/js-api-functions-importers.md`.
- `performance-roadmap.md` — RETIRED 2026-07-14. Its 205 ms-era baselines,
  resolved hotspot list, and “PGO not yet shipped” premise are obsolete; the
  executable measurement approach now lives in `bench/README.md` and
  `bench/scripts/perf.sh`. The full pre-retirement document is recoverable at
  commit `5380ef37` with `git show 5380ef37:docs/design/performance-roadmap.md`.

## Rationale preserved from the retired spikes

### N-API callback boundary

The bridge has two callback mechanisms because the synchronous and asynchronous
compile entry points cross different thread boundaries. `compile` and
`compileString` execute Rust on the JavaScript main thread, so a blocking
`ThreadsafeFunction` call back to that same thread would deadlock; functions
and importers must therefore hold a JS function reference and invoke it
directly and synchronously. `compileAsync` and `compileStringAsync` run
`compute()` on a worker thread with no JS environment, so they need a
`ThreadsafeFunction` to schedule the callback on the main loop and a channel
round trip for the worker to receive the callback's result. Promise-returning
callbacks require a future, non-blocking awaitable protocol that suspends the
Sass operation; they cannot be added by simply blocking the worker. This
sync-callback boundary is what forced the bridge's two-mechanism shape.

### Performance fixture shapes

USWDS and Bootstrap v5.0.2 remain paired because they exercise materially
different Sass workloads. USWDS is `@use`-module-heavy, with a deep graph of
roughly 90 partials, and stresses import-graph and filesystem resolution.
Bootstrap v5.0.2 is legacy `@import`-heavy and `@each`-heavy, generating CSS
through evaluator, value, and serialization paths. Together they prevent a
performance change from being tuned to only one import-graph shape. The pinned
fixture workflow and this rationale are maintained in the
[fixtures section of `bench/README.md`](../../bench/README.md#fixtures).
