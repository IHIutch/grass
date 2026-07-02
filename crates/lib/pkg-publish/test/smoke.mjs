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

console.log("smoke ok");
