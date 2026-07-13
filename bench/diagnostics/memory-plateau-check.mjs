// Checks whether wasm linear memory plateaus (todo #278/#282) across
// repeated compiles in one instance, rather than growing every call.
//
// wasm-bindgen's `--target web` glue keeps the `wasm` instance-exports
// object module-private; this patches a `__debug_wasm` export onto the
// generated grass.js (idempotent, only touches the untracked/gitignored
// build artifact under crates/lib/pkg-publish -- never commit that patch).
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { readFileSync, writeFileSync, statSync, realpathSync, readdirSync } from "fs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const grassJsPath = resolve(__dirname, "../../crates/lib/pkg-publish/grass.js");

let src = readFileSync(grassJsPath, "utf8");
if (!src.includes("__debug_wasm")) {
  src = src.replace(
    "export { initSync, __wbg_init as default };",
    "export { initSync, __wbg_init as default, wasm as __debug_wasm };"
  );
  writeFileSync(grassJsPath, src);
}

const wasmMod = await import(grassJsPath);
const { initSync, compile: wasmCompile } = wasmMod;
initSync({ module: readFileSync(resolve(__dirname, "../../crates/lib/pkg-publish/grass_bg.wasm")) });

const fsCallbacks = {
  is_file(p) { try { return statSync(p).isFile(); } catch { return false; } },
  is_dir(p) { try { return statSync(p).isDirectory(); } catch { return false; } },
  read(p) { return readFileSync(p); },
  canonicalize(p) { return realpathSync(p); },
  resolve_first_existing(c) { for (const p of c) { try { if (statSync(p).isFile()) return p; } catch {} } return null; },
  readdirSync(d) { try { return readdirSync(d, { withFileTypes: true }).map((e) => (e.isFile() ? "f" : e.isDirectory() ? "d" : "o") + e.name); } catch { return []; } },
};

const uswdsPath = resolve(__dirname, "../fixtures/packages/uswds/_index-direct.scss");
const uswdsSrc = readFileSync(uswdsPath, "utf8");
const loadPaths = [resolve(__dirname, "../fixtures/packages")];

const N = process.argv[2] ? parseInt(process.argv[2], 10) : 15;
for (let i = 0; i < N; i++) {
  wasmCompile(uswdsSrc, loadPaths, "expanded", true, false, false, fsCallbacks);
  const mib = wasmMod.__debug_wasm.memory.buffer.byteLength / (1024 * 1024);
  console.log(`iter ${i}: memory.buffer.byteLength = ${mib.toFixed(2)} MiB`);
}
