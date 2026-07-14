# ihiutch-grass

`ihiutch-grass` compiles Sass/SCSS to plain CSS. It is a high-level interface
to `grass`, with native Node performance and a WebAssembly fallback for
portable runtimes. Its output aims for complete feature parity with the
`dart-sass` reference implementation; differences in output other than error
messages and error spans are bugs.

## Quickstart

### Node

The package is ESM. `compileString` is useful for source held in memory, while
`compile` reads a Sass file through Node's filesystem. The async variants have
the same options and result shape:

```js
import {
  compile,
  compileString,
  compileAsync,
  compileStringAsync,
} from "ihiutch-grass";

const fromString = compileString("a { color: red; }");
console.log(fromString.css);

// These file examples assume styles.scss exists in the current directory.
const fromFile = compile("styles.scss");
const fromStringAsync = await compileStringAsync("a { color: blue; }");
const fromFileAsync = await compileAsync("styles.scss");

console.log(fromFile.css, fromStringAsync.css, fromFileAsync.css);
```

Custom functions and importers are available on the Node-native entrypoint.
Return the value classes exported by this package from custom functions:

```js
import { compileString, SassNumber } from "ihiutch-grass";

const result = compileString("a { width: double(5px); }", {
  functions: {
    "double($n)": ([n]) => new SassNumber(n.value * 2, n.numeratorUnits[0]),
  },
});

console.log(result.css);
// a {
//   width: 10px;
// }
```

An importer can provide Sass modules without putting them on disk:

```js
import { compileString } from "ihiutch-grass";

const result = compileString(
  '@use "virtual:colors" as colors; a { color: colors.$primary; }',
  {
    importers: [
      {
        canonicalize(url) {
          return url === "virtual:colors" ? "virtual:colors" : null;
        },
        load(canonicalUrl) {
          return canonicalUrl === "virtual:colors"
            ? { contents: "$primary: rebeccapurple;", syntax: "scss" }
            : null;
        },
      },
    ],
  },
);

console.log(result.css);
```

Full importers may set `nonCanonicalScheme` to a lowercase scheme (or array of
schemes) they promise never to return from `canonicalize`; Sass then supplies
`containingUrl` for loads using those schemes.

Set `sourceMap: true` to receive a source-map object rather than a JSON
string. `sourceMapIncludeSources: true` additionally embeds the source text:

```js
import { compileString } from "ihiutch-grass";

const { sourceMap } = compileString("a { color: red; }", {
  sourceMap: true,
  sourceMapIncludeSources: true,
});

console.log(sourceMap.version); // 3
console.log(sourceMap.sourcesContent);
```

### Browser and bundlers

The browser entrypoint is WASM-only. Initialize it once before compiling; with
no argument it fetches the adjacent `grass_bg.wasm` asset:

```js
import { init, compileString } from "ihiutch-grass";

await init();
const result = compileString("a { color: red; }");
const style = document.createElement("style");
style.textContent = result.css;
document.head.append(style);
```

`compileString` has no filesystem. To compile a path, provide the browser
entrypoint's `options.fs` callbacks. JavaScript functions and importers are
available only through the Node-native entrypoint.

### Cloudflare Workers

Workers require a statically imported, pre-compiled WASM module. Initialize it
once at startup, then compile Sass in a request handler:

```js
import { init, compileString } from 'ihiutch-grass/workers';
import wasmModule from 'ihiutch-grass/grass_bg.wasm';

// Call once at startup
init(wasmModule);

export default {
  async fetch(request) {
    const result = compileString('a { color: red; }');
    return new Response(result.css, {
      headers: { 'Content-Type': 'text/css' },
    });
  },
};
```

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
`sourceMapIncludeSources` option adds `sourcesContent`. These semantics are the
same across the Node WASM, browser, and Workers WASM surfaces. Async methods
preserve the same result and source-map shape as their synchronous counterparts.

The package-wasm CI job builds the dev WASM artifact and runs the full package
correctness suite on pull requests, including Node-WASM, browser, Workers,
persistent-instance, and packed-export checks. The performance command may be
run as an informational follow-up and does not gate correctness. The release
workflow repeats the package suite against the release-profile artifact and
all native bindings before publishing.

## Options and limitations

- `functions` and `importers` work on the four Node-native compile functions.
  `url` and `importer` are available on the two `compileString` functions.
  These options require the native binding; on the Node WASM fallback they
  throw a clear native-binding error instead of being silently ignored.
- `NodePackageImporter` enables Node package `pkg:` URLs. Add
  `new NodePackageImporter()` to `importers` to resolve a package's Sass entrypoint:
  `compileString('@use "pkg:theme" as theme;', {importers: [new NodePackageImporter()]})`.
- `SassNumber`, `SassString`, and `SassList` are exported by the Node entrypoint
  for custom-function return values. They are `undefined` when the Node
  entrypoint is using the WASM fallback, where custom functions and importers
  are unsupported.
- Node-native async compile functions require synchronous custom-function and
  importer callbacks. Promise-returning callbacks are not awaited and produce
  a clear error; use a synchronous callback instead. Browser and Workers APIs
  are WASM-only and do not accept JavaScript functions or importers.
- Source maps are objects with `version: 3`; `sourceMapIncludeSources` adds a
  `sourcesContent` array.

## Status

`grass` targets complete feature parity with `dart-sass`; output deviations other
than error messages and spans are bugs. The real-world corpus is byte-identical
on 14/14 projects to dart-sass 1.101.0, including USWDS and govuk-frontend;
`@use` and `@forward` are covered. The sass-spec baseline records 39 failures
out of 13,888 tests against dart-sass 1.101.0. Report bugs in [IHIutch/grass issues](https://github.com/IHIutch/grass/issues).

`grass` is not a drop-in replacement for `libsass` and does not intend to be.
If you are upgrading to `grass` from `libsass`, you may have to make
modifications to your stylesheets, though these changes should not differ from
those you would have to make if upgrading to `dart-sass`.

## Performance

Across 14 real-world projects, grass compiles 2.5x–18.1x faster than sass-embedded (6.37x median), byte-identically to dart-sass 1.101.0. See the [full corpus results](https://github.com/IHIutch/grass/blob/master/bench/real-world/results.md).

| Project | dart-sass (sass-embedded) | grass | speedup |
|---|---:|---:|---:|
| uswds | 2989.1 ms | 165.1 ms | 18.1x |
| video.js | 61 ms | 4.7 ms | 12.98x |
| grafana | 64.9 ms | 5.5 ms | 11.8x |
| just-the-docs | 71.1 ms | 6.1 ms | 11.66x |
| font-awesome | 71.5 ms | 7.3 ms | 9.79x |
| minimal-mistakes | 96.9 ms | 12.4 ms | 7.81x |
| mastodon | 115.6 ms | 15.4 ms | 7.51x |
| govuk-frontend | 104.5 ms | 20 ms | 5.22x |
| quasar | 149 ms | 28.9 ms | 5.16x |
| vuetify | 140.9 ms | 27.5 ms | 5.12x |
| bootstrap | 192.6 ms | 41.1 ms | 4.69x |
| adminlte | 220.6 ms | 55.7 ms | 3.96x |
| bulma | 486.1 ms | 135.1 ms | 3.6x |
| tabler | 230.5 ms | 92.3 ms | 2.5x |

- Peak memory (Bootstrap, CLI): 66.8 MB → 20.2 MB (3.3x less)
- 8 concurrent Bootstrap compiles (distinct, N=8): 92.4 ms (3.76x vs sequential)
- Real-world parity: 14/14 byte-identical to dart-sass 1.101.0

This package's WASM build (the Node fallback and the browser/Workers entrypoints) runs in browsers, Cloudflare Workers, and other sandboxes where dart-sass's native build cannot run at all — dart-sass's fast path is a native subprocess, and its only option there is the pure-JS `sass` package (283.8 ms warm on Bootstrap). Across the same 14 projects, the WASM build compiles 1.53x–10.04x faster than sass-embedded (3.79x median), byte-identically to dart-sass 1.101.0 on all 14:

| Project | dart-sass (sass-embedded) | grass WASM | speedup |
|---|---:|---:|---:|
| uswds | 3112.6 ms | 310 ms | 10.04x |
| video.js | 63 ms | 6.9 ms | 9.13x |
| grafana | 66.6 ms | 8.2 ms | 8.12x |
| just-the-docs | 70.2 ms | 10.2 ms | 6.88x |
| font-awesome | 77.5 ms | 12.2 ms | 6.35x |
| minimal-mistakes | 97.6 ms | 20.7 ms | 4.71x |
| mastodon | 116.1 ms | 25.5 ms | 4.55x |
| quasar | 162.9 ms | 53.7 ms | 3.03x |
| govuk-frontend | 105.8 ms | 35.7 ms | 2.96x |
| vuetify | 156 ms | 52.9 ms | 2.95x |
| bootstrap | 201.7 ms | 74 ms | 2.73x |
| adminlte | 240.8 ms | 96.8 ms | 2.49x |
| bulma | 526.1 ms | 291.3 ms | 1.81x |
| tabler | 257.2 ms | 168.5 ms | 1.53x |

One-time ~99 ms module init, amortized across compiles; first Bootstrap compile 171.3 ms. The WASM table's dart-sass reference times were re-measured in the same run as the WASM timings, so they differ slightly from the native table above.

Measured 2026-07-14 on a 10-core machine with Node 24.14.0 (LTS). Engine speed (Bootstrap/USWDS and corpus) uses warm in-process medians (2 warmups/5 reps), with no process startup for either engine, same as `bench/real-world/run.mjs`. Peak memory is CLI max RSS from dart-sass's native sass-embedded binary, not the pure-JS npm `sass` CLI.
Concurrency uses `bench/scripts/napi-concurrent.mjs` for Grass-vs-Grass sequential/concurrent compiles. See [`bench/README.md`](https://github.com/IHIutch/grass/blob/master/bench/README.md); performance is vs sass-embedded 1.100.0, while byte-parity is vs dart-sass 1.101.0; WASM is measured through the shipped `pkg-publish` surface with `GRASS_FORCE_WASM=1`, warm in-process, using the same method as the other engine rows.

### Concurrency

`compileAsync` and `compileStringAsync` run on libuv's threadpool, so concurrent compiles can parallelize.

With 8 distinct Bootstrap compiles, `UV_THREADPOOL_SIZE=4` measured 3.76x vs sequential; setting it to 8 measured 6.63x. Set it before the first async work.

Custom JavaScript functions invoked per declaration serialize on the JS thread: fn-heavy measured 1.50x at pool 4 and 1.88x at pool 8. Custom importers do not: Bootstrap made 87 callbacks/compile against 46.8 ms sequential work.

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

## Developing (Rust crate)

This README also serves the Rust crate. The npm package is documented above;
the crate-specific material is retained here for contributors.

### Cargo Features

#### commandline

(enabled by default): build a binary using clap

#### random

(enabled by default): enable the builtin functions [`random([$limit])`](https://sass-lang.com/documentation/modules/math/#random) and [`unique-id()`](https://sass-lang.com/documentation/modules/string/#unique-id)

#### macro

(disabled by default): enable the macro `grass::include!` for compiling Sass to
CSS at compile time

#### nightly

(disabled by default): currently only used by `grass::include!` to enable
[proc_macro::tracked_path](https://github.com/rust-lang/rust/issues/99515)

### Testing

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

`grass` currently targets `dart-sass` version `1.97.3`. An increase to this number will correspond to either a minor or bugfix version, depending on the changes.

For Rust crate documentation, see [docs.rs](https://docs.rs/grass/) and
[crates.io](https://crates.io/crates/grass).
