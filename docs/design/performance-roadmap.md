# Performance roadmap

> Paths in this historical document predate the `bench/` reorganization (2026-07-13).

**Status:** spike deliverable for Plan 014 (solo todo #132). Measured, not implemented — see
"Ranked backlog" for follow-up work.

**Goal restated:** the maintainer's stated goal is for grass to be *decisively* the fastest Sass
compiler available, not just incrementally ahead. This document replaces informal prior claims
("~2x faster than dart-sass", README, measured 2023) with numbers measured this session, and
turns "be the fastest" into a concrete target metric plus a ranked, profile-backed backlog.

**Target metric (proposed):** grass native should stay **≥5× the `sass` npm package (pure-JS
dart-sass build) and ≥2.5× `sass-embedded`** on both reference workloads below. Both bars are
already cleared today (see baseline matrix) — the number to defend going forward is the **PGO
gap**: grass native should be brought to within 5% of its own PGO-optimized ceiling in shipped
artifacts (currently 16% off, see PGO section). That is a concrete, machine-checkable target the
existing `perf-check.sh` gate can enforce once PGO ships.

## Machine and toolchain

- Apple M1 Pro, macOS 15.6.1 (24G90), arm64
- `rustc 1.94.0 (4a4ef493e 2026-03-02)`, Node v23.4.0
- Measured 2026-07-02, worktree `grass-advisor-014` at HEAD `18dcfbd` (branch `advisor/014-perf-roadmap`)
- **Machine-load caveat (see todo #123):** this machine's ambient load (GUI/WindowServer,
  background daemons) inflates naive `perf-check.sh`-style measurements by 15–35% over a
  genuinely idle machine — confirmed again this session (`perf-check.sh`'s 3-run median came back
  291ms against the stored 241ms baseline, with a 467ms cold first run, on unchanged code). All
  numbers in this document use **hyperfine with ≥10 runs and a 3-run warmup**, which produced
  spreads of 0.2–2% throughout — an order of magnitude tighter than the naive 3-run script — and
  should be treated as the reliable signal. The `.perf-baseline` value (241ms) is stale for this
  machine's current ambient load; recommend re-baselining with a hyperfine-based check (see
  "Harness recommendations").

## Baseline matrix

Two workloads, chosen to guard against overfitting a single import graph shape:
- **USWDS** (`prototype/packages/uswds/_index-direct.scss`) — `@use`-module-heavy, ~90 partials,
  deep import graph. This is the existing `perf-check.sh`/`bench.sh` workload.
- **Bootstrap v5.0.2** (`scss/bootstrap.scss`, cloned fresh this session from
  `github.com/twbs/bootstrap` tag `v5.0.2` into `/private/tmp`, not committed) — legacy `@import`
  based, `@each`-loop-heavy utility generation, smaller file count. Roughly 5× less total output
  than USWDS, and structurally very different (see hotspot section — the two workloads are
  dominated by different code paths).

All rows: `hyperfine --warmup 3 --runs 10`, same session, no concurrent builds.

### USWDS

| Engine | Mean | Spread (σ) | vs grass-native |
|---|---|---|---|
| **grass native (CLI)** | **257.2 ms** | ±1.4 ms (0.5%) | 1.00× |
| grass napi-rs (release-napi profile, in Node) | 311.3 ms | ±2.5 ms | 1.21× slower |
| grass WASM (in Node, wasm-opt -O4) | 737.5 ms | ±4.3 ms | 2.87× slower |
| sass-embedded (dart-sass native VM via IPC) | 738.1 ms | ±3.8 ms | 2.87× slower |
| `sass` npm 1.97.3 (pure-JS dart2js build, via `npx`) | 1950.0 ms | ±17 ms | 7.58× slower |

### Bootstrap v5.0.2

| Engine | Mean | Spread (σ) | vs grass-native |
|---|---|---|---|
| **grass native (CLI)** | **52.4 ms** | ±0.5 ms (1.0%) | 1.00× |
| grass napi-rs | 100.0 ms | ±1.1 ms | 1.91× slower |
| grass WASM | 234.9 ms | ±2.4 ms | 4.48× slower |
| sass-embedded | 315.3 ms | ±3.9 ms | 6.01× slower |
| `sass` npm 1.97.3 (pure-JS) | 901.9 ms | ±10.3 ms | 17.20× slower |

**Reading this:** grass native already clears a 5× margin over the JS dart-sass build and a
2.5× margin over sass-embedded on both workloads — comfortably. The gap is *narrower and more
workload-dependent* for the napi/WASM builds (1.2–1.9× on native-code paths, up to 4.5× for WASM),
which matters because those are the artifacts most JS-ecosystem users actually consume (Node
users get napi via `grass-wasm`'s native-binding fallback path; browser/edge users get WASM only).
**The in-Node-process comparisons (napi vs sass-embedded, WASM vs sass-embedded) are the ones
actually being decided at typical JS-Sass adoption time** — grass napi beats sass-embedded by
2.4× (USWDS) to 3.2× (Bootstrap); grass WASM is *essentially at parity* with sass-embedded on
USWDS (737.5ms vs 738.1ms, ~0.1% apart — within measurement noise) and only 1.34× ahead on
Bootstrap. WASM is the weakest link in the "decisively fastest" story and is worth its own
investigation (see backlog).

Sanity check: symbol-stripped release binary (the shipped default, `strip = "symbols"` in
`Cargo.toml`) vs. a `debug`-symbol-preserving build used for profiling below showed no measurable
runtime difference (256.1ms vs 257.1ms, well within noise) — stripping only affects binary size,
not the numbers above.

## Hotspot table

Captured with `samply record --iteration-count N --unstable-presymbolicate` (2kHz sampling) on
the shipped release codegen (stripping doesn't affect runtime, confirmed above; symbols were kept
for this specific binary only to resolve names). USWDS: 40 iterations, 22,395 samples. Bootstrap:
100 iterations, 10,876 samples. Full symbolicated self+inclusive tables are in the session
transcript; below is the top self-time by classification, merged across both workloads.

| Function / cluster | USWDS self | Bootstrap self | Classification | Feeds backlog item |
|---|---|---|---|---|
| `_platform_memmove` | 8.59% | 3.56% | allocation-adjacent (copies) | #4 Value repr |
| `__getattrlist` / `stat` / `__open` / `read` (fs resolution) | 12.22% | 7.03% | I/O-bound | #3 import resolution |
| `_tlv_get_addr` + `LocalKey::with` (TLS) | 5.81% | 3.41% | TLS/threading overhead | #5 interner |
| `mi_malloc_aligned` / `mi_free` / `mi_find_page` / `mi_page_free_list_extend` / `mi_large_huge_page_alloc` / `_mi_heap_realloc_zero` / `_mi_malloc_generic` (mimalloc internals) | ~13.7% | ~8.2% | allocation-bound | #1 PGO, #4 Value repr |
| `Visitor::visit_expr_ref` | 3.19% | 9.83% | algorithmic (evaluator) | — core loop |
| `Environment::get_var` | 1.57% | 5.25% | algorithmic (env lookup) | #4 Value repr |
| `Value::clone` + `AstExpr::clone` + `drop_in_place<Value>` | ~4.4% | ~12.3% | allocation-bound (clone churn) | #4 Value repr (new) |
| `MapView::{Base,Public,Merged}::get` (×3 variants) | 4.76% | — (not in top40) | algorithmic (member lookup) | — |
| `Value::eq` | 2.85% | — | algorithmic | — |
| `ExtensionStore::clone` + `Extension::clone` + `drop_in_place<ExtensionStore>` | 9.22% (inclusive) | 3.05% (`add_selector`, inclusive) | allocation-bound | #2 extend (Plan 008) |
| `Serializer::visit_value` / `visit_stmt` / `visit_group` | 0.29% (inclusive) | 5.66% (inclusive, `visit_stmt`+`visit_group`) | algorithmic | #2 serializer (Plan 007) |
| `Environment::get_fn` / `Module::get_fn` | 0.37% self / 4.36% incl. | not in top40 self | algorithmic (lookup) | #7 phf (estimate only, see below) |
| `alloc::collections::btree::*` (BTreeMap/Set) | 0.63% self | — | algorithmic (ordered map overhead) | #8 BTreeMap→FxHashMap |
| Parser (`StylesheetParser::*`, `TokenLexer::next`, `ValueParser::parse_expression`) | ~6–8% combined, mostly nested in `load_style_sheet` (19.93% inclusive) | ~7.5% combined (nested in `load_style_sheet`, 13.80% inclusive) | mix (algorithmic + some I/O) | not separately actionable — already low relative to evaluator |
| Recursion-depth guards (Plan 005, `with_children`/`parse_paren_expr` checks) | not visible in top 80 | not visible in top 40 | unavoidable | **confirmed <1%, invisible in profile** — Plan 005 concern closed |

**The two workloads are dominated by different things**, which is the whole point of using two:

- **USWDS** (import-graph-heavy): fs resolution (12.2% self / ~19% inclusive via
  `load_style_sheet`) and mimalloc internals (13.7% self) dominate. `canonicalize`/`realpath`
  alone is 6.9% inclusive despite the canonicalize cache landed 2026-03-07 — the cache helps
  repeated resolutions, but USWDS's ~90-partial `@use` graph mostly resolves each path *once*,
  so cold-cache cost dominates.
- **Bootstrap** (`@each`-loop-heavy, CSS-generation-heavy): evaluator/`Value` churn dominates —
  `visit_expr_ref` (9.83%), `Environment::get_var` (5.25%), `Value`/`AstExpr` clone+drop (~12.3%
  combined) are far higher than on USWDS. Serialization is also 10× more prominent (5.66% vs
  0.29% inclusive) because Bootstrap's loops generate proportionally more CSS output per byte of
  input.

**Implication for prioritization:** an optimization that only helps import resolution (USWDS-
shaped) will underserve `@each`/loop-heavy real-world stylesheets like Bootstrap, and vice versa.
The backlog below is ordered with this in mind.

## Cheap-lever validation (Step 3)

### 1. PGO — measured, not yet shipped

Same-session A/B, standard release vs `build-pgo.sh`-produced binary, `hyperfine --warmup 3
--runs 15`, both binaries built back-to-back with no other cargo activity:

| Workload | Standard | PGO | Speedup |
|---|---|---|---|
| USWDS | 258.4 ms ± 1.7 ms | 221.9 ms ± 1.7 ms | **1.16×** (13.9% faster) |
| Bootstrap | 53.0 ms ± 1.2 ms | 45.7 ms ± 0.6 ms | **1.16×** (13.8% faster) |

This **meets and slightly exceeds** the script's stale "~11%" comment, consistently across both
workload shapes (14% vs a naive average of ~11% — worth updating the comment in `build-pgo.sh`).
The gain is workload-shape-independent, which is a good sign it reflects a real, general codegen
improvement (branch layout / inlining around the hot allocator and clone paths above) rather than
overfitting to USWDS.

**Not shipped anywhere today** — `build-pgo.sh` is a local dev script, not referenced by any
`.github/workflows/*.yml`. Bringing it into CI requires solving: the CLI release build
(`[profile.release]`) and the **napi release build now uses a separate `[profile.release-napi]`**
(added by Plan 004, `panic = "unwind"` vs `abort`) — both profiles need their own PGO instrumentation
pass, and PGO profiling requires *running the actual binary on real hardware of the target
architecture* during the build, which cross-compiled CI targets (e.g. building `linux-arm64-gnu`
napi artifacts on an x86 runner, or any QEMU-based cross target) cannot do. Options: (a) restrict
PGO to natively-built targets only (x86_64/aarch64 on matching runners) and ship non-PGO builds
for cross-compiled targets, or (b) skip PGO for napi/WASM release entirely and apply it only to
the CLI binary releases, which are built natively per-target already. This needs its own scoping
pass before landing — not attempted here per the plan's out-of-scope note on CI changes.

### 2. CLI mimalloc — already landed, no action taken

The plan's premise ("the CLI does not use mimalloc although the napi build does") is **stale**.
`crates/lib/Cargo.toml`'s `commandline` feature already pulls in `mimalloc` as the CLI's global
allocator, landed in commit `2b861963` (2026-03-07, "mimalloc + canonicalize cache: 710ms → 353ms
(50% faster, 2x dart-sass)"). `crates/lib/src/main.rs:1-3` gates the global allocator on
`#[cfg(feature = "mimalloc")]`, and `commandline = ["clap", "mimalloc"]`. Verified live during
this session's builds (`--cfg feature="mimalloc"` present in the `rustc` invocation for the
`grass` binary). **No quick-win diff needed here** — this candidate is done and should be
considered closed in the plan/todo body, not re-attempted.

### 3. phf for `GLOBAL_FUNCTIONS` — estimated, not implemented (per plan: estimate only)

The `# todo: benchmark using phf for global functions` comment (`crates/compiler/Cargo.toml:33`)
predates this profile. Findings: `Environment::get_fn`/`Module::get_fn` (the callable-lookup path
that hits `GLOBAL_FUNCTIONS`) is **0.37% self-time on USWDS and doesn't appear in Bootstrap's top
40 self-time list at all** — i.e., the hashmap lookup itself is not hot; the *inclusive* time
attributed to `get_fn` (4.36% USWDS, mostly not present on Bootstrap) is dominated by running the
resolved builtin, not finding it. `GLOBAL_FUNCTIONS` is already an `FxHashMap` (fast, non-crypto
hash) — a `phf` (compile-time perfect hash) swap would only remove the hash-and-probe cost of a
single lookup per function call, which this profile shows is not a measurable cost.
**Recommendation: do not implement; remove or resolve the stale TODO comment.** This is a
negative result worth recording so a future session doesn't re-investigate it.

## Ranked backlog

Each item below is filing-ready — the reviewer should open a solo todo per item (beads/`bd` is
removed from this repo per todo #132 comment 31).

---

**#1 — Ship PGO for the CLI release binary**
Expected gain: **measured 1.16× (≈14%) on both reference workloads**, same-session A/B, this
session.
Effort: Medium. Risk: Low-Medium (build-pipeline complexity, not code-correctness risk).
Prerequisites: scope which release targets can run a native profiling pass in CI (see PGO section
above — cross-compiled targets are blocked without a redesign). Start with CLI-only, natively-built
targets (macOS arm64/x64, Linux x86_64-gnu at minimum) before attempting napi/WASM.
Notes: `build-pgo.sh`'s "~11%" comment should be updated to reflect the measured ~14% once this
lands. This is the single highest-confidence, highest-value item in this backlog — it's a build
change, not a code change, and the number is solidly measured twice (script's own hyperfine +
independent same-session A/B).

---

**#2 — Reduce `@extend`/selector-extension allocation (Plans 007/008)**
Expected gain: profile-confirmed hotspot — `ExtensionStore::clone` + `Extension::clone` +
`drop_in_place<ExtensionStore>` = **9.2% inclusive on USWDS**; `ExtensionStore::add_selector` =
3.05% inclusive on Bootstrap. Serializer (`Serializer::visit_stmt`/`visit_group`) separately
contributes 5.66% inclusive on Bootstrap (much higher than USWDS's 0.29%) — CSS-output-heavy
workloads pay more serializer cost.
Effort: Medium (existing plans 007/008 already scope this — this profile confirms and
re-prioritizes them, doesn't replace them).
Risk: Medium — touches core selector/extend correctness-sensitive code.
Prerequisites: none; ready to pick up. Recommend re-reading Plans 007/008 alongside this profile
data before starting, since the split between extend-clone cost (USWDS-dominant) and serializer
cost (Bootstrap-dominant) suggests these may want separate PRs even if historically bundled.

---

**#3 — Import/fs-resolution overhead**
Expected gain: `canonicalize`/`realpath` = 6.9% inclusive, `__getattrlist`/`stat`/`__open`/`read`
combined = 12.2% self on USWDS (a `@use`-heavy workload); notably smaller on Bootstrap (7.0% self)
— this is USWDS-shaped, not universal.
Effort: Medium. Risk: Low-Medium (caching correctness — must not change import resolution
semantics, especially around case-sensitivity/symlinks that `canonicalize` exists to handle
correctly).
Prerequisites: the 2026-03-07 canonicalize cache (commit `2b861963`) already exists — investigate
why cold-path cost still dominates before adding more caching (likely: cache only pays off on
*repeated* resolution of the same path, and large `@use` graphs mostly resolve each path once).
Consider: reducing the number of candidate paths probed per `@use` (fewer `stat`/`__getattrlist`
calls per resolution), or batching directory-existence checks. Related: todo #146 (untracked
`prototype/packages` fixture) is orthogonal — don't conflate.

---

**#4 — `Value`/`AstExpr` clone-and-drop churn (NEW, no existing plan)**
Expected gain: profile-derived estimate — `Value::clone` + `AstExpr::clone` + `drop_in_place<Value>`
together are **~4.4% self on USWDS but ~12.3% self on Bootstrap** (loop-heavy workloads pay far
more). `Environment::get_var`/`insert_var` (which return/store cloned `Value`s) add another
5.25%/2.38% self on Bootstrap. This is the single largest *uncaptured-by-any-existing-plan*
cluster found this session, and it's the dominant cost on the more realistic "typical stylesheet
with loops" shape (Bootstrap) rather than the "huge module graph" shape (USWDS).
Effort: High — likely requires wrapping expensive `Value` variants (lists, maps, strings) in `Rc`
so clones become refcount bumps instead of deep copies, similar in spirit to the arena-AST work
already landed. This is a design-level change to the `Value` representation, not a local fix.
Risk: High (touches every evaluator code path; correctness-sensitive around mutation semantics
if `Value` is ever mutated in place after a "clone").
Prerequisites: none technically, but this deserves its own design spike before implementation —
don't attempt as a quick win. Recommend prioritizing this above #3 for real-world impact once #1
and #2 are done, given how dominant it is on the loop-heavy workload.

---

**#5 — Thread-local interner access overhead**
Expected gain: `_tlv_get_addr` + `std::thread::local::LocalKey::with` = **5.81% self on USWDS,
3.41% self on Bootstrap** — pure TLS-indirection cost from every `InternedString::get_or_intern`/
`resolve`/`resolve_ref` call going through `STRINGS.with(...)`.
Effort: Medium-High. Risk: Medium — `crates/compiler/src/interner.rs` already documents (see the
type-level doc comment on `InternedString`) that it *cannot* be made `!Send`/`!Sync` without
restructuring the `Unit` conversion tables in `crates/compiler/src/unit/conversion.rs`, which is
the actual blocker, not a minor refactor.
Prerequisites: this is also the **hard prerequisite for any future intra-compilation
parallelism** (already flagged in the plan and in the interner's own doc comment) — two
independent motivations point at the same fix. Recommend scoping this as "redesign the interner's
storage to avoid `thread_local!` in the single-threaded hot path" as a named follow-up plan,
referencing both this perf angle and the parallelism angle so it isn't scoped too narrowly for
either.

---

**#6 — `stacker::maybe_grow` at Plan-005 recursion chokepoints (todo #148)**
Expected gain: not measurable from this profile — Plan 005's own recursion-depth checks are
**confirmed invisible in both hotspot tables (<1%, doesn't appear in top 80/40 self-time)**, so
this item is about removing the depth-limit/napi-stack tension, not about a currently-visible
perf cost. Effort/risk: per todo #148 — this session did not re-scope it, just confirms the
depth-check overhead itself is a non-issue perf-wise.

---

**#7 — `phf` for `GLOBAL_FUNCTIONS`: do not implement**
See "Cheap-lever validation" above. Estimated gain: negligible (<0.5% of total runtime at best,
likely much less). Recommend closing this as a resolved-negative and removing/updating the stale
`# todo: benchmark using phf` comment in `crates/compiler/Cargo.toml:33`.

---

**#8 — `BTreeMap`/`BTreeSet` → `FxHashMap`/`FxHashSet` where ordering is unneeded**
Expected gain: small but real and confirmed — `alloc::collections::btree::map::IntoIter::dying_next`
and `drop_in_place<BTreeSet<usize>>` together are ~0.9% self on USWDS. Low priority on their own,
but low-effort/low-risk — good opportunistic cleanup to bundle with other work in the same files
rather than a dedicated effort.
Effort: Low. Risk: Low (verify no code relies on iteration order — check call sites individually).

---

**#9 — WASM performance gap (NEW, no existing plan)**
Observation, not yet root-caused: grass WASM is the *weakest* comparison point in the baseline
matrix — only at parity with sass-embedded on USWDS (0.5% apart) despite grass native being
2.87× faster than sass-embedded on the same workload. This ~2.9× native-to-WASM gap is larger
than typical WASM overhead (usually 1.2–2×) and suggests either (a) the `fsCallbacks` JS↔WASM
boundary crossings (`bench-breakdown.js` already instruments this — worth running and including
in a follow-up) or (b) WASM codegen (`wasm-opt -O4` may not be capturing what LTO+PGO capture
natively) is costing more than expected. Not profiled this session (samply doesn't profile WASM
directly) — recommend `bench-breakdown.js`'s existing fs-boundary-crossing counter as the starting
point for a follow-up investigation, since it already measures exactly this.
Effort: Unknown (needs investigation first). Risk: N/A yet.

---

## Harness recommendations

- **Use hyperfine, not the naive `perf-check.sh` 3-run median**, for anything where a >5%
  regression gate matters. This session's naive 3-run measurement (291ms) diverged from the
  hyperfine 10-run measurement (257ms) by 13%, and the naive script's own cold-first-run problem
  (467ms) is a known false-positive source (todo #123). Recommend either replacing
  `perf-check.sh`'s internals with a `hyperfine --warmup N --runs M --export-json` call, or
  keeping it as a fast smoke-test and documenting that a hyperfine run is required before trusting
  a reported regression.
- **Re-baseline `.perf-baseline`** once PGO or another confirmed improvement lands — 241ms
  reflects a machine/session state that isn't reproducible today (this session measured 256–258ms
  on unchanged code, consistently, across multiple binaries and hyperfine runs).
- **Two-workload discipline going forward**: this session's biggest structural finding is that
  USWDS and Bootstrap stress completely different code paths (fs resolution vs. evaluator/clone
  churn). Any future perf claim or regression check should run both, not just USWDS — a
  single-workload regression gate will miss regressions in whichever path that workload doesn't
  exercise. Bootstrap v5.0.2 was not committed into the repo (cloned to `/private/tmp` per this
  plan's instructions) — if adopted permanently, it needs a real home (either vendored like
  `prototype/packages/uswds` or fetched by a setup script, matching how USWDS is currently
  handled).
