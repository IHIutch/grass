import { createRequire } from "module";
import assert from "assert";
const require = createRequire(import.meta.url);

// napi build --platform names the file with a platform suffix; find it.
import { readdirSync, writeFileSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
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

// `functions` + `compileAsync`/`compileStringAsync`: not yet supported
// (todo #221 slice 2 — see functions.rs's module doc comment), rejected
// synchronously with a clear error rather than silently ignored or
// (unsoundly) executed off the JS thread.
{
  const opts = { functions: { "noop()": () => null } };
  assert.throws(
    () => binding.compileAsync("a { b: c }", opts),
    /functions is not yet supported with compileAsync/,
  );
  assert.throws(
    () => binding.compileStringAsync("a { b: c }", opts),
    /functions is not yet supported with compileStringAsync/,
  );
}

console.log("ok");
