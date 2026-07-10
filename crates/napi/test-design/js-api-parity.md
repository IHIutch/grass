# JavaScript API parity contract fixtures

Design-only fixtures for Plan 070. These cases describe the behavior that
future implementation and runtime tests must assert. They are intentionally
not imported by `crates/napi/test.mjs`, do not require unimplemented code, and
do not claim feature completion.

The reference shapes below are pinned to Sass 1.97.3's
[`types/options.d.ts`](https://app.unpkg.com/sass@1.97.3/files/types/options.d.ts)
and
[`types/importer.d.ts`](https://app.unpkg.com/sass@1.97.3/files/types/importer.d.ts).
The current native grass wire surface passes URL strings where the reference
API uses `URL` objects; fixtures mark that as an existing wire decision rather
than silently changing it.

## F-306: loadPaths is ordered FileImporter sugar

Reference setup:

```js
const calls = [];
const first = {
  findFileUrl(url) {
    calls.push(["first", url]);
    return null;
  },
};
const second = {
  findFileUrl(url) {
    calls.push(["second", url]);
    return url === "theme" ? new URL("file:///fixture/virtual-theme") : null;
  },
};

const result = compileString('@use "theme" as t; a {color: t.$color}', {
  importers: [first, second],
  loadPaths: ["/fixture/load-path"],
});
```

Required assertions for the eventual runtime test:

1. `first` is called before `second`.
2. A non-null importer result stops resolution; the load path is not called.
3. If both importers return `null`, the load-path candidate resolves the
   stylesheet and its normal partial, extension, and index-file rules apply.
4. A schemed URL such as `pkg:theme` is declined by load-path sugar; it must
   not be converted into a filesystem path by the load-path adapter.
5. With `StringOptions.importer` also present, the entrypoint importer handles
   the source string's own relative load before `Options.importers`, and the
   array still precedes `loadPaths` after it declines.

This fixture verifies the resolution chain, not a new importer implementation.
It should be run against sync and current async entrypoints once the normal
runtime test is extended. Promise-returning callbacks are deliberately not
used here.

## F-307: NodePackageImporter is a Node-side FileImporter adapter

The future Node-package test fixture should create a temporary package tree
with these cases:

```text
entrypoint/
  node_modules/theme/package.json
  node_modules/theme/src/_index.scss
  node_modules/theme/src/_colors.scss
  node_modules/theme/fallback.scss
```

Required assertions:

1. `new NodePackageImporter(entryPointDirectory)` is accepted only by the
   Node entrypoint and can resolve `pkg:theme` through the package's `sass` or
   `style` export/manifest rule.
2. `pkg:theme/colors` resolves a package subpath using Node package entrypoint
   rules, then delegates Sass partial/extension/index handling to the existing
   FileImporter path.
3. `entryPointDirectory` controls the `node_modules` search parent; the
   default is the reference implementation's Node entrypoint-based behavior,
   not the Rust process working directory by accident.
4. `NodePackageImporter` participates in `importers` in the exact array
   position supplied. A preceding importer that resolves `pkg:theme` wins;
   the package adapter is not a privileged compiler path.
5. A missing package, blocked export, or invalid package entry is a surfaced
   Sass/Node error with context; it is not silently treated as `null` unless
   the reference Node resolver explicitly declines that candidate.
6. Browser and Workers bundles do not expose this helper and do not attempt
   Node resolution.

The implementation target is a JS/TS helper in the Node package layer that
returns an absolute copied `file:` URL to the existing FileImporter bridge.
There is no `pkg:` Rust trait or compiler special case in this fixture.

## F-308: nonCanonicalScheme affects context and validation

Reference importer shape:

```js
const importer = {
  nonCanonicalScheme: ["db", "db+cache"],
  canonicalize(url, context) {
    // For an incoming db: URL, containingUrl is supplied when known.
    return url === "db:colors" ? new URL("db:colors") : null;
  },
  load(canonicalUrl) {
    return {contents: "$color: red;", syntax: "scss"};
  },
};
```

Required assertions for the eventual runtime test:

1. `nonCanonicalScheme` accepts one string or an array of strings, without
   `:`. Empty strings, uppercase letters, and characters outside lowercase
   ASCII letters/digits/`+`/`-`/`.` are rejected at option validation time.
2. For a schemed incoming URL whose scheme is declared, `canonicalize` gets
   the known `containingUrl`; an undeclared scheme does not gain that context
   merely because it has a scheme.
3. If `canonicalize` returns a URL using one of its declared
   `nonCanonicalScheme` values, compilation errors rather than accepting an
   invalid canonical URL.
4. `fromImport` remains independent of this hint and is true only for
   Sass `@import`, not for `@use` or `@forward`.
5. The same context behavior is required on sync and current callback-based
   async entrypoints. A Promise-returning callback is a separate unsupported
   case until F-ASYNC is implemented.

## F-ASYNC: Promise direction (not yet an implementation contract)

The pinned declarations permit Promise results for async-only importer and
function callbacks. The current grass bridge must continue to reject such
callbacks clearly until the bridge can suspend without blocking a worker
thread. Before that guard is removed, a future contract test must cover:

- one async callback resolving a Sass value/importer result;
- a rejected Promise and a thrown synchronous error preserving Sass context;
- concurrent compilations with independent callback state;
- callback ordering when multiple imports are attempted; and
- nested compilation from a callback, including awaited nested compilation,
  without deadlock.

The test must also assert that sync entrypoints reject Promise callbacks. The
desired post-implementation contract is the Sass 1.97.3 `PromiseOr` split,
but these cases are intentionally placeholders rather than passing tests.

## Runtime matrix fixture

| Fixture | Native Node | Browser Wasm | Workers Wasm |
|---|---|---|---|
| F-306 loadPaths | supported where paths are readable | host FS/path capability only | host FS/path capability only |
| F-307 NodePackageImporter | Node package layer only | unavailable and rejected/not exported | unavailable and rejected/not exported |
| F-308 nonCanonicalScheme | native full Importer follow-up | unavailable with callback bridge | unavailable with callback bridge |
| F-ASYNC Promise callbacks | current guard until bridge redesign | unavailable with callback bridge | unavailable with callback bridge |

No row permits an option to be silently ignored. A runtime that cannot honor a
field must omit it from its public type or fail explicitly during option
validation.
