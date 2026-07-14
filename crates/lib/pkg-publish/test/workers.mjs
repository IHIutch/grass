import assert from "assert";
import { readFileSync } from "fs";
import * as workers from "../workers.js";

// Calling before init must throw the descriptive init error.
assert.throws(() => workers.compileString("a { b: c }"), /WASM not initialized/);
await assert.rejects(
  workers.compileStringAsync("a { b: c }"),
  /WASM not initialized/,
);

// Workers requires a pre-compiled WebAssembly.Module from a static import;
// simulate that here by compiling the bytes ourselves.
const wasmBytes = readFileSync(new URL("../grass_bg.wasm", import.meta.url));
const wasmModule = await WebAssembly.compile(wasmBytes);
workers.init(wasmModule);

const res = workers.compileString("a { b: c }");
assert.equal(res.css, "a {\n  b: c;\n}");
assert.ok(Array.isArray(res.loadedUrls));
assert.equal(res.sourceMap, undefined);

assert.equal(workers.compileString("a { b: c }", { style: "compressed" }).css, "a{b:c}");
assert.throws(() => workers.compileString("a { b: "));

const ar = await workers.compileStringAsync("a { b: c }");
assert.equal(ar.css, "a {\n  b: c;\n}");

// sourceMap option: absent by default; a real object when requested.
{
  const plain = workers.compileString("a {\n  b: c;\n}\n");
  assert.equal(plain.sourceMap, undefined);

  const withMap = workers.compileString("a {\n  b: c;\n}\n", { sourceMap: true });
  assert.equal(typeof withMap.sourceMap, "object");
  assert.equal(withMap.sourceMap.version, 3);
}

console.log("workers smoke ok");
