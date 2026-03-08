import { createRequire } from "module";
import { performance } from "perf_hooks";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { writeFileSync, unlinkSync } from "fs";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));

// Load napi module
const napi = require("../crates/napi/grass.darwin-arm64.node");

// Load sass-embedded
import * as sass from "sass-embedded";

// Load grass WASM
const grassWasm = await import("../crates/lib/pkg-publish/index.js");

const loadPaths = [resolve(__dirname, "packages")];
const iterations = 5;

// Create a temp file for from_path benchmarks
const benchFile = resolve(__dirname, "_bench_uswds.scss");
writeFileSync(benchFile, '@use "uswds";');

console.log("=== USWDS Compilation Benchmark ===\n");
console.log(`Iterations: ${iterations}\n`);

function bench(name, fn) {
  // Warmup
  fn();

  const times = [];
  for (let i = 0; i < iterations; i++) {
    const start = performance.now();
    const result = fn();
    const elapsed = performance.now() - start;
    times.push(elapsed);
    console.log(`  Run ${i + 1}: ${elapsed.toFixed(0)}ms (${(result.css.length / 1024).toFixed(0)}KB)`);
  }
  const avg = times.reduce((a, b) => a + b, 0) / times.length;
  const min = Math.min(...times);
  console.log(`  Avg: ${avg.toFixed(0)}ms, Min: ${min.toFixed(0)}ms\n`);
  return { name, avg, min };
}

// --- sass-embedded (baseline) ---
console.log("--- sass-embedded (baseline) ---");
const sassResult = bench("sass-embedded", () =>
  sass.compileString('@use "uswds";', { loadPaths, quietDeps: true })
);

// --- grass WASM ---
console.log("--- grass WASM ---");
const wasmResult = bench("grass WASM", () =>
  grassWasm.compileString('@use "uswds";', { loadPaths, quiet: true })
);

// --- grass napi-rs (native) ---
console.log("--- grass napi-rs (native) ---");
const napiResult = bench("grass napi-rs", () =>
  napi.compile(benchFile, { loadPaths, quiet: true })
);

// Cleanup
try { unlinkSync(benchFile); } catch {}

// Summary
console.log("=== Summary (avg times) ===");
console.log(`  sass-embedded:    ${sassResult.avg.toFixed(0)}ms (baseline)`);
console.log(`  grass WASM:       ${wasmResult.avg.toFixed(0)}ms (${(sassResult.avg / wasmResult.avg).toFixed(2)}x faster)`);
console.log(`  grass napi-rs:    ${napiResult.avg.toFixed(0)}ms (${(sassResult.avg / napiResult.avg).toFixed(2)}x faster)`);
console.log(`  napi vs WASM:     ${((1 - napiResult.avg / wasmResult.avg) * 100).toFixed(1)}% faster`);
