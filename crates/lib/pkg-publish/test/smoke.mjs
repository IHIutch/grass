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

console.log("smoke ok");
