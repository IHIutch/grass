# grass

This crate aims to provide a high level interface for compiling [Sass](https://sass-lang.com/documentation/) into
plain CSS. Its public API centers on four compilation entry points — `from_string`, `from_path`, and the
source-map-returning `from_string_with_source_map`/`from_path_with_source_map` — configured through a builder-style
[`Options`](https://docs.rs/grass/latest/grass/struct.Options.html) (output style, load paths, a pluggable `Fs`/`Logger`,
input syntax, and fine-grained deprecation control), along with supporting types such as `Deprecation`,
`Error`/`ErrorKind`/`Result`, `InputSyntax`, `OutputStyle`, and `SourceMapData`.

In addition to a library, this crate also includes a binary that is intended to act as an invisible
replacement to the Sass commandline executable.

This crate aims to achieve complete feature parity with the `dart-sass` reference
implementation. A deviation from the `dart-sass` implementation can be considered
a bug except for in the case of error messages and error spans.

[Documentation](https://docs.rs/grass/)  
[crates.io](https://crates.io/crates/grass)

## Status

`grass` has reached a stage where one can be quite confident in its output. For the average user there should not be perceptible differences from `dart-sass`.

Every commit of `grass` is tested against bootstrap v5.0.2, and every release is tested against the last 2,500 commits of bootstrap's `main` branch.

That said, there are a number of known missing features and bugs. The rough edges of `grass` largely include `@forward` and more complex uses of `@use`. We support basic usage of these rules, but more advanced features such as `@import`ing modules containing `@forward` with prefixes may not behave as expected.

All known missing features and bugs are tracked in [#19](https://github.com/connorskees/grass/issues/19).

`grass` is not a drop-in replacement for `libsass` and does not intend to be. If you are upgrading to `grass` from `libsass`, you may have to make modifications to your stylesheets, though these changes should not differ from those you would have to make if upgrading to `dart-sass`.

## Performance

`grass` is benchmarked against `dart-sass` and `sassc` (`libsass`) [here](https://github.com/connorskees/sass-perf). In general, `grass` appears to be ~2x faster than `dart-sass` and ~1.7x faster than `sassc`.

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
