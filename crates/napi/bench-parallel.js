#!/usr/bin/env node
/**
 * Benchmark: compile() vs compileParallel() on USWDS
 *
 * Usage:
 *   node bench-parallel.js [runs]
 *
 * Runs both sequential and parallel compilation of USWDS,
 * reports median times and speedup.
 */

const path = require("path");
const grass = require("./index.js");

const ENTRY = path.resolve(__dirname, "../../prototype/packages/uswds/_index-direct.scss");
const LOAD_PATHS = [path.resolve(__dirname, "../../prototype/packages")];
const RUNS = parseInt(process.argv[2] || "10", 10);

function median(arr) {
  const sorted = [...arr].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

async function bench(label, fn, runs) {
  // Warmup
  await fn();

  const times = [];
  for (let i = 0; i < runs; i++) {
    const t0 = performance.now();
    await fn();
    times.push(performance.now() - t0);
  }

  const med = median(times);
  const min = Math.min(...times);
  const max = Math.max(...times);
  console.log(
    `  ${label}: ${med.toFixed(1)}ms median (min=${min.toFixed(1)}, max=${max.toFixed(1)}, n=${runs})`
  );
  return med;
}

async function main() {
  const opts = { loadPaths: LOAD_PATHS, style: "expanded" };

  console.log(`Benchmarking USWDS compilation (${RUNS} runs each)\n`);

  // Sequential sync
  const tSeqSync = await bench("compile (sync)", () => {
    return grass.compile(ENTRY, opts);
  }, RUNS);

  // Parallel sync
  const tParSync = await bench("compileParallel (sync)", () => {
    return grass.compileParallel(ENTRY, opts);
  }, RUNS);

  // Sequential async
  const tSeqAsync = await bench("compileAsync", async () => {
    return grass.compileAsync(ENTRY, opts);
  }, RUNS);

  // Parallel async
  const tParAsync = await bench("compileParallelAsync", async () => {
    return grass.compileParallelAsync(ENTRY, opts);
  }, RUNS);

  // Verify output matches
  const seqResult = grass.compile(ENTRY, opts);
  const parResult = grass.compileParallel(ENTRY, opts);
  const match = seqResult.css === parResult.css;

  console.log();
  console.log("Results:");
  console.log(`  Sync speedup:  ${(tSeqSync / tParSync).toFixed(2)}x`);
  console.log(`  Async speedup: ${(tSeqAsync / tParAsync).toFixed(2)}x`);
  console.log(`  Output match:  ${match ? "YES (byte-identical)" : `NO (seq=${seqResult.css.length}, par=${parResult.css.length})`}`);
}

main().catch(console.error);
