import assert from "assert";
import * as grass from "../index.js";

const res = grass.compileString("a { b: c }");
assert.equal(res.css, "a {\n  b: c;\n}\n");
assert.ok(Array.isArray(res.loadedUrls));

assert.equal(grass.compileString("a { b: c }", { style: "compressed" }).css, "a{b:c}");
assert.throws(() => grass.compileString("a { b: "));

const ar = await grass.compileStringAsync("a { b: c }");
assert.equal(ar.css, "a {\n  b: c;\n}\n");
await assert.rejects(grass.compileStringAsync("a { b: "));

// Native async path: when a .node binding is present, compileStringAsync must
// resolve without blocking the event loop — a timer must be able to fire
// while the compile is in flight.
import { readdirSync } from "fs";
const hasNative = readdirSync(new URL("..", import.meta.url)).some((f) => f.endsWith(".node"));
if (hasNative) {
  let timerFired = false;
  const timer = setTimeout(() => { timerFired = true; }, 0);
  // A large enough input that the compile takes > one tick.
  const big = "a { b: c }\n".repeat(50_000);
  const result = await grass.compileStringAsync(big, {});
  clearTimeout(timer);
  assert.ok(result.css.length > 0);
  assert.ok(timerFired, "event loop was blocked during compileStringAsync — native async path not used");
  console.log("native async ok");
}

// sourceMap option (todo #162): absent by default; a real object (not a
// JSON string) shaped like the Sass JS API's result when requested, on both
// the native and WASM paths — matched here regardless of which one this
// process actually loaded (see native/wasm branching in index.js).
{
  const plain = grass.compileString("a {\n  b: c;\n}\n");
  assert.equal(plain.sourceMap, undefined);

  const withMap = grass.compileString("a {\n  b: c;\n}\n", { sourceMap: true });
  assert.equal(typeof withMap.sourceMap, "object");
  assert.equal(withMap.sourceMap.version, 3);
  assert.equal(withMap.sourceMap.mappings, "AAAA;EACE");
  assert.ok(withMap.sourceMap.sources[0].startsWith("data:;charset=utf-8,"));
  assert.equal(withMap.sourceMap.sourcesContent, undefined);

  const withSources = grass.compileString("a {\n  b: c;\n}\n", {
    sourceMap: true,
    sourceMapIncludeSources: true,
  });
  assert.deepEqual(withSources.sourceMap.sourcesContent, ["a {\n  b: c;\n}\n"]);
}

console.log("smoke ok");
