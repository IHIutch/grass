import assert from "assert";
import { readFileSync } from "fs";
import * as browser from "../browser.js";

// Calling before init must throw the descriptive init error.
assert.throws(() => browser.compileString("a { b: c }"), /WASM not initialized/);
await assert.rejects(
  browser.compileStringAsync("a { b: c }"),
  /WASM not initialized/,
);

// Init from raw bytes (the browser/bundler path when no fetch is available).
const wasmBytes = readFileSync(new URL("../grass_bg.wasm", import.meta.url));
await browser.init(wasmBytes);

const res = browser.compileString("a { b: c }");
assert.equal(res.css, "a {\n  b: c;\n}");
assert.ok(Array.isArray(res.loadedUrls));
assert.equal(res.sourceMap, undefined);

assert.equal(browser.compileString("a { b: c }", { style: "compressed" }).css, "a{b:c}");
assert.throws(() => browser.compileString("a { b: "));

const ar = await browser.compileStringAsync("a { b: c }");
assert.equal(ar.css, "a {\n  b: c;\n}");

// sourceMap option: absent by default; a real object when requested.
{
  const plain = browser.compileString("a {\n  b: c;\n}\n");
  assert.equal(plain.sourceMap, undefined);

  const withMap = browser.compileString("a {\n  b: c;\n}\n", { sourceMap: true });
  assert.equal(typeof withMap.sourceMap, "object");
  assert.equal(withMap.sourceMap.version, 3);
}

// compile() without options.fs throws a descriptive error (no filesystem in
// browser/bundler environments unless the caller supplies one).
assert.throws(() => browser.compile("a.scss"), /requires options\.fs/);

console.log("browser smoke ok");
