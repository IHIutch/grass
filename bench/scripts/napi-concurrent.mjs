// Measure whether no-callback napi compileAsync calls run in parallel on the
// libuv threadpool. The distinct workload uses separate Bootstrap copies so
// cross-compile cache deduplication cannot make serialization look parallel.
//
// Usage:
//   PERF_FIXTURE_DIR=/path/to/bootstrap node bench/scripts/napi-concurrent.mjs
//   UV_THREADPOOL_SIZE=8 PERF_FIXTURE_DIR=/path/to/bootstrap node bench/scripts/napi-concurrent.mjs
// The body-CPU column times only callback bodies and does NOT predict the
// ceiling: fn-heavy measures 0.4% body CPU yet caps near 1.5x. callbacks/ms
// is the measured callback-density predictor; the serial-fraction column is
// only a wall-time inference from the observed speedup.

import { createRequire } from "node:module";
import { cpSync, existsSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath, pathToFileURL } from "node:url";
import { resolveFixture } from "../fixtures/resolve.mjs";

const require = createRequire(import.meta.url);
const REPO_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const binding = require(resolve(REPO_ROOT, "crates/napi/grass.darwin-arm64.node"));
const fixture = resolveFixture("bootstrap");
const threadpoolSize = process.env.UV_THREADPOOL_SIZE || "4 (default)";
const Ns = [1, 4, 8];
const WARMUPS = 2;
const REPS = 5;

const tempRoot = mkdtempSync(join(tmpdir(), "grass-napi-concurrent-"));
process.on("exit", () => rmSync(tempRoot, { recursive: true, force: true }));

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

function makeFunctionInputs(callCount) {
  const source = `a {\n${Array.from(
    { length: callCount },
    (_, i) => `  value-${i}: id(${i + 1}px);`,
  ).join("\n")}\n}\n`;
  return Array.from({ length: 8 }, () => {
    const callbackStats = { callbacks: 0, cpuMs: 0 };
    return {
      source,
      callbackStats,
      options: {
        functions: {
          "id($n)": (args) => {
            const start = performance.now();
            try {
              return args[0];
            } finally {
              callbackStats.callbacks++;
              callbackStats.cpuMs += performance.now() - start;
            }
          },
        },
      },
    };
  });
}

const functionInputs = {
  "fn-light": makeFunctionInputs(1000),
  "fn-heavy": makeFunctionInputs(2000),
};

function isFile(path) {
  return existsSync(path) && statSync(path).isFile();
}

function makeBootstrapFileImporter(callbackStats) {
  return {
    findFileUrl(url, context) {
      const start = performance.now();
      try {
        const containingPath = context.containingUrl?.startsWith("file:")
          ? fileURLToPath(context.containingUrl)
          : fixture.entry;
        const roots = [dirname(containingPath), fixture.loadPath];
        const names = [
          url,
          `${url}.scss`,
          `_${url}.scss`,
          `${url}.sass`,
          `_${url}.sass`,
        ];
        for (const root of roots) {
          for (const name of names) {
            const path = join(root, name);
            if (isFile(path)) return pathToFileURL(path).href;
          }
        }
        return null;
      } finally {
        callbackStats.callbacks++;
        callbackStats.cpuMs += performance.now() - start;
      }
    },
  };
}

const importerInputs = Array.from({ length: 8 }, () => {
  const callbackStats = { callbacks: 0, cpuMs: 0 };
  return {
    importerBootstrap: true,
    importer: makeBootstrapFileImporter(callbackStats),
    callbackStats,
    path: fixture.entry,
  };
});

function inputsFor(workload) {
  if (workload === "same") return sameInputs;
  if (workload === "distinct") return distinctInputs;
  if (workload in functionInputs) return functionInputs[workload];
  if (workload === "importer-bootstrap") return importerInputs;
  throw new Error(`unknown workload: ${workload}`);
}

async function compile(input) {
  if (input.callbackStats) {
    input.callbackStats.callbacks = 0;
    input.callbackStats.cpuMs = 0;
  }
  if (input.source) return binding.compileStringAsync(input.source, input.options);
  if (input.importerBootstrap) {
    return binding.compileAsync(input.path, {
      importers: [input.importer],
      quiet: true,
    });
  }
  return binding.compileAsync(input.path, {
    loadPaths: input.loadPaths,
    quiet: true,
  });
}

function callbackTotals(inputs) {
  return inputs.reduce(
    (totals, input) => {
      if (input.callbackStats) {
        totals.callbacks += input.callbackStats.callbacks;
        totals.cpuMs += input.callbackStats.cpuMs;
      }
      return totals;
    },
    { callbacks: 0, cpuMs: 0 },
  );
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
    return { elapsed: performance.now() - start, ...callbackTotals(inputs.slice(0, n)) };
  };

  const concurrent = async () => {
    const start = performance.now();
    const results = await Promise.all(inputs.slice(0, n).map(compile));
    for (let i = 0; i < n; i++) {
      assertByteIdentical(sequentialControl[i], results[i], `${workload}/concurrent/N=${n}`);
    }
    return { elapsed: performance.now() - start, ...callbackTotals(inputs.slice(0, n)) };
  };

  for (let i = 0; i < WARMUPS; i++) {
    await sequential();
    await concurrent();
  }

  const sequentialSamples = [];
  const concurrentSamples = [];
  for (let i = 0; i < REPS; i++) {
    sequentialSamples.push(await sequential());
    concurrentSamples.push(await concurrent());
  }

  const medianSample = (samples) =>
    [...samples].sort((a, b) => a.elapsed - b.elapsed)[Math.floor(samples.length / 2)];
  const sequentialSample = medianSample(sequentialSamples);
  const concurrentSample = medianSample(concurrentSamples);
  const sequentialMedian = sequentialSample.elapsed;
  const concurrentMedian = concurrentSample.elapsed;
  const callbacksPerCompile = sequentialSample.callbacks / n;
  const bodyCpuPercent = (sequentialSample.cpuMs / sequentialMedian) * 100;
  const sequentialPerCompile = sequentialMedian / n;
  const callbacksPerMs = callbacksPerCompile / sequentialPerCompile;
  return {
    workload,
    n,
    sequential: sequentialMedian,
    concurrent: concurrentMedian,
    speedup: sequentialMedian / concurrentMedian,
    serialFraction: n === 1
      ? null
      : (1 / (sequentialMedian / concurrentMedian) - 1 / n) / (1 - 1 / n),
    callbacksPerCompile,
    bodyCpuPercent,
    callbacksPerMs,
    mean: concurrentMedian / n,
  };
}

console.log(`Bootstrap fixture: ${fixture.root}`);
console.log(`UV_THREADPOOL_SIZE: ${threadpoolSize}`);
console.log(`Warmups: ${WARMUPS}; measured reps: ${REPS}; medians reported`);
console.log(
  "workload             N  pool             sequential ms  concurrent ms  speedup  serial fraction (wall)  callbacks/compile  callbacks/ms  body-CPU %  concurrent mean ms",
);

for (const workload of ["same", "distinct", "fn-light", "fn-heavy", "importer-bootstrap"]) {
  for (const n of Ns) {
    const result = await measure(workload, n);
    const serialFraction = result.serialFraction === null
      ? "n/a"
      : result.serialFraction.toFixed(3);
    console.log(
      `${result.workload.padEnd(20)} ${String(result.n).padStart(1)}  ${threadpoolSize.padEnd(15)} ` +
        `${result.sequential.toFixed(1).padStart(14)}  ${result.concurrent.toFixed(1).padStart(14)}  ` +
        `${result.speedup.toFixed(2).padStart(7)}  ${serialFraction.padStart(22)}  ` +
        `${result.callbacksPerCompile.toFixed(1).padStart(17)}  ${result.callbacksPerMs.toFixed(1).padStart(12)}  ` +
        `${result.bodyCpuPercent.toFixed(1).padStart(10)}  ` +
        `${result.mean.toFixed(1).padStart(20)}`,
    );
  }
}
