# Source maps: design spike and prototype

**Status:** design spike deliverable for Plan 013 (solo todo #131). Prototype behind an
off-by-default option; not wired into any user-facing surface. See "Deferred slices" for what's
left.

**Why this matters:** source maps are the marquee missing dart-sass feature. Today no surface can
produce them: `Options` had no source-map field, the CLI declares four source-map flags that are
silently ignored (`crates/lib/src/main.rs:118-145` — declared, never read by `main()`), and the npm
package hardcodes `sourceMap: undefined` (`crates/lib/pkg-publish/index.js:109`, typed
`sourceMap?: undefined` in `index.d.ts:4`). This blocks adoption by anyone who debugs compiled CSS
in browser devtools or a bundler pipeline. This spike pins the design and de-risks the hardest
part: collecting `(output position → input span)` mappings in the serializer without disturbing
byte-exact output.

## dart-sass contract (observed, not assumed)

All facts below were captured by running `npx sass@1.97.3` and the JS API against small
fixtures (`echo 'a { b: c; }' | ...`, and a two-file `@use` fixture), not inferred from
documentation.

### Map JSON shape

```json
{
  "version": 3,
  "sourceRoot": "",
  "sources": ["in.scss"],
  "names": [],
  "mappings": "AAAA;EACE",
  "file": "out.css"
}
```

- `version` is always `3`.
- `sourceRoot` is always `""` in every configuration tested (CLI and JS API).
- `sources` lists file names in **first-appearance order during compilation** (dependency-first:
  for `@use 'partial'; .bar {...}`, `_partial.scss` appears before `main.scss` in `sources`, even
  though `main.scss` is the entry point).
- `names` was empty (`[]`) in every fixture tried (no identifier-renaming scenarios were exercised
  by grass's declaration/selector-only feature set — dart-sass likely populates this for
  SassScript identifiers echoed into output, which is out of this spike's scope).
- `mappings` is the standard VLQ-encoded string (semicolon = new generated line, comma = new
  segment on the same line). Decoded and verified by hand — see "VLQ semantics" below.
- `file` (CLI only) is the output CSS file's name. **The JS API's `sourceMap` object omits `file`
  entirely** — confirmed via `sass.compileString(..., {sourceMap: true})`, whose result keys are
  exactly `{css, sourceMap, loadedUrls}` and whose `sourceMap` has no `file` key. `file` is added
  by the CLI wrapper, not the compiler core.

### Trailer comment and flag interactions

| Flags | Output |
|---|---|
| (default) | CSS + trailing `\n\n/*# sourceMappingURL=out.css.map */`, plus `out.css.map` written |
| `--no-source-map` | CSS only, no trailer, no `.map` file |
| `--style=compressed` | Single-line CSS (`a{b:c}`), trailer still appended, same map shape |
| `--source-map-urls=absolute` | `sources` becomes `["file:///private/tmp/sm/in.scss"]` (absolute `file://` URL) instead of the relative `"in.scss"` |
| `--embed-sources` | Adds a `sourcesContent` array (verbatim source text) alongside `sources` |
| `--embed-source-map` | No `.map` file; trailer becomes a `data:application/json;charset=utf-8,<url-encoded JSON>` URL instead of a filename |
| stdin input | `sources` becomes a **data URL** of the input itself (`["data:;charset=utf-8,a%20%7B...%0A"]`), not a synthetic filename — and the `.map` file is still written and referenced normally |

The four dormant grass CLI flags (`NO_SOURCE_MAP`, `SOURCE_MAP_URLS`, `EMBED_SOURCES`,
`EMBED_SOURCE_MAP`) map 1:1 onto dart-sass's `--no-source-map` / `--source-map-urls` /
`--embed-sources` / `--embed-source-map` — no surprises, no additional flags needed.

### VLQ semantics (decoded by hand, then confirmed by the prototype)

Each mapping segment is 4 VLQ fields: `[dst_col, src_file_idx, src_line, src_col]` (a 5th `name`
field exists per-spec but is unused here since `names` is always empty for this feature set).

- `dst_col` (generated column) resets to `0` at the start of every generated line, and each
  segment's value is a **delta from the previous segment on the same line**.
- The other three fields (`src_file_idx`, `src_line`, `src_col`) are each a running delta
  **cumulative across the entire mappings string**, not reset per line and not reset per source
  file. This is the easy-to-miss part: e.g. in the two-file fixture, the source-line field for the
  first mapping in `main.scss` is a delta from the *last line value seen in `_partial.scss`*, even
  though they're different files.

Worked example, input `a {\n  b: c;\n}\n`, dart-sass output `"mappings":"AAAA;EACE"`:

- Line 0 group `"AAAA"` → all-zero deltas → dst col 0 maps to source file 0, line 0, col 0 (the
  `a` of the selector).
- Line 1 group `"EACE"` → decodes to `[+2, +0, +1, +2]` → dst col 2 (after 2-space indent) maps to
  source file 0, line 1 (delta +1), col 2 (the `b` of `b: c;`).

This exact string is what the prototype (below) reproduces byte-for-byte for this fixture — see
`crates/lib/tests/source_maps.rs::first_mapping_matches_hand_computed_dart_sass_output`.

## Collection design

### Do spans survive to the serializer? Yes — traced end-to-end

**Declarations:** `crates/compiler/src/ast/style.rs:7-13` defines `Style { property, value,
declared_as_custom_property, property_span }`. `property_span` is populated at
`crates/compiler/src/evaluate/visitor.rs:994` (and a second call site at `:5366`) as
`property_span: style.span` — the parser AST declaration's own span, carried straight through
evaluation with no loss. That `Style` becomes `CssStmt::Style(style)` and reaches
`crates/compiler/src/serializer.rs`'s `write_style` (originally at old line 1586, now 1609 after
this spike's edits), which **already consumed `style.property_span` before this spike** (for
custom-property re-indentation, `reindent_buffer_from`). So declarations were provably
span-carrying at the serializer before any of this spike's code was written.

**Selectors:** `crates/compiler/src/serializer.rs`'s `visit_stmt`, `CssStmt::RuleSet` branch
(`selector.as_selector_list()` → `sel_list.span`), already consumed `sel_list.span.high()` (for
same-line comment placement, `brace_line`) before this spike. `sel_list.span.low()` is therefore
equally available — no new plumbing needed.

**Conclusion: the STOP condition ("spans do not survive to serialization for declarations") does
NOT trigger.** Both chokepoints already had the spans in scope for unrelated pre-existing
formatting logic; this spike's only change is to *also* read `.low()` off them and call
`record_mapping`.

### Where the collection points are (concrete functions/lines, post-spike)

- `serializer.rs::write_style` (property declarations) — `self.record_mapping(style.property_span.low())`
  is called immediately after `write_indentation()` and before any property-name bytes are written.
- `serializer.rs::visit_stmt`, `CssStmt::RuleSet` arm (selectors) — `self.record_mapping(sel_list.span.low())`
  is called immediately after `write_indentation()` and before `write_top_level_selector_list`.

Both call sites record the mapping *before* the node's own bytes are written, matching the
observed dart-sass convention (a mapping's `dst_col` is the position of the mapped token's first
character, after indentation).

### Output line/column tracking — avoided touching write-site call sites

The scratchpad's maintenance note flagged a real risk here: the serializer writes to its `Vec<u8>`
buffer through **dozens of call sites** (`buffer.push`, `buffer.extend_from_slice`, `write!`,
scattered across ~50+ methods — selectors, colors, calculations, comments, etc.), with no existing
wrapper type. Instrumenting every one of those to maintain a running `(line, col)` counter would be
a large, high-risk diff — exactly the second near-STOP condition ("counter cannot be maintained
without touching >10 write sites").

**This spike avoids that entirely.** Because mappings are only ever needed at two chokepoints (not
on every byte written), `record_mapping` computes the generated line/column by **scanning only the
unscanned tail of the buffer**, `self.buffer[state.scan_pos..]`, counting `\n` bytes and UTF-8
lead bytes since the last mapping was recorded, then advancing `state.scan_pos` to `buffer.len()`.
Total scan cost across a whole serialization is `O(total output size)` — each byte is scanned
exactly once, no matter how many mappings are collected — and zero write sites needed touching.
This is implemented in `serializer.rs::record_mapping` (new method).

One subtlety verified safe: `write_style`'s custom-property path (`reindent_buffer_from`)
`truncate`s and rewrites buffer bytes *after* `record_mapping` has already run and advanced
`scan_pos` past the pre-truncation length for that declaration. Since the scan always starts fresh
from `scan_pos` (an index, not a cached line/col snapshot) at the *next* mapping call, and never
re-scans a region twice, the truncate-and-rewrite is invisible to the tracker — it only ever sees
the buffer's final bytes in that region.

**Round-1 review finding, fixed:** the first version of this spike stored the five mapping fields
(`mappings: Vec<_>`, `mapping_sources: Vec<_>`, `scan_pos`/`dst_line`/`dst_col: usize`) directly on
`Serializer`, always-initialized in *both* constructors — including `new_expr`, the per-`#{...}`-
interpolation constructor used at very high frequency during expression serialization, which never
maps anything. That cost showed up as a measurable option-OFF regression (+2-4.5%, confirmed by
the reviewer's order-swap-controlled hyperfine A/B; see "Inertness gate results" below). The fix
collapses all five fields into a single `mapping_state: Option<Box<MappingState>>`: `None`
whenever the option is off, and **always** `None` in `new_expr` regardless of the option (there is
no `CodeMap` there and expression serialization never maps). `record_mapping` gates on the `Option`
and does nothing when it's `None`. Only the top-level `Serializer::new` allocates a `Box` when
`options.source_map` is `true`. This restores the off-path and the `new_expr` hot path to a single
pointer-sized field.

### VLQ: hand-rolled, not a crate

Per the repo's dep-skeptical posture (see `Cargo.toml` comments), VLQ encoding is hand-rolled in
the new `crates/compiler/src/source_map.rs` module (~70 lines including JSON assembly, tests
excluded). It has no dependencies beyond `std`. Decoding isn't needed (grass only produces maps, it
doesn't consume them), which keeps the module small.

### Source file list

`codemap::CodeMap` (pinned `0.1.3`) has no public method to enumerate all registered files — its
`files: Vec<Arc<File>>` field is private and there's no `files()` accessor. Rather than vendor a
patch or add a dependency, the prototype builds the deduplicated `sources` array incrementally: each
`record_mapping` call resolves the mapped position's file name via `CodeMap::look_up_pos(..).file.name()`
and does a linear search against the sources collected so far, appending on first sight. This is
`O(files²)` in the number of distinct source files, which is fine for realistic file counts
(dozens, not thousands) but is called out here as a known simplification — a real implementation
touching more of the codebase could add a `CodeMap::files()` accessor upstream, or maintain a
`HashMap<file name, idx>` instead of a linear scan.

## Prototype

**Scope, as planned:** mappings collected for **style declarations and selectors only**
(at-rules, comments, imports, media/supports preludes are unmapped — a real implementation would
extend `record_mapping` calls to those sites using the same pattern, no new infrastructure
needed).

**New/changed surface (all additive — no existing public signature changed):**

- `Options::source_map(bool)` (default `false`) — `crates/compiler/src/options.rs`.
- `pub fn from_string_with_source_map<S: Into<String>>(input: S, options: &Options) -> Result<(String, Option<String>)>`
  — `crates/compiler/src/lib.rs`, re-exported from `crates/lib/src/lib.rs`. Returns `(css,
  Some(map_json))` when `options.source_map()` is `true`, `(css, None)` otherwise. CSS output is
  byte-identical to `from_string` in both cases.
- `crates/compiler/src/source_map.rs` (new module) — `RawMapping`, `encode_vlq`,
  `build_source_map_json`.
- `Serializer` gained one field, `mapping_state: Option<Box<MappingState>>` (a new private struct
  holding `mappings`, `sources`, `scan_pos`, `dst_line`, `dst_col`), a `record_mapping` method, and
  a `take_mappings()` accessor — `crates/compiler/src/serializer.rs`. See "Round-1 review finding,
  fixed" above for why this is a single `Option<Box<_>>` rather than five plain fields.

**Refactor to avoid duplicating the compile driver:** `from_string_with_file_name`'s body (parse →
visit → serialize loop → finish) was extracted into a private `compile_impl` returning `(String,
Vec<RawMapping>, Vec<String>)`; both `from_string_with_file_name` (discards the mapping data) and
`from_string_with_source_map` (builds JSON from it) call the same `compile_impl`. This means the
option-off path for existing callers is unchanged — same parse/visit/serialize code runs, just
with an unused empty `Vec` returned and dropped.

**Tests** (`crates/lib/tests/source_maps.rs`):

1. `option_off_is_byte_identical_to_from_string` — `from_string_with_source_map` with the default
   `Options` produces CSS identical to `from_string`, and `map` is `None`.
2. `option_on_emits_valid_v3_json` — with `.source_map(true)`, the returned JSON string has
   `version:3`, the expected `sources`, empty `names`, and a non-empty `mappings`.
3. `first_mapping_matches_hand_computed_dart_sass_output` — for the one-rule fixture used in the
   "VLQ semantics" section above, asserts the exact substring `"mappings":"AAAA;EACE"` — the same
   string dart-sass 1.97.3 produced for the identical input. This is not just internally
   consistent; it matches the external reference implementation byte-for-byte.

All three pass. The `source_map` module also has 3 unit tests pinning individual VLQ digit
sequences (`AAAA`, `EACE`, and a negative-value case `F` = -2, all cross-checked against the
decoded dart-sass fixtures) — these run without needing a full compile.

## Inertness gate results (option off)

- **sass-spec:** `python3 run-sass-specs.py --spec-dir /Users/jbhutch/Sites/grass/sass-spec/spec`
  → **13,692/13,801 (99.2%)**, 109 failures, identical category breakdown to the pre-spike
  baseline (core_functions 59, values 21, non_conformant 11, libsass-closed-issues 5, css 4,
  directives 4, libsass 4, libsass-todo-issues 1). No change.
- **cargo test:** `~/.cargo/bin/cargo test --features=macro --no-fail-fast` → same **45
  pre-existing failures** across the same 13 targets (solo todos #144/#145) — name-set diffed
  against the baseline list and it matches exactly; no new failures, none fixed.
- **clippy:** `~/.cargo/bin/cargo clippy --features=macro -- -D warnings` → clean.
- **Perf, round 1 (five-field `Serializer`, since reverted):** initial hyperfine A/B looked clean
  (both workloads ~1.00×), but round-1 review caught it with an order-swap-controlled A/B: branch
  was consistently slower with the option off — USWDS +2.1-2.9%, Bootstrap +2.9-4.5% — across four
  measurements and both benchmark orderings. Root cause: five always-initialized mapping fields on
  `Serializer` (~64 bytes, two `Vec`s), present in **both** constructors including `new_expr`, the
  per-`#{...}`-interpolation hot path. This was the plan's own hard STOP condition
  ("perf regresses with the option off") and required a design fix, not just re-measuring — see
  "Round-1 review finding, fixed" above.
- **Perf, round 2 (after collapsing to `mapping_state: Option<Box<MappingState>>`), option off,
  base `080457e` vs branch binary, order-swap protocol (each pair run in both orderings), machine
  load checked before each run (`uptime` 1-min < 6, no `cargo`/`rustc` processes running):**
  - USWDS (`/tmp/_grass_perf_check.scss -I prototype/packages`, 15 warmup, 15 runs each):
    - order A (base, then branch): base 264.7ms ±2.7ms vs branch 264.8ms ±1.8ms — 1.00×.
    - order B (branch, then base): branch 265.0ms ±1.6ms vs base 265.2ms ±2.1ms — 1.00×.
  - Bootstrap v5.0.2 (`/private/tmp/bootstrap-bench/scss/bootstrap.scss`, 5 warmup, 20 runs each):
    - order A (base, then branch): base 52.7ms ±0.6ms vs branch 52.7ms ±0.6ms — 1.00×.
    - order B (branch, then base): branch 52.6ms ±0.5ms vs base 52.7ms ±0.6ms — 1.00×.
  - All four measurements land at 1.00× with the faster side flipping between orderings (split
    direction) — the noise signature the order-swap control is designed to surface, not a
    consistent same-direction cost. Passes the plan's inertness gate.

No STOP condition triggered in the final state. The option is provably inert when off: same code
path (`compile_impl`), same output, same test pass/fail set, same performance (order-swap
confirmed, not just single-direction 1.00× readings that round 1 relied on).

## Deferred slices (not built — design-only per spike scope)

Filing-ready for the reviewer to create as solo todos:

1. **CLI wiring** — read the four already-declared, currently-ignored flags
   (`crates/lib/src/main.rs:118-145`) and thread `Options::source_map`, `--source-map-urls`,
   `--embed-sources`, `--embed-source-map` through to `from_string_with_source_map`; write the
   `.map` file and append the `/*# sourceMappingURL=... */` trailer (or embed per flag). Effort:
   **M** — flag parsing already exists, this is wiring + file I/O + trailer formatting matching
   the observed dart-sass conventions above.
2. **napi surface** — `CompileResult` (`crates/napi/src/lib.rs:19`) needs a `source_map: Option<String>`
   field; `grass_napi`'s compile entry point needs to call `from_string_with_source_map` when
   requested. Effort: **S**.
3. **WASM/npm surface** — `pkg-publish/index.js:109`'s hardcoded `sourceMap: undefined` and
   `index.d.ts:4`'s `sourceMap?: undefined` type need to become real, matching the JS API shape
   observed above (`{version, sourceRoot, sources, names, mappings}`, no `file` key). Effort: **S**,
   once the napi slice lands (WASM path likely mirrors it).
4. **`sourcesContent` / URL-style options** — `--embed-sources` (add `sourcesContent`) and
   `--source-map-urls={relative,absolute}` (rewrite the `sources` array to `file://` URLs).
   Straightforward once file names are tracked; the linear-scan `MappingState::sources` dedup (see
   "Source file list" above) would benefit from becoming a `HashMap` first if this lands. Effort: **S**.
5. **At-rule / comment / media / supports mappings** — extend `record_mapping` calls to the
   remaining `CssStmt` variants in `visit_stmt` using the exact same pattern established for
   `RuleSet`/`Style`. Effort: **S** per variant, **M** total.
6. **UTF-16 column semantics** — `record_mapping`'s column counter currently counts Unicode
   scalar values (skips UTF-8 continuation bytes), which matches byte-for-byte for ASCII CSS (the
   overwhelming common case) but would diverge from the JS-convention UTF-16-code-unit columns
   for non-BMP characters (e.g. emoji in a `content` string or comment). Needs a decision: is
   byte-parity with dart-sass on exotic Unicode worth the extra bookkeeping? Effort: **S** if yes
   (swap the skip-continuation-byte counter for a `char::len_utf16()` sum).
7. **`CodeMap::files()` upstream accessor** — would let `sources` be built by enumerating all
   registered files (matching dart-sass's evident dependency-first ordering more robustly for
   complex `@forward` graphs) instead of first-mapping-seen order, and would drop the `O(files²)`
   dedup scan. Requires either a codemap crate patch/fork or vendoring. Effort: **M** (external
   dependency negotiation).
8. **`names` array** — always empty in every fixture tested; dart-sass likely uses it for
   SassScript-interpolated identifiers echoed into selectors/values. Open question, not
   investigated further — no observed fixture exercised it.

## Open questions

- **stdin source naming:** dart-sass encodes stdin input as a `data:` URL in `sources` rather than
  a literal `"stdin"` string. `from_string_with_source_map` currently uses the literal filename
  passed to `compile_impl` (`"stdin"` for `from_string`-style callers) — matching this exactly
  would mean detecting the synthetic stdin path and swapping in a data-URL-encoded copy of the
  input, deferred to the CLI-wiring slice where the real stdin path is known.
- **`@import`ed-file `sources` URLs:** the two-file `@use` fixture showed dependency-first
  ordering (`_partial.scss` before `main.scss`); this wasn't stress-tested against deeper
  `@forward` chains, multiple load paths, or files sharing a basename in different directories —
  worth a dedicated fixture pass when the CLI-wiring slice lands.
- **compressed-mode edge cases:** confirmed the map shape is identical between `--style=expanded`
  and `--style=compressed` (only `mappings` values differ, matching the different column
  positions), but multi-declaration single-line-per-selector overlap (many mappings on one
  generated line) wasn't exercised beyond the two-mapping-per-line case in the worked example.
- **sass-spec / JS-API test coverage:** searched `sass-spec/spec` for anything source-map-related
  (`grep -rl "sourceMap\|source_map\|sourcemap\|sourceMappingURL"`) — exactly **one** hit,
  `spec/css/comment.hrx`, which tests that a literal `/*# sourceMappingURL=... */` comment in
  *input* CSS is stripped from output (already handled by grass's existing `write_comment`
  /`write_inline_comment`, unrelated to map *generation*). **sass-spec has essentially zero
  coverage of source-map generation itself** — dart-sass's own JS-API test suite (not vendored
  here) is where that behavior is specified and tested. Any future CLI-wiring slice should budget
  time to port relevant cases from dart-sass's `js-api-doc`/`spec` test suites rather than
  expecting sass-spec to catch regressions.

## Judgment calls made this session

- Chose incremental-buffer-scan tracking over instrumenting every write site or introducing a
  buffer wrapper type — see "Output line/column tracking" above. This is the single highest-value
  design decision in this spike; a future implementer should not re-litigate it without first
  re-reading that section.
- Chose to extract `compile_impl` rather than duplicate the parse/visit/serialize driver in a
  sibling function — keeps the option-off path provably identical to pre-spike behavior (same
  code, not parallel copies that could drift), at the cost of a slightly larger diff to
  `from_string_with_file_name`.
- Left `sources` ordering as "first mapping seen" rather than trying to replicate dart-sass's
  exact dependency-graph-order semantics for complex `@forward` chains — the two-file case tested
  matches, deeper cases are an open question (above), not a bug found.
- Round-1 review caught an option-off perf regression that a same-direction-only hyperfine A/B
  missed; round 2 used an order-swap protocol (each workload run in both binary orderings) to
  distinguish a real regression from ambient-load noise, and that protocol is now the standard for
  any future perf claim on this spike — a single-ordering 1.00× reading is not sufficient
  evidence by itself.
