# grass

This crate aims to provide a high level interface for compiling [Sass](https://sass-lang.com/documentation/) into
plain CSS.

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

### WASM package measurements

The package's WASM performance is measured separately from correctness. Run
`npm run perf` after the generated WASM files have been copied into this
directory. The command forces the package's WASM path, warms each workload,
then reports per-sample wall time, output size, RSS, heap usage, Node version,
Rust/wasm-pack versions when available, and the WASM artifact hash. It covers
both a representative string compile and a file compile with imports.

The banked USWDS release-profile measurement is approximately **300 ms** and
**1.5x native** after filesystem-boundary batching. Treat a result as a
regression only when it uses the same USWDS fixture, host class, Node version,
Rust/wasm-pack toolchain, and WASM build profile: flag a median above 375 ms
(25% over the ~300 ms banked baseline), or a same-run WASM/native ratio above
1.875x. Cross-machine, dev-vs-release, and browser-vs-Node comparisons are
not regression verdicts. The benchmark is informational and is never part of
`npm test`.

## Supported runtimes and package contract

The package has one Node entrypoint and two explicit WASM entrypoints. The
default export uses Node's `node` condition for Node consumers and the browser
WASM condition for browser/bundler consumers. Workers use the explicit
`/workers` export.

| Runtime / entrypoint | Status | Initialization | Filesystem and imports | Async semantics |
| --- | --- | --- | --- | --- |
| Node + matching native binding (`ihiutch-grass`) | Supported | Automatic | Node filesystem; `compile()` and imports supported | Native async binding is used when available |
| Node + WASM fallback (`GRASS_FORCE_WASM=1`) | Supported | Automatic from bundled `grass_bg.wasm` | Node filesystem; core Sass imports supported; JS functions/importers are unsupported and throw | Promise API schedules the synchronous WASM compile on a microtask; it does not move CPU work off the event loop |
| Browser/bundler (`ihiutch-grass`) | Supported | `await init()`; pass bytes/module or let the entrypoint fetch the adjacent WASM asset | `compileString()` has no filesystem; `compile()` requires caller-provided `options.fs` callbacks | Promise API schedules the synchronous WASM compile on a microtask |
| Cloudflare Workers (`ihiutch-grass/workers`) | Supported | `init(wasmModule)` with a statically imported `WebAssembly.Module` | `compileString()` is the primary API; no platform filesystem; caller-provided `options.fs` is allowed | Promise API schedules the synchronous WASM compile on a microtask |

The capability contract is:

| Capability | Node native | Node WASM | Browser | Workers |
| --- | --- | --- | --- | --- |
| `compileString` / `style` / `quiet` / `loadPaths` | Supported | Supported | Supported | Supported |
| `compile()` from a path | Supported | Supported | Supported with `options.fs` | Unsupported by this entrypoint |
| `compileAsync` / `compileStringAsync` | Supported | Supported | Supported | `compileStringAsync` supported; `compileAsync` unavailable |
| Source maps (`sourceMap`, `sourceMapIncludeSources`) | Supported | Supported | Supported | Supported |
| JS `functions`, `importers`, `url`, and `importer` options | Supported | Unsupported; throws a native-binding error | Unsupported; omitted from types | Unsupported; omitted from types |

Initialization failures are intentional contract errors: browser APIs say
`WASM not initialized. Call \`await init()\``, and Workers says
`WASM not initialized. Call init(wasmModule)`. The Node entrypoint initializes
automatically and reports a normal module/file error if its generated WASM
artifact is absent.

Filesystem callbacks use `is_file`, `is_dir`, `read`, and `canonicalize`;
Node's wrapper also supplies the optional batched `readdirSync` callback.
Browser and Workers results have `loadedUrls: []` because those entrypoints
do not assign filesystem URLs. Node path compilation reports the input as a
file URL in `loadedUrls`.

Source maps are omitted unless `sourceMap: true`; when enabled they are
returned as a source-map object (version 3), not a JSON string. The
`sourceMapIncludeSources` option adds `sourcesContent`. These semantics are
the same across the Node WASM, browser, and Workers WASM surfaces. Async
methods preserve the same result and source-map shape as their synchronous
counterparts.

The package-wasm CI job builds the dev WASM artifact and runs the full package
correctness suite on pull requests, including Node-WASM, browser, Workers,
persistent-instance, and packed-export checks. The performance command may be
run as an informational follow-up and does not gate correctness. The release
workflow repeats the package suite against the release-profile artifact and
all native bindings before publishing.

## Cargo Features

### commandline

(enabled by default): build a binary using clap

### random

(enabled by default): enable the builtin functions [`random([$limit])`](https://sass-lang.com/documentation/modules/math/#random) and [`unique-id()`](https://sass-lang.com/documentation/modules/string/#unique-id)

### macro

(disabled by default): enable the macro `grass::include!` for compiling Sass to
CSS at compile time

### nightly

(disabled by default): currently only used by `grass::include!` to enable
[proc_macro::tracked_path](https://github.com/rust-lang/rust/issues/99515)

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

## Versioning

The minimum supported rust version (MSRV) of `grass` is `1.80.0`. An increase to the MSRV will correspond with a minor version bump. The current MSRV is not a hard minimum, but future bugfix
versions of `grass` are not guaranteed to work on versions prior to this.

`grass` currently targets `dart-sass` version `1.97.3`. An increase to this number will correspond to either a minor or bugfix version bump, depending on the changes.
