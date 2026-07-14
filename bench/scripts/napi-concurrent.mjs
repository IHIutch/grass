// Measure whether no-callback napi compileAsync calls run in parallel on the
// libuv threadpool. The distinct workload uses separate Bootstrap copies so
// cross-compile cache deduplication cannot make serialization look parallel.
//
// Usage:
//   PERF_FIXTURE_DIR=/path/to/bootstrap node bench/scripts/napi-concurrent.mjs
//   UV_THREADPOOL_SIZE=8 PERF_FIXTURE_DIR=/path/to/bootstrap node bench/scripts/napi-concurrent.mjs

import { createRequire } from "node:module";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { resolveFixture } from "../fixtures/resolve.mjs";

const require = createRequire(import.meta.url);
const REPO_ROOT = resolve(new URL("../..", import.meta.url).pathname);
const binding = require(resolve(REPO_ROOT, "crates/napi/grass.darwin-arm64.node"));
const fixture = resolveFixture("bootstrap");
const threadpoolSize = process.env.UV_THREADPOOL_SIZE || "4 (default)";
const Ns = [1, 4, 8];
const WARMUPS = 2;
const REPS = 5;

const tempRoot = mkdtempSync(join(tmpdir(), "grass-napi-concurrent-"));
process.on("exit", () => rmSync(tempRoot, { recursive: true, force: true }));

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? sorted[middle]
    : (sorted[middle - 1] + sorted[middle]) / 2;
}

const sameInputs = Array.from({ length: 8 }, () => ({
  path: fixture.entry,
  loadPaths: [fixture.loadPath],
}));

function makeDistinctInputs() {
  const inputs = [];
  const batchRoot = mkdtempSync(join(tempRoot, "distinct-"));
  for (let i = 0; i < 8; i++) {
    const copyRoot = join(batchRoot, `bootstrap-${i}`);
    cpSync(fixture.root, copyRoot, { recursive: true });
    inputs.push({
      path: join(copyRoot, "scss/bootstrap.scss"),
      loadPaths: [join(copyRoot, "scss")],
    });
  }
  return inputs;
}

const distinctInputs = makeDistinctInputs();

function inputsFor(workload) {
  return workload === "same" ? sameInputs : distinctInputs;
}

async function compile(input) {
  return binding.compileAsync(input.path, {
    loadPaths: input.loadPaths,
    quiet: true,
  });
}

function assertByteIdentical(expected, actual, label) {
  if (expected.css !== actual.css) {
    console.error(`FAIL: concurrent output differs from sequential control (${label})`);
    process.exit(1);
  }
}

async function measure(workload, n) {
  const inputs = inputsFor(workload);
  const sequentialControl = [];
  for (let i = 0; i < n; i++) sequentialControl.push(await compile(inputs[i]));

  const sequential = async () => {
    const start = performance.now();
    const results = [];
    for (let i = 0; i < n; i++) results.push(await compile(inputs[i]));
    for (let i = 0; i < n; i++) {
      assertByteIdentical(sequentialControl[i], results[i], `${workload}/sequential/N=${n}`);
    }
    return performance.now() - start;
  };

  const concurrent = async () => {
    const start = performance.now();
    const results = await Promise.all(inputs.slice(0, n).map(compile));
    for (let i = 0; i < n; i++) {
      assertByteIdentical(sequentialControl[i], results[i], `${workload}/concurrent/N=${n}`);
    }
    return performance.now() - start;
  };

  for (let i = 0; i < WARMUPS; i++) {
    await sequential();
    await concurrent();
  }

  const sequentialTimes = [];
  const concurrentTimes = [];
  for (let i = 0; i < REPS; i++) {
    sequentialTimes.push(await sequential());
    concurrentTimes.push(await concurrent());
  }

  const sequentialMedian = median(sequentialTimes);
  const concurrentMedian = median(concurrentTimes);
  return {
    workload,
    n,
    sequential: sequentialMedian,
    concurrent: concurrentMedian,
    speedup: sequentialMedian / concurrentMedian,
    mean: concurrentMedian / n,
  };
}

console.log(`Bootstrap fixture: ${fixture.root}`);
console.log(`UV_THREADPOOL_SIZE: ${threadpoolSize}`);
console.log(`Warmups: ${WARMUPS}; measured reps: ${REPS}; medians reported`);
console.log(
  "workload   N  pool             sequential ms  concurrent ms  speedup  concurrent mean ms",
);

for (const workload of ["same", "distinct"]) {
  for (const n of Ns) {
    const result = await measure(workload, n);
    console.log(
      `${result.workload.padEnd(10)} ${String(result.n).padStart(1)}  ${threadpoolSize.padEnd(15)} ` +
        `${result.sequential.toFixed(1).padStart(14)}  ${result.concurrent.toFixed(1).padStart(14)}  ` +
        `${result.speedup.toFixed(2).padStart(7)}  ${result.mean.toFixed(1).padStart(20)}`,
    );
  }
}
