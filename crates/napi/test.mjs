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

console.log("ok");
