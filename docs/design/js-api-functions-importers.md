# Design spike: napi bridge for `functions` and `importers` (todo #221)

> Paths in this historical document predate the `bench/` reorganization (2026-07-13).

Status: design only, no production code. Written against grass HEAD `1c7174e`,
`sass` npm package `1.97.3`, `napi`/`napi-derive` `2.x` (per
`crates/napi/Cargo.toml`).

Labeling key used throughout: **[probed]** = observed by actually running the
real `sass` npm package in a scratch dir; **[read-from-types]** = read from
the package's shipped `.d.ts` files (not executed); **[inferred]** = my
conclusion from combining the above with grass's source, not directly
observed.

## Plan 070 boundary and pinned source record

This section supersedes the stale future-work wording below for the remaining
parity slice. The target is **Dart Sass 1.97.3**, not the current rolling Sass
website. The exact package artifacts used for the API-shape decisions are the
[`sass@1.97.3` `types/options.d.ts`](https://app.unpkg.com/sass@1.97.3/files/types/options.d.ts),
[`sass@1.97.3` `types/importer.d.ts`](https://app.unpkg.com/sass@1.97.3/files/types/importer.d.ts),
and [`sass@1.97.3` `types/package.json`](https://app.unpkg.com/sass@1.97.3/files/package.json).
The corresponding narrative references are the Sass
[`Options`](https://sass-lang.com/documentation/js-api/interfaces/options/),
[`Importer`](https://sass-lang.com/documentation/js-api/interfaces/importer/),
[`CanonicalizeContext`](https://sass-lang.com/documentation/js-api/interfaces/canonicalizecontext/),
and [`NodePackageImporter`](https://sass-lang.com/documentation/js-api/classes/nodepackageimporter/)
pages.

`StringOptions.importer` and `StringOptions.url` are already shipped by the
native binding (see `crates/napi/index.d.ts` and todo #280). They are not
remaining implementation work here. This plan designs only: (a) load-path
sugar and ordering, (b) Node package importer ownership and `pkg:` behavior,
(c) `nonCanonicalScheme`, and (d) a future-compatible direction for Promise
callbacks. The contract fixtures in
[`crates/napi/test-design/js-api-parity.md`](../../crates/napi/test-design/js-api-parity.md)
are executable-plan fixtures, not feature-completion tests.

## 1. Probed Contracts

### 1.1 `functions`

- Shape: `Options.functions: Record<string, CustomFunction>` where the key is
  a full signature string like `'sum($arg1, $arg2)'` — the same syntax as an
  `@function` declaration's parameter list, including rest args
  (`'name($args...)'`) **[read-from-types]**, confirmed the rest-arg case
  works at runtime **[probed]**.
- The callback receives `args: Value[]` — already positionally *bound*
  against the declared parameter list, not a raw call-site argument blob.
  Verified: calling `my-fn(1px, "hi", (1,2,3), (k: v))` against
  `'my-fn($a, $b, $c, $d)'` handed the callback exactly 4 `Value`s in
  declared-parameter order, typed as `SingleUnitSassNumber` (a `SassNumber`
  subclass), `SassString`, `SassList`, `SassMap` respectively **[probed]**.
- Rest args (`'my-fn($args...)'`) collapse into a single trailing
  `SassArgumentList`. Its `.asList` gives positional values in call order and
  `.keywords` gives an `immutable.OrderedMap` of named args (dollar sign
  stripped from keys) **[probed]** — for `my-fn(1, 2, $k: 3)` we observed
  `positional=[1,2]`, `keywords={k: 3}`.
- Sync vs async: a `CustomFunction<'sync'>` must return synchronously; passing
  an `async` callback (or one returning a `Promise`) to `compileString`
  (sync) throws immediately at call time with
  `Invalid return value for custom function "asyncfn": ...` **[probed]**. The
  same async callback works fine through `compileStringAsync` and its
  `await`ed result is used as the return value **[probed]**. There is no
  partial/streaming mode — it's a full call-and-return per invocation.
- Error propagation: any thrown value (an `Error`, or even a bare string) is
  caught by the compiler and wrapped into a `sass.Exception` whose `.message`
  includes the Sass-side formatted context (source frame, caret, "N:N root
  stylesheet" trace) and whose `.sassMessage` is *just* the thrown message,
  unwrapped. The thrown error carries a real `.span` pointing at the call site
  in the Sass source (`{start,end,url}` with byte offsets) **[probed]**, i.e.
  the JS exception is turned into a proper Sass compile error with span
  attribution, not just an opaque failure.
- `Value` is an abstract base class; concrete instances are constructed via
  its subclasses (`SassNumber`, `SassString`, `SassColor`, `SassList`,
  `SassMap`, `SassBoolean`, `SassArgumentList`, `SassFunction`, `SassMixin`,
  `SassCalculation`/`CalculationOperation`/`CalculationInterpolation`) or the
  singletons `sassNull`, `sassTrue`, `sassFalse` **[read-from-types]**.
  `Value.asList`, `.assertNumber()`/`.assertString()`/etc., and
  `.sassIndexToListIndex()` are the documented way user code should
  destructure arguments — these are pure-JS helper methods on the `Value`
  base class, not part of the wire contract; grass's bridge does not need to
  reimplement them, only construct/read the underlying data **[read-from-types]**.

### 1.2 `importers`

- Two importer shapes, mutually exclusive per entry: `FileImporter` (has
  `findFileUrl`, must NOT have `canonicalize`) and `Importer` (has
  `canonicalize`+`load`, must NOT have `findFileUrl`) — enforced by the
  TypeScript types via `{findFileUrl?: never}` / `{canonicalize?: never}`
  **[read-from-types]**, and there's also a built-in `NodePackageImporter`
  class for `pkg:` URLs that isn't a plain object at all
  **[read-from-types]**.
- `Importer.canonicalize(url, context)` is called with the raw load-rule
  string (e.g. `db:foo/bar`, or a relative string resolved against the
  current file's canonical URL) plus a `CanonicalizeContext` of
  `{fromImport: boolean, containingUrl: URL | null}`. It must return an
  absolute `URL` (any scheme) or `null` to decline **[read-from-types]**,
  confirmed live: for `@use "db:foo/bar"`, `canonicalize` was invoked with
  `url="db:foo/bar"`, `fromImport=false`, `containingUrl=null` (entrypoint has
  no containing file) **[probed]**.
- `Importer.load(canonicalUrl)` is called only for URLs that came back from
  `canonicalize`, and must return `{contents: string, syntax: 'scss'|'sass'|
  'css', sourceMapUrl?: URL}` or `null` **[read-from-types]**; confirmed the
  returned `contents`/`syntax` become the parsed stylesheet body **[probed]**.
- Same canonical URL → same cached module: the compiler treats
  `canonicalize`'s output as a cache/dedup key ("if Sass has already loaded a
  stylesheet with this canonical URL, it re-uses the existing parse tree")
  **[read-from-types]** — this is a *correctness* requirement on the Rust
  side's future importer registry, not just an optimization: two different
  `@use` sites resolving to the same canonical URL must not re-parse or
  re-instantiate the module.
- `FileImporter.findFileUrl(url, context)` returns a `file:` URL (fully or
  partially resolved — the compiler then applies the normal partial/
  extension/index-file resolution on top, exactly like a load path) or
  `null` **[read-from-types]**. Confirmed end-to-end: `findFileUrl` mapping
  `~pkg` → `pathToFileURL('.../pkg')` let `@use "~pkg" as p` resolve and load
  `pkg.scss` from disk with normal extension inference **[probed]**.
- Resolution order across the whole option surface, straight from the pinned
  type doc **[read-from-types]**: (1) for a relative URL only, resolve against
  the loading stylesheet's canonical URL and invoke the importer that loaded
  that stylesheet; (2) for `compileString`, apply the already-shipped
  `StringOptions.url`/`StringOptions.importer` entrypoint rule; (3) each entry
  of `Options.importers` in array order
  (`Importer`/`FileImporter`/`NodePackageImporter` interleave in the order
  given); (4) `Options.loadPaths` entries, each treated as sugar for a
  `FileImporter` that only resolves relative URLs under that path. A decline
  (`null`/`undefined`) advances to the next step; a callback exception stops
  resolution and is reported as a Sass error.
- `StringOptions.importer` (singular) + `StringOptions.url` are shipped
  behavior, not a future design: for `compileString`, they define the
  entrypoint-specific importer/canonical-URL pair that seeds the source's own
  relative loads. If `url` is a `file:` URL and no `importer` is given,
  filesystem loading is the default **[read-from-types]**. If an importer is
  supplied without a URL, the relative rule is passed to that importer as-is;
  this is the 1.97.3 contract and must not be replaced by a synthetic file
  URL.
- Errors thrown from `canonicalize`/`load`/`findFileUrl` are wrapped the same
  way as function errors (message/sassMessage/span on the `@use`/`@import`
  site) **[read-from-types]**, consistent with the function error behavior
  observed above.
- `nonCanonicalScheme?: string | string[]` lets an importer register schemes
  (without `:`) it promises never to return from `canonicalize`. If a load has
  one of those schemes, Sass passes the known `containingUrl`; returning a
  canonical URL with a registered scheme is an error **[read-from-types]**.
  Values must be non-empty and contain only lowercase ASCII letters, digits,
  `+`, `-`, and `.`. This is a validation/context contract, not a new
  resolution source. It remains an unimplemented follow-up and is covered by
  a design fixture rather than claimed runtime support.

## 2. Existing Machinery (grass, read at HEAD `1c7174e`)

### 2.1 Custom functions — `crates/compiler/src/options.rs` /
`crates/compiler/src/builtin/functions/mod.rs`

- `Options::add_custom_fn(name, Builtin)` (options.rs:181, gated behind
  `#[cfg(feature = "custom-builtin-fns")]`) inserts into
  `custom_fns: FxHashMap<String, Builtin>`, keyed by **bare function name
  only** — there is no signature/arity string, unlike the JS API's
  `'sum($arg1, $arg2)'` keys.
- `Builtin` (functions/mod.rs:236) is defined as:
  ```rust
  pub struct Builtin(
      pub(crate) fn(ArgumentResult, &mut Visitor) -> SassResult<Value>,
      usize,
      pub(crate) Option<(&'static str, &'static str)>,
  );
  ```
  Field 0 is a **plain `fn` pointer**, not `Box<dyn Fn>`/`Box<dyn FnMut>`. A
  bare fn pointer cannot capture a closure environment, which is exactly what
  a JS bridge needs (it must capture a `ThreadsafeFunction`/`Ref<JsFunction>`
  per registered custom function). **This is a hard blocker**: the functions
  bridge cannot be built on top of `Builtin` as it exists today. It needs
  either a second `enum` variant/field that holds `Rc<dyn Fn(...)>`, or a
  parallel registration path in `Options`/`Visitor` dispatch
  (`visitor.rs:4571`'s `self.options.custom_fns.get(name.as_str())` lookup)
  that goes through a boxed-closure map instead.
- The callback signature also receives a *raw, unbound* `ArgumentResult`
  (`positional: Vec<Value>` + `named: SmallOrderedMap<Identifier, Value>`,
  ast/args.rs:167) — each builtin does its own `args.get_err(idx, name)`-style
  extraction. This is **not** the same shape as the JS API's pre-bound
  `args: Value[]`. To match JS semantics (fixed params bound by position/name,
  trailing `...` collected into one argument-list value with keywords), the
  bridge needs to parse the registered signature string with the existing
  `ArgumentDeclaration` parser (`parse_argument_declaration`,
  `crates/compiler/src/parse/stylesheet.rs:387`, backing `@function`
  parameter-list parsing) and reuse whatever binding routine already matches
  an `ArgumentInvocation` against an `ArgumentDeclaration` for user-defined
  `@function`s — that binding logic is the natural thing to call before
  invoking the JS callback, rather than re-deriving argument binding from
  scratch. (Not traced to its exact call site in this spike; flagged as an
  implementation-time lookup, not a fresh design.)
- Related, out of scope here: todo **#238** notes the whole
  `custom-builtin-fns` feature isn't forwarded from `grass_compiler` through
  the public `grass` crate yet (default-features mismatch), so even the
  existing Rust-only `add_custom_fn` isn't reachable from a normal `grass`
  dependency today. That's a sibling task; this design assumes it's fixed
  independently, since `crates/napi` depends directly on `grass_compiler`
  (Cargo.toml: `grass_compiler = { path = "../compiler", features =
  ["random"] }`) and could add `"custom-builtin-fns"` itself regardless of
  the `grass` crate's forwarding state.

### 2.2 Import resolution — `crates/compiler/src/fs.rs` /
`crates/compiler/src/evaluate/visitor.rs`

- `Fs` trait (fs.rs) is a real seam already: `&'a dyn Fs` on `Options`, with
  `is_dir`/`is_file`/`read`/`canonicalize`/`resolve_first_existing`/
  `dir_listing`. But it operates purely on filesystem **paths** — no concept
  of a URL, a scheme, non-file "loaded" content, or an `ImporterResult`
  syntax tag. It's the equivalent of *part* of `FileImporter` (existence
  probing + read), not a general `Importer`.
- The actual load path (`Visitor::find_import`/`find_import_uncached`,
  `evaluate/visitor.rs:2144`–`2382`) is deeply **`PathBuf`-shaped**: it builds
  candidate `PathBuf`s (partial-prefix, `.sass`/`.scss`/`.css`, `.import.*`,
  `/index` variants), checks conflicts among them, and falls back through
  `Options.load_paths: Vec<PathBuf>`. There is no canonical-URL concept
  distinct from a resolved path, no scheme dispatch, and no per-load
  "which importer resolved this" bookkeeping (`import_cache` is keyed by
  canonicalized `PathBuf`, `visitor.rs:322`).
- `import_like_node` (visitor.rs:2399) reads bytes straight from
  `self.options.fs.read(&name)` and parses with syntax inferred from the
  path's extension (`InputSyntax::for_path`) — there's no seam for "here are
  the contents, and here's the syntax to parse them with" the way
  `ImporterResult{contents, syntax}` provides.
- **Conclusion**: unlike `functions` (a dispatch-table blocker on `Builtin`'s
  type), `importers` needs a genuinely new resolution path, not a small
  extension of `Fs`. The natural shape is a new trait — call it
  `Importer`/`ImporterHook` — that sits *before* `find_import` in priority
  order (matching the JS resolution order in §1.2) and can return either "not
  mine" (`None`, fall through to `Fs`/load-path resolution) or a resolved
  `(canonical_url: String, contents: String, syntax: InputSyntax)` tuple that
  bypasses `find_import`/`Fs` entirely for that load. A `FileImporter`-style
  variant (return a path/URL only, let existing path-resolution machinery
  handle partials/extensions/index files, then hand off to `Fs::read`) can
  reuse more of the existing machinery and matches how `Options.load_paths`
  already piggybacks on `Fs`. The canonical-URL-as-cache-key requirement
  (§1.2) means the new `import_cache` key must become
  `(source: PathOrUrl)` rather than assuming everything is a `PathBuf`.

### 2.3 napi crate — `crates/napi/src/lib.rs`

- `napi`/`napi-derive` `2.x`, `napi4` feature (Cargo.toml) — `napi4` is the
  feature gate that enables `ThreadsafeFunction`
  **[read-from-types-equivalent, i.e. from napi-rs's own Cargo feature docs,
  not independently executed here]**.
- Current public functions: `compile`/`compile_string` (fully synchronous,
  called directly on the JS thread, `lib.rs:206,226`) and
  `compile_async`/`compile_string_async` (`lib.rs:311,320`), which wrap a
  `CompileTask`/`CompileStringTask` implementing napi's `Task` trait and are
  driven via `AsyncTask::new(...)`.
- **Critical mechanism note on `Task`**: napi's `AsyncTask` runs `Task::compute()`
  on a libuv worker thread with **no JS `Env` access at all** — that's the
  entire point of the trait (keep the JS main thread free while Rust work
  runs elsewhere), and `Task::resolve()` afterwards runs back on the main
  thread with `Env` to build the JS return value. Calling into a JS callback
  *from inside `compute()`* is therefore not a plain function call — it
  requires `ThreadsafeFunction`, which schedules the actual JS invocation
  back onto the main event loop and returns to the calling (non-JS) thread
  asynchronously. To get a *synchronous-looking* round trip (JS function
  called, Rust `compute()` blocks until it has the return `Value`), the
  standard pattern is: `compute()` thread calls `tsfn.call(args,
  ThreadsafeFunctionCallMode::Blocking)` (or `NonBlocking` + a channel), the
  JS-side callback runs on the main loop and sends its result back over a
  `std::sync::mpsc`/oneshot channel, and the `compute()` thread blocks on
  `recv()` for that channel. This does not deadlock as long as the main
  thread is actually free to service its event loop — which it is, since
  `compute()` runs off-thread precisely so the main loop keeps spinning.
- For the **fully synchronous** entry points (`compile`/`compile_string`),
  there is no worker thread at all — Rust code runs directly on the JS main
  thread inside the synchronous native call. A `ThreadsafeFunction` blocking
  call *from the main thread back to the main thread* would deadlock (the
  scheduled callback can never run because the thread that would run it is
  the one blocking). The correct mechanism there is much simpler: hold a
  plain `Ref<JsFunction>` (or equivalent `FunctionRef`) captured from the
  passed-in JS callback and invoke it with a normal, direct, synchronous
  `.call()` — no threadsafe machinery needed, because we're already on the
  right thread. **This means `functions`/`importers` need two different
  Rust-side calling conventions depending on which entry point is used** —
  direct synchronous call for `compile`/`compileString`, and
  `ThreadsafeFunction` + blocking channel round-trip for
  `compileAsync`/`compileStringAsync`. `sass-embedded`'s Node host
  (conceptually, not independently verified here) solves the async side with
  a very similar message-passing dispatch over a channel to the JS side; the
  design here doesn't need a full duplex protocol since we're in-process,
  just the channel round-trip described above.
- No `ThreadsafeFunction`, `Ref`, or JS-callback-holding code exists in the
  napi crate today — this is new territory, not an extension of an existing
  pattern in this codebase.

## 3. Value Marshalling Table

grass's `Value` enum (`crates/compiler/src/value/mod.rs:37`) against the JS
`sass` package's `Value` subclasses:

| grass `Value` variant | JS class | Notes |
|---|---|---|
| `True` / `False` | `sassTrue` / `sassFalse` (`SassBoolean` singletons) | 1:1, no fields to marshal. |
| `Null` | `sassNull` | 1:1 singleton. |
| `Dimension(SassNumber { num, unit, as_slash })` | `SassNumber` (`{value, numeratorUnits, denominatorUnits}`) | grass's `Unit` is a single compound unit type (numerator/denominator implied by its internal representation); JS's constructor takes explicit `numeratorUnits`/`denominatorUnits` string arrays. Needs a conversion helper `Unit ↔ (Vec<String>, Vec<String>)` — grass's own `unit_suggestion()` helper (functions/mod.rs) already does a `numer_and_denom()` split, reusable here. `as_slash` (grass's "this number came from a slash-separated pair, for division-vs-separator disambiguation") has **no JS-side representation** — JS's `SassNumber` has no slash-tracking field. Lossy corner: passing a slash-tagged number out to JS and back will silently drop the tag. |
| `List(Rc<Vec<Value>>, ListSeparator, Brackets)` | `SassList` (`{contents, separator, brackets}`) | Direct 1:1, `ListSeparator`/`Brackets` map onto JS's `separator`/`hasBrackets`. |
| `Color(Rc<Color>)` (`space: ColorSpace`, `channels: [Option<f64>; 3]`, `alpha: Option<f64>`, format hint) | `SassColor` (per-space constructors, `channelsOrNull`/`channels`, missing-channel = `null`) | Very close 1:1 — both model color as (space, 3 channels with explicit "missing" state, alpha). grass's `format: ColorFormat` (`Rgb`/`Hsl`/`Literal(String)`/`Infer` — how to *serialize* the color, e.g. preserve a hex literal) has **no JS equivalent**; JS's `SassColor` has no serialization-hint field. Round-tripping a color through a JS function will lose the "keep original literal text" hint and re-serialize by space/channels like any other color. |
| `String(CompactString, QuoteKind)` | `SassString` (`{text, hasQuotes}`) | Direct 1:1. Watch UTF-16 vs UTF-8/codepoint indexing if any index-based `SassString` methods are ever exposed Rust-side (they're pure-JS helpers per §1.1, so likely N/A for the wire format itself, only matters if grass ever needs to *replicate* `sassIndexToStringIndex`). |
| `Map(SassMap)` (`entries: Rc<Vec<(Spanned<Value>, Value)>>`) | `SassMap` (`OrderedMap<Value, Value>`) | Direct 1:1, both insertion-ordered. grass's `Spanned<Value>` key wrapper carries a `Span` grass uses internally for error messages on duplicate/invalid keys — drop the span when crossing to JS (JS keys are plain `Value`s), and there is no span to reconstruct if a map comes *back* from JS (assign a synthetic/call-site span). |
| `ArgList(ArgList)` (`elems`, `keywords: SmallOrderedMap<Identifier,Value>`, `separator`, `were_keywords_accessed` tracking) | `SassArgumentList` (`SassList` subclass + `keywords: OrderedMap<string,Value>`) | Close 1:1 for `elems`/`keywords`/`separator`. grass's `were_keywords_accessed` (used to drive the "not all keywords were used" warning) has no JS equivalent to read *from*, but constructing a `SassArgumentList` on the Rust side for a JS-bound call would need to decide a value for it (probably: mark accessed, since we don't track anything on the JS return path). |
| `FunctionRef(Box<SassFunction>)` | `SassFunction` (wraps a signature string + a JS callback invoked via `meta.call()`) | Structurally similar (both are "signature + callback"), but a grass-side `SassFunction` and a JS-constructed `new sass.SassFunction(sig, cb)` are callables in *different runtimes*. Passing a JS-authored first-class function into Sass and having grass invoke it via `meta.call()` requires the exact same JS-callback-holding machinery as `functions`/`importers` (a `Ref`/`ThreadsafeFunction` captured inside the `Value`), not just a data copy. This is the least tested lossy corner — flagged as an explicit open question in §7, not designed further here. |
| `MixinRef(Box<SassMixin>)` | `SassMixin` | JS's `SassMixin` cannot be constructed from JS at all (constructor throws) — it only flows *out* of Sass (e.g. as a `meta.get-mixin()` value) into a function that inspects it opaquely. Marshalling only needs one direction (grass → JS, opaque handle), never the reverse. |
| `Calculation(SassCalculation)` | `SassCalculation` + `CalculationOperation`/`CalculationInterpolation` | Structurally 1:1 (`name`+`arguments`, or operator/left/right, or an interpolated string) — both sides explicitly do *not* eagerly simplify calculations (JS docs say so directly; matches grass's separate `Calculation` AST-ish value). Recursive: arguments can nest calculations/operations/interpolations/numbers/strings — the conversion table entry here is recursive over the same tree. |

Overall: the marshalling surface is **structurally very close** — grass's
internal `Value` representation already tracks almost exactly the same
per-variant state as the JS classes (this was presumably shaped by targeting
dart-sass parity in the first place). The lossy corners are all
serialization/diagnostic *hints* that don't affect the CSS a value would
produce (slash-tagging, literal-color-text preservation, keyword-access
tracking, map-key spans) — real semantic data survives the round trip in
every case except first-class `SassFunction`/`SassMixin` callables, which
need the same callback-capturing plumbing as custom functions themselves.

## 4. Functions Bridge Design

### 4.1 Rust-side registration type

`Builtin` (functions/mod.rs:274) needs a variant that can hold a captured
JS callback instead of (or alongside) a bare `fn` pointer. Sketch:

```rust
// crates/compiler/src/builtin/functions/mod.rs
pub enum BuiltinFn {
    Native(fn(ArgumentResult, &mut Visitor) -> SassResult<Value>),
    Dynamic(Rc<dyn Fn(ArgumentResult, &mut Visitor) -> SassResult<Value>>),
}
```

`Options::add_custom_fn` already takes a bare `name: String`; a JS-facing
registration additionally needs the *signature string* (`'sum($a, $b)'`) to
parse into an `ArgumentDeclaration` once at registration time (not per-call),
so the dispatch closure can bind positional/named/rest args into the JS
`Value[]` shape before invoking the callback. This likely means
`add_custom_fn`'s `name: String` argument becomes "parse `name` as a full
signature, split into (bare_name, ArgumentDeclaration)" — a compiler-crate
change, not napi-only, since the bare-`fn`-pointer path (`Options::default()
.add_custom_fn("length", Builtin::new(length))` from the doctest) currently
takes a bare name deliberately (native builtins bind their own args
manually via `ArgumentResult`). Whether native and JS-bridged functions
should share one registration API or two is an open question (§7).

### 4.2 napi-side calling convention (per §2.3's two-mechanism finding)

```rust
// crates/napi/src/lib.rs (sketch, not implemented)
enum JsCallback {
    // compile()/compileString(): direct call, same thread, no channel needed.
    Sync(Ref<JsFunction>),
    // compileAsync()/compileStringAsync(): compute() runs off the JS thread;
    // must hop back via a threadsafe function + blocking channel round trip.
    ThreadsafeBlocking(ThreadsafeFunction<Vec<JsValueBox>, ErrorStrategy::CalleeHandled>),
}

fn call_js_function(cb: &JsCallback, args: Vec<grass_compiler::Value>) -> SassResult<grass_compiler::Value> {
    match cb {
        JsCallback::Sync(func_ref) => {
            // same thread: env.call_function() style direct invocation,
            // marshal args in, marshal the JS return Value back out,
            // catch a thrown JS exception and turn it into a SassResult::Err
            // carrying the call-site span (matches §1.1's span-attribution behavior).
        }
        JsCallback::ThreadsafeBlocking(tsfn) => {
            let (tx, rx) = std::sync::mpsc::channel();
            tsfn.call(
                ThreadsafeFunctionCallArgs { args, respond_to: tx },
                ThreadsafeFunctionCallMode::Blocking,
            );
            rx.recv().expect("JS callback thread dropped sender")
        }
    }
}
```

The `Value` marshalling itself (§3) is identical for both calling
conventions — only how the JS function gets invoked differs. This split
should live entirely in `crates/napi`; the `grass_compiler` side just needs
`BuiltinFn::Dynamic` to hold *some* `Rc<dyn Fn(...)>`, agnostic to what's
inside it.

### 4.3 Error propagation

Matches §1.1 exactly: a thrown JS value (Error or plain value) must be
caught at the `call_js_function` boundary, its message extracted (`.message`
if present, else `.toString()`), and turned into a `SassResult::Err` tagged
with the call-site `Span` already available to `Builtin`'s caller
(`run_function_callable`) — grass's existing `SassResult<Value>`/`Span`
error plumbing already does exactly this for every other builtin error, so
no new error-representation work, just a new *source* of errors (a caught
JS exception) feeding the same pipe.

### 4.4 What `compileAsync` needs beyond sync

- The `ThreadsafeFunction` + channel machinery from §4.2.
- `AsyncTask::compute()` (currently pure Rust, no `Env`) needs to carry the
  `ThreadsafeFunction` handles as part of the task struct so `compute()` can
  reach them without touching `Env` directly (`ThreadsafeFunction` is
  `Send`/`Sync`-safe by design for exactly this reason).
- Nothing else conceptually new — the `Value` marshalling and error handling
  are identical; only the call mechanism changes per §2.3.

## 5. Importers Design

### 5.1 Rust-side hook trait (new, alongside `Fs`)

Per §2.2's conclusion, this is not an `Fs` extension. Sketch of a new trait
that plugs into `find_import`'s priority chain ahead of `Fs`-backed
resolution:

```rust
// crates/compiler/src/importer.rs (new file, sketch)
pub enum ImportResolution {
    /// Fully resolved contents + syntax, bypasses Fs/find_import entirely
    /// (backs a JS `Importer`'s canonicalize+load pair).
    Resolved { canonical_url: String, contents: String, syntax: InputSyntax },
    /// A path/URL for the *existing* candidate-resolution machinery to keep
    /// handling (partials, extensions, index files) — backs a JS
    /// `FileImporter`.
    DelegateToPath(PathBuf),
    /// This importer doesn't recognize the URL; try the next one.
    NotFound,
}

pub trait Importer: std::fmt::Debug {
    fn canonicalize(&self, url: &str, from_import: bool, containing_url: Option<&str>) -> SassResult<ImportResolution>;
}
```

`Options` gains `importers: Vec<Rc<dyn Importer>>` (order-preserving, per
§1.2's array-order resolution rule). `find_import`/`find_import_uncached`
need a new first step: walk `self.options.importers` before falling into the
existing `PathBuf`-candidate logic, and `import_cache`'s key
(`visitor.rs:322`, currently `PathBuf`) needs to become an enum/newtype over
"filesystem path" vs. "importer canonical URL string" so a `db:foo/bar`
canonical URL and a real file path can't collide and both get proper
same-canonical-URL-same-module caching (§1.2's correctness requirement, not
just perf).

### 5.2 JS wrapper (napi side)

Two JS-facing shapes per §1.2 (`Importer` vs `FileImporter`), both reduced to
the same Rust `Importer` trait:

- A JS `FileImporter` (`findFileUrl` only) → Rust wrapper calls
  `findFileUrl` (sync direct-call or threadsafe-blocking, exactly per §4.2's
  split) and wraps a returned URL as `ImportResolution::DelegateToPath`,
  `null` as `NotFound`.
- A JS `Importer` (`canonicalize`+`load`) → Rust wrapper calls `canonicalize`
  first; if it returns a URL, calls `load` with that URL and wraps the
  `{contents, syntax}` result as `ImportResolution::Resolved`; `null` from
  either step is `NotFound`. The `CanonicalizeContext` (`fromImport`,
  `containingUrl`) must be threaded through from `find_import`'s existing
  `for_import: bool` parameter and grass's `current_import_path` tracking
  (`visitor.rs`) respectively — both pieces of data already exist in
  `Visitor`, just need to reach this new call site.
- **Placement decision: `NodePackageImporter` belongs in the Node JS package
  layer, not Rust.** The 1.97.3 class is Node-only and its behavior is the
  standard Node package-resolution algorithm for `pkg:` URLs. The adapter
  should be ordinary JS/TS in `crates/lib/pkg-publish`'s Node entrypoint. It
  exposes the public class/constructor shape and lowers each instance to the
  existing `FileImporter` bridge: `findFileUrl(url, context)` returns an
  absolute `file:` URL (or `null` to decline), after Node resolution applies
  the package `exports` `sass`/`style`/`default` conditions, package-root
  `sass`/`style` fallbacks, and partial/extension/index resolution. No Rust
  compiler trait, `pkg:` scheme, or browser/Wasm/Workers implementation is
  needed. This is a design decision based on the pinned class contract and
  the already-shipped FileImporter bridge **[read-from-types] [inferred]**.

## 5.3 Plan 070 support and ownership boundaries

The compatibility target and the repository's split entrypoints impose these
boundaries:

| Surface | `loadPaths` | custom `importer`/`importers` | `NodePackageImporter` | URL ownership |
|---|---|---|---|---|
| Native Node (`crates/napi`) | Rust compiler, ordered after importers; relative URLs only | JS callbacks through the native bridge; sync callbacks on sync APIs, current async bridge only supports callbacks that return immediately | Node JS package layer, lowered to FileImporter | Rust owns copied canonical URL strings; callback context and canonical URLs are reconstructed per call |
| Pure Wasm browser | Existing Wasm path/FS behavior only | Unsupported until a callback bridge exists; the surface must reject these options clearly, never ignore them | Unsupported; Node resolution is unavailable | Host-provided filesystem paths/URLs only |
| Pure Wasm Workers | Existing Wasm path/FS behavior only | Unsupported until a callback bridge exists; the surface must reject these options clearly, never ignore them | Unsupported; Node resolution is unavailable | Host-provided filesystem paths/URLs only |

The browser and Workers declarations currently omit the native callback
fields, while the Node declaration exposes them. Any future shared option
normalization must retain that distinction: an option that is present but
unsupported on a runtime is an explicit error, not a no-op. `loadPaths` is
only effective where the selected Wasm host can actually resolve/read a path;
the contract fixture records this as a host capability rather than inventing
Node module semantics for Wasm.

For the native bridge, URL lifetime is deliberately value-based. Rust must
copy the canonical URL and containing URL before returning from a callback;
JS must not retain a borrowed pointer or receive a mutable Rust-owned object.
The current grass wire surface uses URL strings, including `containingUrl` and
`canonicalUrl`, while the reference Sass API uses `URL` objects for those
positions. This is an existing, documented wire-level deviation; this plan
does not silently change it while implementing #306–#308. A future URL-object
parity change must be a separately versioned API decision and must update both
sync and async bridges together.

## 5.4 Promise-returning callback direction

The pinned declarations permit `PromiseOr` results for async `Importer`,
`FileImporter`, and `CustomFunction` callbacks and restrict those callbacks
to `compileAsync`/`compileStringAsync` **[read-from-types]**. Grass currently
uses a `ThreadsafeFunction` plus a blocking channel while Rust's async task
computes. That protocol can service a synchronous callback, but it cannot
wait for a JS Promise without holding a worker thread and risking deadlock or
re-entrancy failures. The current clear Promise-return error is therefore a
documented limitation, not an accidental API behavior.

The implementation direction is a separate async-bridge slice:

1. Keep sync entrypoints and sync callbacks direct and rejecting Promise
   results.
2. For async entrypoints, add an awaitable callback protocol that suspends the
   Sass operation until JS settles the Promise; it must not block the worker
   thread on a Promise. The protocol must carry callback identity, copied
   arguments, resolved value, thrown/rejected value, and cancellation/error
   state.
3. Prove concurrent compilations, callback ordering, thrown/rejected errors,
   and nested compilation before changing the public async declarations.
4. Only then widen the generated types to `PromiseOr` and remove the current
   Promise-return guard. Importer and function callbacks should use the same
   protocol so the behavior does not diverge by callback kind.

This work is intentionally not part of #306–#308 and is not implemented by
the design fixtures.

## 6. Phasing

| Slice | Scope | Effort | Depends on |
|---|---|---|---|
| **0. Feature forwarding** (sibling todo #238, not part of this spike) | Forward `custom-builtin-fns` through the `grass` crate so `add_custom_fn` is reachable outside `grass_compiler` directly. | S | — |
| **1. `Builtin` closure variant + signature-based binding** | Add `BuiltinFn::Dynamic(Rc<dyn Fn(...)>)` alongside the existing `fn` pointer variant in `crates/compiler/src/builtin/functions/mod.rs`; extend `add_custom_fn` (or add a sibling method) to accept a signature string, parse it via the existing `ArgumentDeclaration` parser, and bind `ArgumentResult` into the JS-style `Vec<Value>` before calling the closure. Ship with a Rust-only test (a closure captured in a `Box`/`Rc`, no napi yet) proving the signature-parsing + binding + dispatch path end-to-end. This is the prerequisite every other functions-side slice depends on. | M | Slice 0 |
| **2. napi sync functions bridge** | `crates/napi`: accept a `functions: Record<string, JsFunction>`-shaped option on `compile`/`compileString` only (not the `_async` variants yet), wire it to `Options` via slice 1's dynamic `Builtin`, implement the direct-same-thread calling convention (§4.2's `JsCallback::Sync` arm) and the `Value` marshalling table (§3) for every variant except `SassFunction`/`SassMixin` (defer those, they need the callable-capturing plumbing noted as an open question). Error propagation per §4.3. | M | Slice 1 |
| **3. napi async functions bridge** | Extend slice 2 to `compileAsync`/`compileStringAsync`: `ThreadsafeFunction` + blocking-channel round trip (§4.2's `ThreadsafeBlocking` arm), threaded through `CompileTask`/`CompileStringTask`'s `compute()`. | M | Slice 2 |
| **4. Importer trait + `FileImporter` bridge** | New `Importer` trait + `ImportResolution` enum (§5.1) in `grass_compiler`; wire into `find_import`/`find_import_uncached`'s priority chain ahead of path-candidate resolution; fix `import_cache`'s key to distinguish path vs. canonical-URL sources. napi-side: wire only the `FileImporter` (`findFileUrl`) shape first, since it delegates back into existing path machinery and needs no new stylesheet-parsing plumbing. Sync entry points only. | L | Slice 1 (shares the sync/async calling-convention split) |
| **5. Full `Importer` bridge (canonicalize+load) + async** | Wire the `canonicalize`+`load` JS shape (arbitrary schemes, `ImportResolution::Resolved`), `CanonicalizeContext` threading (`fromImport`/`containingUrl`), and extend both importer shapes to the async entry points. | L | Slice 4, Slice 3 |
| **6. `StringOptions.importer`/`url`, `loadPaths`-as-sugar parity, `NodePackageImporter`** | Historical grouping. `StringOptions.importer`/`url` are shipped; the remaining work is split below so load-path verification and Node package resolution have independent ownership. | S–M | Slice 5 |

### Current implementation sequence for #306–#308

1. **#306 — verify load-path sugar and ordering.** Add the design fixture's
   combined case to the eventual N-API runtime test: an ordered custom
   importer declines, a later importer resolves, and a load-path candidate is
   only consulted after every importer declines. Include the entrypoint
   `importer` case so the shipped singular importer remains ahead of the
   array and load paths. This is verify-first and should produce no production
   change if the existing `find_import_uncached` chain matches the contract.
2. **#307 — ship the Node-only package adapter.** Implement and test the
   `NodePackageImporter(entryPointDirectory?)` public helper in the Node
   package layer. Resolve `pkg:` using Node's package entrypoint rules, return
   a copied absolute `file:` URL to the existing FileImporter path, and keep
   the helper out of Rust and non-Node bundles. Test root package, subpath,
   `exports` condition, explicit `entryPointDirectory`, fallback, and
   decline/error behavior. The adapter must not bypass the importer's array
   order or duplicate Sass partial resolution in JS.
3. **#308 — thread the importer hint.** Add the optional
   `nonCanonicalScheme: string | string[]` contract to the full Importer
   shape, validate its grammar, pass `containingUrl` for matching incoming
   schemes, and error if canonicalize returns a URL using a declared
   non-canonical scheme. Keep this out of FileImporter, which always receives
   a known containing URL when one exists. Exercise sync and current async
   callback paths with the same context assertions.
4. **Promise callbacks — separate architecture follow-up.** Design and prove
   the non-blocking awaitable protocol described in §5.4 before changing
   declarations or removing the current guard. Its completion is not a
   prerequisite for the verify-only #306 case, but it is a prerequisite for
   claiming full 1.97.3 async parity.

The dependency graph is therefore `#306 → none` (verification of the shipped
chain), `#307 → existing FileImporter bridge + Node package layer`, and
`#308 → existing full Importer context plumbing`. Promise support depends on
the callback bridge and concurrency/re-entrancy tests, not on Node package
resolution.

**First slice to hand an executor**: **Slice 1** — it's pure `grass_compiler`
work (no napi, no JS, no threading concerns), has a crisp Rust-only test
(construct a `Builtin::Dynamic` from a closure, register it with a
`'name($a, $b, $rest...)'` signature string, call it from a `test!` macro
stylesheet, assert the closure received correctly-bound positional +
rest-collected arguments), and unblocks every downstream slice. It should
NOT touch `crates/napi` at all.

## 7. Open Questions / Risks

1. **First-class `SassFunction`/`SassMixin` round-tripping** (§3's last
   table rows): a JS-constructed `new sass.SassFunction(sig, cb)` passed
   into Sass, or a grass-side first-class function passed out to JS and
   later invoked via `meta.call()`, both need the *same* callback-capturing
   machinery as custom functions/importers, but bidirectionally and
   possibly recursively (a JS custom function could receive a grass-side
   `SassFunction` and call it back via `meta.call()` from Sass, which would
   need to call back into the *original* Rust `Value`, not JS). Not designed
   here; likely its own follow-up spike once slices 1–3 land and the
   calling-convention plumbing exists to build on.
2. **Should native (`fn`-pointer) and JS-bridged (`Rc<dyn Fn>`) custom
   functions share one `add_custom_fn` API or two?** Slice 1 sketches a
   single `BuiltinFn` enum, but the *registration* ergonomics (signature
   string required for one, not really needed for the other since native
   Rust builtins already bind their own args) may want two entry points.
   Needs a decision before slice 1 lands, not after.
3. **`import_cache` key migration risk**: changing `import_cache`'s key from
   plain `PathBuf` (visitor.rs:322) to something that also covers importer
   canonical URLs touches a hot cache on the main import path — needs a perf
   check per this repo's standing pre-commit performance-check convention,
   not just a correctness review.
4. **`ThreadsafeFunction` blocking-call deadlock risk**: the blocking-channel
   round trip (§4.2) assumes the main JS thread is always free to service
   the scheduled callback while `compute()` blocks on `recv()`. This is true
   for the *simple* case (one compile, one thread), but needs verification
   under concurrent `compileAsync` calls sharing a limited libuv threadpool,
   and under re-entrant custom functions (a custom function that itself
   triggers another async Sass compile) — not analyzed in this spike.
5. **Signature-string parsing edge cases**: dart-sass's real `functions`
   option accepts the exact same grammar as `@function` parameter lists
   (defaults, `...`, etc.) — need to confirm grass's `ArgumentDeclaration`
   parser (`parse_argument_declaration`) handles 100% of what real-world JS
   callers pass (e.g. default values like `'foo($a: 1px)'`) before slice 1
   claims parity; not verified against the parser's actual grammar coverage
   in this spike, only confirmed the parser exists and is the right thing to
   reuse.
6. **`NodePackageImporter` is no longer an open placement question.** The
   decision is Node JS package-layer ownership (§5.2): a helper/class lowers
   to the existing FileImporter bridge and is absent from Wasm, browser, and
   Workers bundles. The remaining implementation questions are the exact
   Node package-resolution error/exports cases, which belong in #307's package
   tests rather than Rust importer design.
7. **Perf cost of the new `find_import` pre-check**: even when no JS
   importers are registered, slice 4 adds a new "walk `options.importers`"
   step to every `find_import_uncached` call. Needs to be a zero-cost no-op
   (empty `Vec` check) verified against the historical `prototype/perf-check.sh`, matching
   how `custom_fns`/`GLOBAL_FUNCTIONS` lookups already sit ahead of hot paths
   without regressing USWDS/Bootstrap compiles.
8. **Promise callback architecture**: the pinned target allows Promise
   results on async entrypoints, but the current grass bridge blocks a worker
   thread. The non-blocking suspension protocol, cancellation behavior, and
   nested async compilation semantics remain to be designed and tested before
   this limitation can be removed (§5.4).
