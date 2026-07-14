# grass

This crate aims to provide a high level interface for compiling [Sass](https://sass-lang.com/documentation/) into
plain CSS. Its public API centers on four compilation entry points — `from_string`, `from_path`, and the
source-map-returning `from_string_with_source_map`/`from_path_with_source_map` — configured through a builder-style
[`Options`](https://docs.rs/grass/latest/grass/struct.Options.html) (output style, load paths, a pluggable `Fs`/`Logger`,
input syntax, and fine-grained deprecation control), along with supporting types such as `Deprecation`,
`Error`/`ErrorKind`/`Result`, `InputSyntax`, `OutputStyle`, and `SourceMapData`.

In addition to a library, this crate also includes a binary that is intended to act as an invisible
replacement to the Sass commandline executable.

Node users can install [`ihiutch-grass`](https://www.npmjs.com/package/ihiutch-grass) with `npm install ihiutch-grass`.

This crate aims to achieve complete feature parity with the `dart-sass` reference
implementation. A deviation from the `dart-sass` implementation can be considered
a bug except for in the case of error messages and error spans.

[Documentation](https://docs.rs/grass/)  
[crates.io](https://crates.io/crates/grass)

## Performance

Across 14 real-world projects, grass compiles 2.54x–17.17x faster than sass-embedded (6.23x median), byte-identically to dart-sass 1.101.0. See the [full corpus results](bench/real-world/results.md).

| Project | dart-sass (sass-embedded) | grass | speedup |
|---|---:|---:|---:|
| uswds | 3046.1 ms | 177.4 ms | 17.17x |
| video.js | 58 ms | 4.2 ms | 13.81x |
| grafana | 64.8 ms | 5.6 ms | 11.57x |
| just-the-docs | 66.9 ms | 6 ms | 11.15x |
| font-awesome | 71.9 ms | 7.5 ms | 9.59x |
| minimal-mistakes | 95 ms | 12 ms | 7.92x |
| mastodon | 111.4 ms | 15.4 ms | 7.23x |
| govuk-frontend | 107.7 ms | 20.6 ms | 5.23x |
| vuetify | 149.9 ms | 29.3 ms | 5.12x |
| quasar | 156.6 ms | 30.8 ms | 5.08x |
| bootstrap | 203.9 ms | 42.6 ms | 4.79x |
| adminlte | 231.9 ms | 58.8 ms | 3.94x |
| bulma | 501.8 ms | 140.7 ms | 3.57x |
| tabler | 247.3 ms | 97.5 ms | 2.54x |

- Peak memory (Bootstrap, CLI): 187.7 MB → 21.1 MB (8.90x less)
- 8 concurrent Bootstrap compiles (distinct, N=8): 90.2 ms (3.79x vs sequential)
- Real-world parity: 14/14 byte-identical to dart-sass 1.101.0

Measured 2026-07-14 on a 10-core machine with Node 23.4.0. Engine speed (Bootstrap/USWDS and corpus) uses warm in-process medians (2 warmups/5 reps), with no process startup for either engine, same as `bench/real-world/run.mjs`. Peak memory is CLI max RSS from the `sass` binary directly.
Concurrency uses `bench/scripts/napi-concurrent.mjs` for Grass-vs-Grass sequential/concurrent compiles. See [`bench/README.md`](bench/README.md); performance is vs sass-embedded 1.100.0, while byte-parity is vs dart-sass 1.101.0.

## Status

`grass` targets complete feature parity with `dart-sass`; output deviations other than error messages and spans are bugs.
The real-world corpus is byte-identical on 14/14 projects to dart-sass 1.101.0, including USWDS and govuk-frontend;
`@use` and `@forward` are covered. The sass-spec baseline records 39 failures out of 13,888 tests against dart-sass 1.101.0.
CI checks byte-zero output for Bootstrap v5.0.2 and USWDS when the fixture is available. Report bugs in [IHIutch/grass issues](https://github.com/IHIutch/grass/issues).

`grass` is not a drop-in replacement for `libsass` and does not intend to be. If you are upgrading to `grass` from `libsass`, you may have to make modifications to your stylesheets, though these changes should not differ from those you would have to make if upgrading to `dart-sass`.

## Cargo Features

### commandline

(enabled by default): build a binary using clap

### random

(enabled by default): enable the builtin functions [`random([$limit])`](https://sass-lang.com/documentation/modules/math/#random) and [`unique-id()`](https://sass-lang.com/documentation/modules/string/#unique-id)

### stacker

(enabled by default): grow the stack on demand at recursive parsing chokepoints instead of
relying on a small fixed depth limit. Not supported on `wasm32`; WASM builds (which disable
default features) fall back to a lower fixed recursion limit.

### wasm-exports

(disabled by default): expose JavaScript-friendly WebAssembly bindings (`from_string_js`,
`compile_js`, `compile_file_js`) via `wasm-bindgen`.

### macro

(disabled by default): enable the macro `grass::include!` for compiling Sass to
CSS at compile time

### nightly

(disabled by default): currently only used by `grass::include!` to enable 
[proc_macro::tracked_path](https://github.com/rust-lang/rust/issues/99515)

## Source Maps

`grass` can produce [Source Map v3](https://sourcemaps.info/spec.html) mappings alongside its CSS
output, covering declarations, selectors, and comments.

From the CLI, source maps are written by default whenever compiling to an output file (`-o`/positional
output argument); they're skipped when writing to stdout unless requested. Flags:

- `--no-source-map` — disable source map generation
- `--source-map-urls=<relative|absolute>` — how the map links back to source files (default `relative`)
- `--embed-sources` — embed the original source text in the map's `sourcesContent`
- `--embed-source-map` — inline the map as a `data:` URL in the CSS's `sourceMappingURL` comment, instead of writing a sibling `.css.map` file

From the library, call `Options::default().source_map(true)` and use
`from_string_with_source_map`/`from_path_with_source_map` in place of `from_string`/`from_path`; both
return `(String, Option<SourceMapData>)`, where `SourceMapData::to_json(file, embed_sources)` renders
the standard JSON map.

The napi binding (`ihiutch-grass-napi`) and the WASM/npm package (`ihiutch-grass`) expose the same
capability through `CompileOptions.sourceMap`/`sourceMapIncludeSources`, populating
`CompileResult.sourceMap` when requested — mirroring the Sass JS API's `sourceMap`/`sourceMapIncludeSources` options.

## Deprecation Control

`grass` tracks the same set of dart-sass deprecations (18 IDs, e.g. `slash-div`, `import`,
`color-functions`, `if-function`) and lets you silence, fatalize, or opt into them early.

From the CLI:

- `--silence-deprecation <id>` — don't warn for the given deprecation(s); repeatable and/or comma-separated
- `--fatal-deprecation <id|version>` — treat the given deprecation(s) as hard errors; also accepts a
  dart-sass version (e.g. `1.95.0`) to fatalize every deprecation introduced at or before it
- `--future-deprecation <id>` — opt in early to a deprecation that isn't yet on by default

From the library, `Options` exposes `silence_deprecation`, `fatal_deprecation`, and
`future_deprecation`, each taking one `Deprecation` value and chainable per call.

The napi binding mirrors this with `CompileOptions.silenceDeprecations`/`fatalDeprecations`/`futureDeprecations`
(string IDs); `fatalDeprecations` additionally accepts `{major, minor, patch}` version objects for the
same range-fatalization behavior as the CLI's version form.

## Watch Mode

`-w`/`--watch` compiles a single `INPUT` → `OUTPUT` file pair once, then keeps recompiling
whenever the Sass source changes, until stopped with Ctrl-C. It requires a real input path and a
real output file — it's rejected alongside `--stdin` or when printing to stdout.

- After every compile, dependency tracking watches the directory of each file the compile actually
  loaded (via `@use`/`@forward`/`@import`, including variable/mixin/function-only partials that
  never emit CSS), plus every `-I`/`--load-path` directory recursively as a fallback for files that
  might start mattering later. This is still directory-based rather than a precise per-file diff, so
  editing an unrelated `.scss`/`.sass` file that happens to sit in the same directory as a real
  dependency also triggers a recompile — but unrelated directories no longer do. A failed compile
  falls back to watching the entry file's directory recursively until a compile succeeds again.
- `--poll` switches to a polling backend (checking for changes on an interval) instead of native
  filesystem events — useful on filesystems/environments where native watching doesn't fire (e.g.
  some network mounts or containers). Only valid together with `--watch`.
- A failed compile during watch mode prints the error to stderr and updates the output file per
  `--error-css`/`--no-error-css` below, then keeps watching instead of exiting.

## Error CSS

When compiling to an output file (`-o`/positional output argument), a failed compile writes a
synthesized "error CSS" stylesheet to that file by default — a `body::before { content: ... }` rule
that renders the error message when the stylesheet is loaded in a browser, matching `dart-sass`'s
behavior byte-for-byte. Pass `--no-error-css` to delete the output file instead of writing error CSS
on a failed compile. When printing to stdout, a failed compile never writes anything, regardless of
this flag.

## Custom Builtin Functions

Rust functions can be registered as Sass builtins via `Options::add_custom_fn` and `Builtin`,
re-exported from `grass` behind its default-enabled `custom-builtin-fns` feature:

```rust
use grass::{
    sass_value::{ArgumentResult, SassNumber, Value},
    Builtin, Options, Result as SassResult, Visitor,
};

// An example function that looks up the length of an array or map and adds 2 to it
fn length(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let len = args.get_err(0, "list")?.as_list().len();

    Ok(Value::Dimension(SassNumber::new_unitless(len + 2)))
}

fn main() {
    let options = Options::default().add_custom_fn("length", Builtin::new(length));
    let css = grass::from_string("a { color: length([a, b]); }".to_owned(), &options).unwrap();

    assert_eq!(css, "a {\n  color: 4;\n}\n");
}
```

The same types are also available directly from the lower-level `grass_compiler` crate (the crate
that `grass` itself is built on), which is useful if you depend on it directly instead of `grass`.

## Testing

As much as possible this library attempts to follow the same [philosophy for testing as
`rust-analyzer`](https://internals.rust-lang.org/t/experience-report-contributing-to-rust-lang-rust/12012/17).
Namely, all one should have to do is run `cargo test` to run all its tests.
This library maintains a test suite distinct from the `sass-spec`, though it
does include some spec tests verbatim. This has the benefit of allowing tests
to be run without ruby as well as allowing the tests more granular than they
are in the official spec.

Having said that, to run the official test suite,

```bash
# This script expects node >=v14.14.0. Check version with `node --version`
git clone https://github.com/connorskees/grass --recursive
cd grass && cargo b --release
cd sass-spec && npm install
npm run sass-spec -- --impl=dart-sass --command '../target/release/grass'
```

The spec runner does not work on Windows.

Using an internal runner (`run-sass-specs.py`, checked out with the repo) that skips warning-only
fixtures and, for tests expecting an error, checks only that compilation fails rather than diffing
exact message/span text, `grass` achieves the following results against `dart-sass` `1.97.3`:

```
2026-07-07
PASSING: 13762
FAILING: 39
TOTAL: 13801 (99.7%)
```

The remaining failures are largely outdated spec fixtures (verified against `dart-sass` directly,
where `grass`'s actual output matches) plus a small number of tracked edge cases spread across
`@media` query nesting/merging, `@use`/`@forward` ordering, and out-of-gamut color-space conversions.

## Versioning

The minimum supported rust version (MSRV) of `grass` is `1.80.0`. An increase to the MSRV will correspond with a minor version bump. The current MSRV is not a hard minimum, but future bugfix
versions of `grass` are not guaranteed to work on versions prior to this.

`grass` currently targets `dart-sass` version `1.97.3`. An increase to this number will correspond to either a minor or bugfix version bump, depending on the changes.

`grass` (crates.io), the native Node.js binding (`ihiutch-grass-napi` on npm), and the WASM/native npm
package (`ihiutch-grass` on npm) are versioned independently of one another — a release of one does
not imply a matching version bump in the others.
