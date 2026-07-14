import assert from "assert";
import { spawnSync } from "child_process";
import { writeFileSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);

if (process.argv[2] !== "--worker") {
  // Launcher: re-exec this file in a subprocess with GRASS_FORCE_WASM=1, so
  // the assertions below exercise the WASM path regardless of whether a
  // .node binding happens to be present in this environment.
  const result = spawnSync(process.execPath, [__filename, "--worker"], {
    stdio: "inherit",
    env: { ...process.env, GRASS_FORCE_WASM: "1" },
  });
  if (result.status !== 0) {
    console.error("forced-wasm worker failed");
    process.exit(result.status ?? 1);
  }
  console.log("forced-wasm ok");
  process.exit(0);
}

// --- Worker: runs under GRASS_FORCE_WASM=1 ---

assert.equal(process.env.GRASS_FORCE_WASM, "1");

const grass = await import("../index.js");
assert.equal(grass.SassNumber, undefined, "GRASS_FORCE_WASM did not disable the native binding");

// --- compileString: expanded, compressed, error throw ---

assert.equal(grass.compileString("a { b: c }").css, "a {\n  b: c;\n}");
assert.equal(grass.compileString("a { b: c }", { style: "compressed" }).css, "a{b:c}");
assert.throws(() => grass.compileString("a { b: "));

// --- compile(path) + compileAsync(path): fixture that @uses a second
// fixture, proving fsCallbacks (incl. import resolution) work over WASM ---

const depName = `grass-forced-wasm-dep-${process.pid}`;
const depPath = join(tmpdir(), `${depName}.scss`);
const entryPath = join(tmpdir(), `grass-forced-wasm-entry-${process.pid}.scss`);
writeFileSync(depPath, "$c: teal;");
writeFileSync(entryPath, `@use "${depName}" as dep;\na { b: dep.$c; }`);

try {
  const fileResult = grass.compile(entryPath);
  assert.equal(fileResult.css, "a {\n  b: teal;\n}");

  const asyncResult = await grass.compileAsync(entryPath);
  assert.equal(asyncResult.css, "a {\n  b: teal;\n}");
} finally {
  rmSync(depPath);
  rmSync(entryPath);
}

// --- sourceMap:true → real object, shaped like the Sass JS API's result ---

const withMap = grass.compileString("a {\n  b: c;\n}\n", { sourceMap: true });
assert.equal(typeof withMap.sourceMap, "object");
assert.equal(withMap.sourceMap.version, 3);

// --- functions/importers still require the native binding under forced
// WASM, even when a .node file is present in this environment ---

assert.throws(
  () => grass.compileString("a { b: c }", { functions: { "f()": () => null } }),
  /native binding/,
);
assert.throws(
  () => grass.compileString("a { b: c }", { importers: [{ findFileUrl: () => null }] }),
  /native binding/,
);

console.log("forced-wasm worker ok");
