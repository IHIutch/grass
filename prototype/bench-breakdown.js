import { createRequire } from "module";
import { performance } from "perf_hooks";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { writeFileSync, unlinkSync, readFileSync, statSync, realpathSync } from "fs";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));

const napi = require("../crates/napi/grass.darwin-arm64.node");
import * as sass from "sass-embedded";
const grassWasm = await import("../crates/lib/pkg-publish/index.js");

const loadPaths = [resolve(__dirname, "packages")];
const benchFile = resolve(__dirname, "_bench_uswds.scss");
writeFileSync(benchFile, '@use "uswds";');

const N = 5;

function median(arr) {
  const sorted = [...arr].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function runN(label, fn) {
  fn(); // warmup
  const times = [];
  for (let i = 0; i < N; i++) {
    const s = performance.now();
    fn();
    times.push(performance.now() - s);
  }
  const med = median(times);
  const min = Math.min(...times);
  console.log(`  ${label.padEnd(20)} median: ${med.toFixed(0)}ms  min: ${min.toFixed(0)}ms`);
  return { med, min };
}

// Count WASM-JS boundary calls
let fsCallCount = 0;
const countingFs = {
  is_file(path) { fsCallCount++; try { return statSync(path).isFile(); } catch { return false; } },
  is_dir(path) { fsCallCount++; try { return statSync(path).isDirectory(); } catch { return false; } },
  read(path) { fsCallCount++; return Array.from(readFileSync(path)); },
  canonicalize(path) { fsCallCount++; return realpathSync(path); },
  resolve_first_existing(candidates) { fsCallCount++; for (const p of candidates) { try { if (statSync(p).isFile()) return p; } catch {} } return null; },
};

// Import WASM internals to call with counting fs
import { initSync, compile as rawWasmCompile } from "../crates/lib/pkg-publish/grass_wasm.js";

console.log("=== USWDS Time Breakdown ===\n");

// 1. Measure each engine
const sassRes = runN("sass-embedded", () =>
  sass.compileString('@use "uswds";', { loadPaths, quietDeps: true })
);
const wasmRes = runN("grass WASM", () =>
  grassWasm.compileString('@use "uswds";', { loadPaths, quiet: true })
);
const napiRes = runN("grass napi-rs", () =>
  napi.compile(benchFile, { loadPaths, quiet: true })
);

// 2. Count fs calls in WASM path
fsCallCount = 0;
rawWasmCompile('@use "uswds";', loadPaths, "expanded", true, countingFs);
const totalFsCalls = fsCallCount;

// 3. Measure fs call overhead in isolation
const testPaths = [];
for (let i = 0; i < 100; i++) testPaths.push(resolve(__dirname, `packages/nonexistent_${i}.scss`));
const fsStart = performance.now();
for (let rep = 0; rep < 100; rep++) {
  for (const p of testPaths) { try { statSync(p); } catch {} }
}
const fsPerCall = (performance.now() - fsStart) / 10000; // 100*100 calls

console.log(`\n=== Analysis ===\n`);
console.log(`Native binary (no Node overhead):  ~1460ms`);
console.log(`grass napi-rs (in Node process):   ${napiRes.med.toFixed(0)}ms`);
console.log(`grass WASM (in Node process):      ${wasmRes.med.toFixed(0)}ms`);
console.log(`sass-embedded (Dart VM, IPC):      ${sassRes.med.toFixed(0)}ms`);
console.log(``);
console.log(`WASM-JS boundary crossings:        ${totalFsCalls} fs calls per compilation`);
console.log(`Avg time per fs call (statSync):   ${(fsPerCall * 1000).toFixed(1)}µs`);
console.log(`Estimated fs boundary overhead:    ${(totalFsCalls * fsPerCall).toFixed(0)}ms`);
console.log(``);
console.log(`--- Where time goes (grass napi-rs ${napiRes.med.toFixed(0)}ms) ---`);
const pureCompile = 1460; // native binary baseline
const nodeOverhead = napiRes.med - pureCompile;
console.log(`  Pure Sass compilation:           ~${pureCompile}ms (${(pureCompile/napiRes.med*100).toFixed(0)}%)`);
console.log(`  Node/napi-rs overhead:           ~${nodeOverhead.toFixed(0)}ms (${(nodeOverhead/napiRes.med*100).toFixed(0)}%)`);
console.log(``);
console.log(`--- Where time goes (grass WASM ${wasmRes.med.toFixed(0)}ms) ---`);
const wasmOverhead = wasmRes.med - pureCompile;
const estFsOverhead = totalFsCalls * fsPerCall;
const wasmExecOverhead = wasmOverhead - estFsOverhead;
console.log(`  Pure Sass compilation:           ~${pureCompile}ms (${(pureCompile/wasmRes.med*100).toFixed(0)}%)`);
console.log(`  WASM-JS fs boundary overhead:    ~${estFsOverhead.toFixed(0)}ms (${(estFsOverhead/wasmRes.med*100).toFixed(0)}%)`);
console.log(`  WASM execution overhead:         ~${wasmExecOverhead.toFixed(0)}ms (${(wasmExecOverhead/wasmRes.med*100).toFixed(0)}%)`);

try { unlinkSync(benchFile); } catch {}
