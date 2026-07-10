import { createRequire } from "module";
import assert from "assert";
const require = createRequire(import.meta.url);

// napi build --platform names the file with a platform suffix; find it.
import { readdirSync, writeFileSync, rmSync, mkdirSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { pathToFileURL } from "url";
const nodeFile = readdirSync(new URL(".", import.meta.url)).find((f) => f.endsWith(".node"));
assert(nodeFile, "no .node binary found — run `npx napi build --platform --release` first");
const binding = require(`./${nodeFile}`);

// Export surface
for (const name of ["compile", "compileString", "compileAsync", "compileStringAsync"]) {
  assert.equal(typeof binding[name], "function", `missing export: ${name}`);
}

// Sync happy path + option mapping
assert.equal(binding.compileString("a { b: c }", null).css, "a {\n  b: c;\n}\n");
assert.equal(binding.compileString("a { b: c }", { style: "compressed" }).css, "a{b:c}");

// Sync error path throws (does not abort)
assert.throws(() => binding.compileString("a { b: ", null));

// Async happy + error paths
const r = await binding.compileStringAsync("a { b: c }", null);
assert.equal(r.css, "a {\n  b: c;\n}\n");
await assert.rejects(binding.compileStringAsync("a { b: ", null));

// silenceDeprecations / fatalDeprecations option mapping
const slashDivSource = "$a: 1;\nb { c: $a/2; }";
assert.equal(
  binding.compileString(slashDivSource, { silenceDeprecations: ["slash-div"] }).css,
  "b {\n  c: 0.5;\n}\n",
);
assert.throws(() =>
  binding.compileString(slashDivSource, { fatalDeprecations: ["slash-div"] }),
);

// Unknown deprecation IDs warn-and-continue (matches the real `sass` JS API:
// prints `WARNING: Invalid deprecation "…".` to stderr, does not throw) —
// verified against `sass@1.97.3`'s `compileString`, unlike the CLI, which
// hard-errors on the same input.
assert.equal(
  binding.compileString("a { b: c }", { silenceDeprecations: ["bogus-id"] }).css,
  "a {\n  b: c;\n}\n",
);
assert.equal(
  binding.compileString("a { b: c }", { fatalDeprecations: ["bogus-id"] }).css,
  "a {\n  b: c;\n}\n",
);
assert.equal(
  binding.compileString("a { b: c }", { futureDeprecations: ["bogus-id"] }).css,
  "a {\n  b: c;\n}\n",
);
// A bogus ID mixed with a real one: the real one still takes effect.
assert.throws(() =>
  binding.compileString(slashDivSource, {
    fatalDeprecations: ["slash-div", "bogus-id"],
  }),
);

// fatalDeprecations accepts {major, minor, patch} version objects, expanding
// to every deprecation introduced at or before that version — verified
// against the real `sass` npm package (1.97.3): `if-function` was
// introduced in exactly Dart Sass 1.95.0, so `Version(1, 95, 0)` fatalizes
// it but `Version(1, 94, 9)` does not.
const ifFunctionSource = "a { b: if(true, 1, 2) }";
assert.equal(
  binding.compileString(ifFunctionSource, {
    fatalDeprecations: [{ major: 1, minor: 94, patch: 9 }],
  }).css,
  "a {\n  b: 1;\n}\n",
);
assert.throws(() =>
  binding.compileString(ifFunctionSource, {
    fatalDeprecations: [{ major: 1, minor: 95, patch: 0 }],
  }),
);

// A string ID and a version object combine in the same array.
assert.throws(() =>
  binding.compileString(slashDivSource, {
    fatalDeprecations: ["bogus-id", { major: 1, minor: 95, patch: 0 }],
  }),
);

// sourceMap option (todo #162): absent by default, matching the real `sass`
// npm package's `compileString(..., {})` result having no `sourceMap` key.
{
  const plain = binding.compileString("a {\n  b: c;\n}\n", null);
  assert.equal(plain.sourceMap, undefined);
}

// sourceMap: true returns a real object (not a JSON string), shaped like the
// real Sass JS API's result: {version, sourceRoot, sources, names,
// mappings}, sources[0] a data: URL (no url option given), no `file` key.
{
  const r = binding.compileString("a {\n  b: c;\n}\n", { sourceMap: true });
  assert.equal(typeof r.sourceMap, "object");
  assert.equal(r.sourceMap.version, 3);
  assert.equal(r.sourceMap.sourceRoot, "");
  assert.deepEqual(r.sourceMap.names, []);
  assert.equal(r.sourceMap.mappings, "AAAA;EACE");
  assert.equal(r.sourceMap.file, undefined);
  assert.equal(r.sourceMap.sources.length, 1);
  assert(r.sourceMap.sources[0].startsWith("data:;charset=utf-8,"));
  assert.equal(r.sourceMap.sourcesContent, undefined);
}

// sourceMapIncludeSources adds sourcesContent.
{
  const r = binding.compileString("a {\n  b: c;\n}\n", {
    sourceMap: true,
    sourceMapIncludeSources: true,
  });
  assert.deepEqual(r.sourceMap.sourcesContent, ["a {\n  b: c;\n}\n"]);
}

// compileStringAsync mirrors the sync API for sourceMap.
{
  const r = await binding.compileStringAsync("a {\n  b: c;\n}\n", { sourceMap: true });
  assert.equal(r.sourceMap.mappings, "AAAA;EACE");
}

// compile() against a real file on disk.
{
  const path = join(tmpdir(), `grass-napi-test-${process.pid}.scss`);
  writeFileSync(path, "a { b: c }");
  try {
    assert.equal(binding.compile(path, null).css, "a {\n  b: c;\n}\n");
  } finally {
    rmSync(path);
  }
}

// compile() on a missing path rejects with an error rather than throwing a
// native panic/crash.
{
  const path = join(tmpdir(), `grass-napi-test-missing-${process.pid}.scss`);
  assert.throws(() => binding.compile(path, null));
}

// Same-ID fatalDeprecations + silenceDeprecations precedence: fatal wins —
// verified against the real `sass` npm package (1.97.3):
// `compileString(src, {fatalDeprecations: ["slash-div"],
// silenceDeprecations: ["slash-div"]})` still throws.
assert.throws(() =>
  binding.compileString(slashDivSource, {
    fatalDeprecations: ["slash-div"],
    silenceDeprecations: ["slash-div"],
  }),
);

// --- `functions` option (todo #221 slice 2) ---------------------------

// Numeric custom function: unit is preserved through the round trip.
{
  const opts = {
    functions: {
      "double($n)": (args) => {
        const n = args[0];
        assert(n instanceof binding.SassNumber, "arg should be a SassNumber instance");
        return new binding.SassNumber(n.value * 2, {
          numeratorUnits: n.numeratorUnits,
          denominatorUnits: n.denominatorUnits,
        });
      },
    },
  };
  const res = binding.compileString("a { b: double(5px); }", opts);
  assert.equal(res.css, "a {\n  b: 10px;\n}\n");
}

// String custom function: quoted/unquoted round-trips.
{
  const opts = {
    functions: {
      "shout($s)": (args) => {
        const s = args[0];
        assert(s instanceof binding.SassString, "arg should be a SassString instance");
        return new binding.SassString(`${s.text.toUpperCase()}!`, s.hasQuotes);
      },
    },
  };
  const res = binding.compileString('a { b: shout("hi"); c: shout(hi); }', opts);
  assert.equal(res.css, 'a {\n  b: "HI!";\n  c: HI!;\n}\n');
}

// List custom function: in/out, separator preserved.
{
  const opts = {
    functions: {
      "double-list($list)": (args) => {
        const list = args[0];
        assert(list instanceof binding.SassList, "arg should be a SassList instance");
        const doubled = list.contents.map((v) => new binding.SassNumber(v.value * 2));
        return new binding.SassList(doubled, list.separator, list.brackets);
      },
    },
  };
  const res = binding.compileString("a { b: double-list(1 2 3); }", opts);
  assert.equal(res.css, "a {\n  b: 2 4 6;\n}\n");
}

// null/bool: mapped to plain JS null/true/false (grass-specific simplification,
// see values.rs's marshalling table doc comment).
{
  const opts = {
    functions: {
      "negate($b)": (args) => {
        assert.equal(typeof args[0], "boolean");
        return !args[0];
      },
      "or-default($x, $fallback)": (args) => (args[0] === null ? args[1] : args[0]),
    },
  };
  const res = binding.compileString(
    "a { b: negate(true); c: negate(false); d: or-default(null, 5); e: or-default(9, 5); }",
    opts,
  );
  assert.equal(res.css, "a {\n  b: false;\n  c: true;\n  d: 5;\n  e: 9;\n}\n");
}

// Rest-args signature: `$args...` collapses into a single SassList-shaped
// argument (positional elements only — keywords are dropped, documented
// divergence from the real `SassArgumentList`/`.keywords` API).
{
  const opts = {
    functions: {
      "sum-all($nums...)": (args) => {
        const rest = args[0];
        assert(rest instanceof binding.SassList, "rest arg should be SassList-shaped");
        const total = rest.contents.reduce((acc, v) => acc + v.value, 0);
        return new binding.SassNumber(total);
      },
    },
  };
  const res = binding.compileString("a { b: sum-all(1, 2, 3); }", opts);
  assert.equal(res.css, "a {\n  b: 6;\n}\n");
}

// A bare JS Array is also accepted as an ergonomic convenience (becomes a
// comma-separated, non-bracketed list) — real API requires an explicit
// SassList, this is a grass-specific relaxation.
{
  const opts = {
    functions: {
      "make-list()": () => [new binding.SassNumber(1), new binding.SassNumber(2)],
    },
  };
  const res = binding.compileString("a { b: make-list(); }", opts);
  assert.equal(res.css, "a {\n  b: 1, 2;\n}\n");
}

// A thrown JS exception surfaces as a clean compile error (not a crash).
{
  const opts = {
    functions: {
      "boom()": () => {
        throw new Error("kaboom");
      },
    },
  };
  assert.throws(() => binding.compileString("a { b: boom(); }", opts), /kaboom/);
}

// An unsupported argument type (SassColor isn't supported in slice 2)
// produces a clear error naming the type, not a crash or silent misbehavior.
{
  const opts = {
    functions: {
      "identity($x)": (args) => args[0],
    },
  };
  assert.throws(() => binding.compileString("a { b: identity(red); }", opts), /SassColor/);
}

// An unsupported return value shape (a plain object, not one of the
// recognized shapes) also produces a clear error rather than a crash.
{
  const opts = {
    functions: {
      "bad-return()": () => ({ foo: "bar" }),
    },
  };
  assert.throws(() => binding.compileString("a { b: bad-return(); }", opts));
}

// --- `functions` + async entry points (todo #221 slice 3) -------------

// Small helper: race a promise against a timeout so a genuine deadlock in
// the code under test reports as a clear failure line instead of a hung
// `node test.mjs` process (per the slice 3 brief's probe requirements).
function withTimeout(promise, ms, label) {
  return Promise.race([
    promise,
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error(`TIMEOUT (${ms}ms): ${label}`)), ms),
    ),
  ]);
}

// 1. BASELINE: a single compileStringAsync (await) with a sync JS function
// doing unit-preserving numeric work.
{
  const opts = {
    functions: {
      "double($n)": (args) => {
        const n = args[0];
        assert(n instanceof binding.SassNumber);
        return new binding.SassNumber(n.value * 2, {
          numeratorUnits: n.numeratorUnits,
          denominatorUnits: n.denominatorUnits,
        });
      },
    },
  };
  const r = await withTimeout(
    binding.compileStringAsync("a { b: double(21px); }", opts),
    10000,
    "baseline async fn",
  );
  assert.equal(r.css, "a {\n  b: 42px;\n}\n");
}

// A thrown JS exception under compileAsync surfaces as a clean rejection
// (not a hang, not a process abort).
{
  const opts = { functions: { "boom()": () => { throw new Error("kaboom"); } } };
  await assert.rejects(
    withTimeout(binding.compileStringAsync("a { b: boom(); }", opts), 10000, "throwing async fn"),
    /kaboom/,
  );
}

// 2. CONCURRENCY: Promise.all of N=8 and N=16 concurrent compileStringAsync
// calls (exceeding the 4-thread default libuv pool), each using a JS
// function — all must resolve correctly. Also re-run once with
// UV_THREADPOOL_SIZE=1 externally (see the slice 3 report) to stress worker
// starvation — that variant isn't run automatically here since the env var
// must be set before the process starts.
async function probeConcurrency(n) {
  const opts = { functions: { "id($n)": (args) => args[0] } };
  const start = Date.now();
  const promises = [];
  for (let i = 0; i < n; i++) {
    promises.push(binding.compileStringAsync(`a { b: id(${i}px); }`, opts));
  }
  const results = await withTimeout(Promise.all(promises), 20000, `concurrency N=${n}`);
  const elapsed = Date.now() - start;
  results.forEach((r, i) => assert.equal(r.css, `a {\n  b: ${i}px;\n}\n`));
  console.log(`  concurrency N=${n}: PASS, elapsed=${elapsed}ms`);
}
await probeConcurrency(8);
await probeConcurrency(16);

// 3. RE-ENTRANCY.
// 3a. A custom function whose JS body itself calls compileString (SYNC) —
// expected safe: it's a plain nested synchronous call on the same thread.
{
  const opts = {
    functions: {
      "nested-sync()": () => {
        const inner = binding.compileString("x { y: z }", null);
        return new binding.SassString(inner.css.trim(), false);
      },
    },
  };
  const r = await withTimeout(
    binding.compileStringAsync("a { b: nested-sync(); }", opts),
    10000,
    "sync re-entrancy",
  );
  assert.equal(r.css, "a {\n  b: x { y: z; };\n}\n");
  console.log("  re-entrancy (sync nested compile): PASS");
}

// 3b. A custom function that calls compileStringAsync WITHOUT awaiting it
// (the only shape a non-async custom function can use, since the custom
// function itself must return synchronously) — must not deadlock; the
// inner compile keeps running independently after the outer one returns.
{
  let innerSettled = false;
  const opts = {
    functions: {
      "nested-async-fireforget()": () => {
        binding
          .compileStringAsync("x { y: z }", null)
          .then(() => { innerSettled = true; })
          .catch(() => { innerSettled = true; });
        return new binding.SassString("started", false);
      },
    },
  };
  const r = await withTimeout(
    binding.compileStringAsync("a { b: nested-async-fireforget(); }", opts),
    10000,
    "async re-entrancy (fire-and-forget)",
  );
  assert.equal(r.css, "a {\n  b: started;\n}\n");
  await new Promise((res) => setTimeout(res, 200));
  assert.equal(innerSettled, true, "inner fire-and-forget compile should settle shortly after");
  console.log("  re-entrancy (async nested compile, fire-and-forget): PASS");
}

// 3c. An ASYNC custom function that itself `await`s a nested
// compileStringAsync — the prime deadlock candidate. Calling an `async`
// function always returns a Promise synchronously (before any internal
// `await` resumes), so this hits the Promise-return guard (case 4) before
// ever reaching a wait on the nested compile — guarded, not hung.
{
  const opts = {
    functions: {
      "nested-async-awaited()": async () => {
        const inner = await binding.compileStringAsync("x { y: z }", null);
        return new binding.SassString(inner.css.trim(), false);
      },
    },
  };
  await assert.rejects(
    withTimeout(
      binding.compileStringAsync("a { b: nested-async-awaited(); }", opts),
      10000,
      "async re-entrancy (awaited) — must be guarded, not hang",
    ),
    /async custom functions returning a Promise are not yet supported/,
  );
  console.log("  re-entrancy (async nested compile, awaited): GUARDED (Promise-return error), no hang");
}

// 4. ASYNC JS FUNCTION returning a Promise directly (not via re-entrancy) —
// must produce a clear, specific error, never hang and never mis-marshal
// the Promise object as if it were a plain Sass value.
{
  const opts = { functions: { "async-fn()": async () => new binding.SassNumber(1) } };
  await assert.rejects(
    withTimeout(binding.compileStringAsync("a { b: async-fn(); }", opts), 10000, "async fn returning Promise"),
    /async custom functions returning a Promise are not yet supported in compileAsync; use a synchronous function/,
  );
  console.log("  async-function-returns-Promise: GUARDED with clear error");
}
{
  // A plain (non-async) function that manually returns a `new Promise(...)`
  // — same guard, doesn't require the function to be declared `async`.
  const opts = {
    functions: { "manual-promise()": () => new Promise((resolve) => resolve(new binding.SassNumber(1))) },
  };
  await assert.rejects(
    withTimeout(binding.compileStringAsync("a { b: manual-promise(); }", opts), 10000, "manual Promise return"),
    /async custom functions returning a Promise are not yet supported/,
  );
  console.log("  manual-Promise-return: GUARDED with clear error");
}

// compile()/compileString() (sync) still work unchanged with `functions`
// alongside the async entries above, confirming no cross-contamination
// between the sync (Ref<JsFunction>) and async (ThreadsafeFunction) bridges.
{
  const opts = { functions: { "double($n)": (args) => new binding.SassNumber(args[0].value * 2) } };
  assert.equal(binding.compileString("a { b: double(3); }", opts).css, "a {\n  b: 6;\n}\n");
}

// --- `importers` option (todo #221 slice 4, FileImporter only) --------

// A FileImporter redirects a virtual URL to a real file on disk, returned
// as a `file:` URL — mirrors the real Sass JS API's FileImporter contract
// (findFileUrl(url, context) returns a file: URL, or null to decline).
{
  const path = join(tmpdir(), `grass-napi-importer-redirect-${process.pid}.scss`);
  writeFileSync(path, "$a: red;");
  try {
    const opts = {
      importers: [
        {
          findFileUrl(url, context) {
            assert.equal(typeof context.fromImport, "boolean");
            assert(context.containingUrl === null || typeof context.containingUrl === "string");
            if (url === "virtual:thing") {
              return pathToFileURL(path).href;
            }
            return null;
          },
        },
      ],
    };
    const res = binding.compileString('@import "virtual:thing";\na { b: $a; }', opts);
    assert.equal(res.css, "a {\n  b: red;\n}\n");
  } finally {
    rmSync(path);
  }
}

// A FileImporter path containing a space is percent-encoded by
// pathToFileURL and must be decoded by the native importer bridge.
{
  const path = join(tmpdir(), `grass-napi-importer-space ${process.pid}.scss`);
  writeFileSync(path, "$a: purple;");
  try {
    const opts = {
      importers: [
        {
          findFileUrl(url) {
            if (url === "virtual:space") return pathToFileURL(path).href;
            return null;
          },
        },
      ],
    };
    const res = binding.compileString('@import "virtual:space";\na { b: $a; }', opts);
    assert.equal(res.css, "a {\n  b: purple;\n}\n");
  } finally {
    rmSync(path);
  }
}

// findFileUrl returning null declines and falls through to the default
// (loadPaths) resolution underneath it — the importer doesn't break normal
// resolution just by being registered.
{
  const dir = join(tmpdir(), `grass-napi-importer-fallthrough-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "real-thing.scss"), "$b: blue;");
  try {
    const opts = {
      importers: [{ findFileUrl: () => null }],
      loadPaths: [dir],
    };
    const res = binding.compileString('@import "real-thing";\na { b: $b; }', opts);
    assert.equal(res.css, "a {\n  b: blue;\n}\n");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// FileImporter entries are consulted in array order before loadPaths, while a
// load-path entry still supplies normal partial resolution after they decline.
{
  const dir = join(tmpdir(), `grass-napi-loadpaths-ordering-${process.pid}`);
  const importedPath = join(tmpdir(), `grass-napi-loadpaths-theme-${process.pid}.scss`);
  const calls = [];
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "_virtual-fallback.scss"), "$color: magenta;");
  writeFileSync(join(dir, "_real-dep.scss"), "$color: orange;");
  writeFileSync(importedPath, "$color: teal;");
  try {
    const opts = {
      importers: [
        {
          findFileUrl(url) {
            calls.push(["first", url]);
            return null;
          },
        },
        {
          findFileUrl(url) {
            calls.push(["second", url]);
            return url === "virtual:theme" ? pathToFileURL(importedPath).href : null;
          },
        },
      ],
      loadPaths: [dir],
    };
    const res = binding.compileString(
      '@use "virtual:theme" as theme;\n@use "real-dep" as real;\na { color: theme.$color; border-color: real.$color; }',
      opts,
    );
    assert.deepEqual(
      calls.filter(([, url]) => url === "virtual:theme").map(([name]) => name),
      ["first", "second"],
    );
    assert.equal(res.css, "a {\n  color: teal;\n  border-color: orange;\n}\n");
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(importedPath, { force: true });
  }
}

// Load-path sugar declines schemed URLs instead of treating the scheme as a
// filesystem path.
{
  const dir = join(tmpdir(), `grass-napi-loadpaths-scheme-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  try {
    assert.throws(
      () => binding.compileString('@use "pkg:whatever" as whatever;', { loadPaths: [dir] }),
      /Can't find stylesheet to import/,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// A thrown JS exception inside findFileUrl surfaces as a clean compile
// error (not a crash), same as a thrown `functions` callback.
{
  const opts = {
    importers: [
      {
        findFileUrl() {
          throw new Error("importer kaboom");
        },
      },
    ],
  };
  assert.throws(
    () => binding.compileString('@import "anything"; a { b: c; }', opts),
    /importer kaboom/,
  );
}

// --- full `Importer` (canonicalize+load) + async importers (todo #221 slice 5b) ---

// A full Importer resolves an arbitrary non-file: scheme entirely from
// inline contents (canonicalize returns a canonical URL, load returns
// {contents, syntax}) — no filesystem involved at all.
{
  const opts = {
    importers: [
      {
        canonicalize(url, context) {
          assert.equal(typeof context.fromImport, "boolean");
          assert(context.containingUrl === null || typeof context.containingUrl === "string");
          if (url === "db:colors") return "db:colors";
          return null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "db:colors");
          return { contents: "$c: red;", syntax: "scss" };
        },
      },
    ],
  };
  const res = binding.compileString('@use "db:colors" as colors;\na { b: colors.$c; }', opts);
  assert.equal(res.css, "a {\n  b: red;\n}\n");
}

// canonicalize/load returning null declines, falling through cleanly (no
// filesystem hit, no crash) when nothing else resolves the URL either.
{
  const opts = {
    importers: [{ canonicalize: () => null, load: () => null }],
  };
  assert.throws(() => binding.compileString('@use "db:nope" as nope;', opts));
}

// An entry mixing findFileUrl with canonicalize/load (or missing all three)
// is rejected with a clear shape error, not silently misinterpreted.
{
  assert.throws(
    () => binding.compileString("a { b: c }", { importers: [{ findFileUrl: () => null, canonicalize: () => null, load: () => null }] }),
    /not a mix of both shapes/,
  );
  assert.throws(
    () => binding.compileString("a { b: c }", { importers: [{}] }),
    /must be an object with either/,
  );
}

// A thrown JS exception inside canonicalize/load surfaces as a clean compile
// error under the full Importer shape too.
{
  const opts = {
    importers: [
      {
        canonicalize: () => "db:boom",
        load() {
          throw new Error("load kaboom");
        },
      },
    ],
  };
  assert.throws(
    () => binding.compileString('@use "db:boom" as boom;', opts),
    /load kaboom/,
  );
}

// ASYNC: a FileImporter findFileUrl redirect works under compileStringAsync.
{
  const path = join(tmpdir(), `grass-napi-importer-async-file-${process.pid}.scss`);
  writeFileSync(path, "$a: teal;");
  try {
    const opts = {
      importers: [
        {
          findFileUrl(url) {
            if (url === "virtual:async-thing") return pathToFileURL(path).href;
            return null;
          },
        },
      ],
    };
    const res = await withTimeout(
      binding.compileStringAsync('@import "virtual:async-thing";\na { b: $a; }', opts),
      10000,
      "async FileImporter",
    );
    assert.equal(res.css, "a {\n  b: teal;\n}\n");
  } finally {
    rmSync(path);
  }
}

// ASYNC: a full Importer (canonicalize+load) works under compileStringAsync,
// exercising the two-sequential-round-trip path.
{
  const opts = {
    importers: [
      {
        canonicalize(url) {
          return url === "db:async-colors" ? "db:async-colors" : null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "db:async-colors");
          return { contents: "$c: purple;", syntax: "scss" };
        },
      },
    ],
  };
  const res = await withTimeout(
    binding.compileStringAsync('@use "db:async-colors" as asyncColors;\na { b: asyncColors.$c; }', opts),
    10000,
    "async full Importer",
  );
  assert.equal(res.css, "a {\n  b: purple;\n}\n");
}

// ASYNC: a throwing canonicalize/load surfaces as a clean rejection, never a
// process abort (the risk napi's call_with_return_value would otherwise hit).
{
  const opts = {
    importers: [
      {
        canonicalize: () => "db:boom-async",
        load() {
          throw new Error("async load kaboom");
        },
      },
    ],
  };
  await assert.rejects(
    withTimeout(
      binding.compileStringAsync('@use "db:boom-async" as boomAsync;', opts),
      10000,
      "async importer throw",
    ),
    /async load kaboom/,
  );
}
{
  const opts = { importers: [{ findFileUrl: () => { throw new Error("findFileUrl kaboom"); } }] };
  await assert.rejects(
    withTimeout(
      binding.compileStringAsync('@import "anything";', opts),
      10000,
      "async FileImporter throw",
    ),
    /findFileUrl kaboom/,
  );
}

// ASYNC: a Promise-returning importer method is guarded with a clear error,
// never awaited and never hung.
{
  const opts = { importers: [{ findFileUrl: async () => null }] };
  await assert.rejects(
    withTimeout(
      binding.compileStringAsync('@import "anything";', opts),
      10000,
      "async Promise-returning findFileUrl",
    ),
    /returning a Promise.*are not yet supported/,
  );
}
{
  const opts = {
    importers: [
      {
        canonicalize: async () => "db:promise",
        load: () => ({ contents: "", syntax: "scss" }),
      },
    ],
  };
  await assert.rejects(
    withTimeout(
      binding.compileStringAsync('@use "db:promise" as promiseMod;', opts),
      10000,
      "async Promise-returning canonicalize",
    ),
    /returning a Promise.*are not yet supported/,
  );
}

// CONCURRENCY: Promise.all of several compileStringAsync calls, each with
// its own importer (mixing both shapes) — all must resolve, none deadlock.
{
  async function probeImporterConcurrency(n) {
    const promises = [];
    for (let i = 0; i < n; i++) {
      const useFile = i % 2 === 0;
      const opts = useFile
        ? { importers: [{ findFileUrl: () => null }] }
        : {
            importers: [
              {
                canonicalize: (url) => (url === `db:c${i}` ? `db:c${i}` : null),
                load: () => ({ contents: `$v: ${i};`, syntax: "scss" }),
              },
            ],
          };
      const source = useFile
        ? `a { b: ${i}; }`
        : `@use "db:c${i}" as m${i};\na { b: m${i}.$v; }`;
      promises.push(binding.compileStringAsync(source, opts));
    }
    const results = await withTimeout(Promise.all(promises), 20000, `importer concurrency N=${n}`);
    results.forEach((r, i) => assert.equal(r.css, `a {\n  b: ${i};\n}\n`));
    console.log(`  importer concurrency N=${n}: PASS`);
  }
  await probeImporterConcurrency(8);
}

// --- path-based compileAsync callback combinations --------------------

// A path-based async compile invokes a custom function and preserves its
// numeric units just like compileStringAsync.
{
  const path = join(tmpdir(), `grass-napi-compile-async-function-${process.pid}.scss`);
  writeFileSync(path, "a { b: double(5px); }");
  try {
    const res = await withTimeout(
      binding.compileAsync(path, {
        functions: {
          "double($n)": (args) => {
            const n = args[0];
            assert(n instanceof binding.SassNumber);
            return new binding.SassNumber(n.value * 2, {
              numeratorUnits: n.numeratorUnits,
              denominatorUnits: n.denominatorUnits,
            });
          },
        },
      }),
      10000,
      "path async custom function",
    );
    assert.equal(res.css, "a {\n  b: 10px;\n}\n");
  } finally {
    rmSync(path);
  }
}

// A path-based async compile supports a FileImporter redirect to a temporary
// file on disk.
{
  const entryPath = join(tmpdir(), `grass-napi-compile-async-file-importer-${process.pid}.scss`);
  const importedPath = join(tmpdir(), `grass-napi-compile-async-file-imported-${process.pid}.scss`);
  writeFileSync(entryPath, '@use "virtual:async-file" as imported;\na { b: imported.$a; }');
  writeFileSync(importedPath, "$a: teal;");
  try {
    const res = await withTimeout(
      binding.compileAsync(entryPath, {
        importers: [
          {
            findFileUrl(url) {
              return url === "virtual:async-file" ? pathToFileURL(importedPath).href : null;
            },
          },
        ],
      }),
      10000,
      "path async FileImporter",
    );
    assert.equal(res.css, "a {\n  b: teal;\n}\n");
  } finally {
    rmSync(entryPath);
    rmSync(importedPath);
  }
}

// A path-based async compile supports the full canonicalize+load Importer
// shape, including its two sequential callback round trips.
{
  const path = join(tmpdir(), `grass-napi-compile-async-importer-${process.pid}.scss`);
  writeFileSync(path, '@use "db:async-colors" as colors;\na { b: colors.$c; }');
  try {
    const res = await withTimeout(
      binding.compileAsync(path, {
        importers: [
          {
            canonicalize(url) {
              return url === "db:async-colors" ? "db:async-colors" : null;
            },
            load(canonicalUrl) {
              assert.equal(canonicalUrl, "db:async-colors");
              return { contents: "$c: purple;", syntax: "scss" };
            },
          },
        ],
      }),
      10000,
      "path async full Importer",
    );
    assert.equal(res.css, "a {\n  b: purple;\n}\n");
  } finally {
    rmSync(path);
  }
}

// A FileImporter may decline a path-based async load and let loadPaths
// resolve it normally.
{
  const entryPath = join(tmpdir(), `grass-napi-compile-async-decline-${process.pid}.scss`);
  const dir = join(tmpdir(), `grass-napi-compile-async-decline-dir-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(entryPath, '@import "real-thing";\na { b: $b; }');
  writeFileSync(join(dir, "real-thing.scss"), "$b: blue;");
  try {
    const res = await withTimeout(
      binding.compileAsync(entryPath, {
        importers: [{ findFileUrl: () => null }],
        loadPaths: [dir],
      }),
      10000,
      "path async importer decline",
    );
    assert.equal(res.css, "a {\n  b: blue;\n}\n");
  } finally {
    rmSync(entryPath);
    rmSync(dir, { recursive: true, force: true });
  }
}

// The ordered FileImporter chain and load-path partial fallback also hold for
// the path-based async entry point.
{
  const entryPath = join(tmpdir(), `grass-napi-loadpaths-ordering-async-${process.pid}.scss`);
  const importedPath = join(tmpdir(), `grass-napi-loadpaths-theme-async-${process.pid}.scss`);
  const dir = join(tmpdir(), `grass-napi-loadpaths-ordering-async-dir-${process.pid}`);
  const calls = [];
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    entryPath,
    '@use "virtual:theme" as theme;\n@use "real-dep" as real;\na { color: theme.$color; border-color: real.$color; }',
  );
  writeFileSync(join(dir, "_virtual-fallback.scss"), "$color: magenta;");
  writeFileSync(join(dir, "_real-dep.scss"), "$color: orange;");
  writeFileSync(importedPath, "$color: teal;");
  try {
    const res = await withTimeout(
      binding.compileAsync(entryPath, {
        importers: [
          {
            findFileUrl(url) {
              calls.push(["first", url]);
              return null;
            },
          },
          {
            findFileUrl(url) {
              calls.push(["second", url]);
              return url === "virtual:theme" ? pathToFileURL(importedPath).href : null;
            },
          },
        ],
        loadPaths: [dir],
      }),
      10000,
      "path async loadPaths ordering",
    );
    assert.deepEqual(
      calls.filter(([, url]) => url === "virtual:theme").map(([name]) => name),
      ["first", "second"],
    );
    assert.equal(res.css, "a {\n  color: teal;\n  border-color: orange;\n}\n");
  } finally {
    rmSync(entryPath, { force: true });
    rmSync(importedPath, { force: true });
    rmSync(dir, { recursive: true, force: true });
  }
}

// A thrown path-based FileImporter callback rejects cleanly without
// terminating the process.
{
  const path = join(tmpdir(), `grass-napi-compile-async-throw-${process.pid}.scss`);
  writeFileSync(path, '@import "virtual:throw";');
  try {
    await assert.rejects(
      withTimeout(
        binding.compileAsync(path, {
          importers: [{ findFileUrl: () => { throw new Error("path importer kaboom"); } }],
        }),
        10000,
        "path async importer throw",
      ),
      /path importer kaboom/,
    );
  } finally {
    rmSync(path);
  }
}

// A missing path rejects as a normal async error rather than throwing
// synchronously or aborting the process.
{
  const path = join(tmpdir(), `grass-napi-compile-async-missing-${process.pid}.scss`);
  await assert.rejects(
    withTimeout(binding.compileAsync(path, null), 10000, "path async missing file"),
  );
}

// Path-based async source maps retain the same result shape as the string
// entry point, including embedded source text when requested.
{
  const path = join(tmpdir(), `grass-napi-compile-async-source-map-${process.pid}.scss`);
  const source = "a {\n  b: c;\n}\n";
  writeFileSync(path, source);
  try {
    const res = await withTimeout(
      binding.compileAsync(path, { sourceMap: true, sourceMapIncludeSources: true }),
      10000,
      "path async source map",
    );
    assert.equal(typeof res.sourceMap, "object");
    assert.equal(res.sourceMap.version, 3);
    assert.equal(res.sourceMap.sourceRoot, "");
    assert.deepEqual(res.sourceMap.names, []);
    assert.equal(typeof res.sourceMap.mappings, "string");
    assert.equal(res.sourceMap.file, undefined);
    assert.equal(res.sourceMap.sources.length, 1);
    assert.deepEqual(res.sourceMap.sourcesContent, [source]);
  } finally {
    rmSync(path);
  }
}

// Independent callbacks remain attached to their own concurrent path-based
// compileAsync tasks.
{
  const firstPath = join(tmpdir(), `grass-napi-compile-async-concurrent-one-${process.pid}.scss`);
  const secondPath = join(tmpdir(), `grass-napi-compile-async-concurrent-two-${process.pid}.scss`);
  writeFileSync(firstPath, "a { b: marker(); }");
  writeFileSync(secondPath, "a { b: marker(); }");
  try {
    const results = await withTimeout(
      Promise.all([
        binding.compileAsync(firstPath, {
          functions: { "marker()": () => new binding.SassString("one", false) },
        }),
        binding.compileAsync(secondPath, {
          functions: { "marker()": () => new binding.SassString("two", false) },
        }),
      ]),
      20000,
      "path async callback concurrency",
    );
    assert.equal(results[0].css, "a {\n  b: one;\n}\n");
    assert.equal(results[1].css, "a {\n  b: two;\n}\n");
  } finally {
    rmSync(firstPath);
    rmSync(secondPath);
  }
}

// --- StringOptions.importer + url (todo #280 item 1) ---

// url seeds the entrypoint importer's `containingUrl` for the source's own
// relative loads, and the entrypoint `importer` resolves them (sync).
{
  const opts = {
    url: "my://entry",
    importer: {
      canonicalize(url, context) {
        assert.equal(context.containingUrl, "my://entry");
        if (url === "dep") return "my://dep";
        return null;
      },
      load(canonicalUrl) {
        assert.equal(canonicalUrl, "my://dep");
        return { contents: "$c: red;", syntax: "scss" };
      },
    },
  };
  const res = binding.compileString('@use "dep" as d;\na { b: d.$c; }', opts);
  assert.equal(res.css, "a {\n  b: red;\n}\n");
}

// entrypoint `importer` alone (no url) resolves a custom-scheme load (sync).
{
  const opts = {
    importer: {
      canonicalize(url) {
        return url === "db:x" ? "db:x" : null;
      },
      load() {
        return { contents: "$c: blue;", syntax: "scss" };
      },
    },
  };
  const res = binding.compileString('@use "db:x" as x;\na { b: x.$c; }', opts);
  assert.equal(res.css, "a {\n  b: blue;\n}\n");
}

// StringOptions.importer handles the source's own relative load before the
// importers array, and the array remains ahead of loadPaths after it declines.
{
  const arrayPath = join(tmpdir(), `grass-napi-entrypoint-array-${process.pid}.scss`);
  const loadPathDir = join(tmpdir(), `grass-napi-entrypoint-loadpath-${process.pid}`);
  let entryDeclines = false;
  mkdirSync(loadPathDir, { recursive: true });
  writeFileSync(arrayPath, "$color: blue;");
  writeFileSync(join(loadPathDir, "_dep.scss"), "$color: green;");
  try {
    const opts = {
      url: "my://entry",
      importer: {
        canonicalize(url, context) {
          assert.equal(context.containingUrl, "my://entry");
          return !entryDeclines && url === "dep" ? "my://dep" : null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "my://dep");
          return { contents: "$color: red;", syntax: "scss" };
        },
      },
      importers: [
        {
          findFileUrl(url) {
            return url === "dep" ? pathToFileURL(arrayPath).href : null;
          },
        },
      ],
      loadPaths: [loadPathDir],
    };
    const source = '@use "dep" as d;\na { color: d.$color; }';
    assert.equal(binding.compileString(source, opts).css, "a {\n  color: red;\n}\n");
    entryDeclines = true;
    assert.equal(binding.compileString(source, opts).css, "a {\n  color: blue;\n}\n");
  } finally {
    rmSync(arrayPath, { force: true });
    rmSync(loadPathDir, { recursive: true, force: true });
  }
}

// The same entrypoint-importer precedence is preserved by compileStringAsync.
{
  const arrayPath = join(tmpdir(), `grass-napi-entrypoint-array-async-${process.pid}.scss`);
  const loadPathDir = join(tmpdir(), `grass-napi-entrypoint-loadpath-async-${process.pid}`);
  let entryDeclines = false;
  mkdirSync(loadPathDir, { recursive: true });
  writeFileSync(arrayPath, "$color: blue;");
  writeFileSync(join(loadPathDir, "_dep.scss"), "$color: green;");
  try {
    const opts = {
      url: "my://entry",
      importer: {
        canonicalize(url, context) {
          assert.equal(context.containingUrl, "my://entry");
          return !entryDeclines && url === "dep" ? "my://dep" : null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "my://dep");
          return { contents: "$color: red;", syntax: "scss" };
        },
      },
      importers: [
        {
          findFileUrl(url) {
            return url === "dep" ? pathToFileURL(arrayPath).href : null;
          },
        },
      ],
      loadPaths: [loadPathDir],
    };
    const source = '@use "dep" as d;\na { color: d.$color; }';
    const first = await withTimeout(
      binding.compileStringAsync(source, opts),
      10000,
      "string async entrypoint importer",
    );
    assert.equal(first.css, "a {\n  color: red;\n}\n");
    entryDeclines = true;
    const second = await withTimeout(
      binding.compileStringAsync(source, opts),
      10000,
      "string async importer-array precedence",
    );
    assert.equal(second.css, "a {\n  color: blue;\n}\n");
  } finally {
    rmSync(arrayPath, { force: true });
    rmSync(loadPathDir, { recursive: true, force: true });
  }
}

// url + entrypoint importer over the ASYNC entry point.
{
  const opts = {
    url: "my://entry",
    importer: {
      canonicalize(url, context) {
        assert.equal(context.containingUrl, "my://entry");
        return url === "dep" ? "my://dep" : null;
      },
      load() {
        return { contents: "$c: green;", syntax: "scss" };
      },
    },
  };
  const res = await withTimeout(
    binding.compileStringAsync('@use "dep" as d;\na { b: d.$c; }', opts),
    10000,
    "compileStringAsync url+importer",
  );
  assert.equal(res.css, "a {\n  b: green;\n}\n");
}

// url becomes the source map's entrypoint `sources` entry (not a data: URL).
{
  const res = binding.compileString("a { b: c }", {
    url: "my://entry.scss",
    sourceMap: true,
  });
  assert.ok(res.sourceMap, "expected a sourceMap");
  assert.ok(
    res.sourceMap.sources.includes("my://entry.scss"),
    `sources should include the seeded url, got ${JSON.stringify(res.sourceMap.sources)}`,
  );
}

console.log("ok");
