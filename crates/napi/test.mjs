import { createRequire } from "module";
import assert from "assert";
const require = createRequire(import.meta.url);

// napi build --platform names the file with a platform suffix; find it.
import { readdirSync } from "fs";
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

console.log("ok");
