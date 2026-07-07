// Time-breakdown benchmark: where does grass WASM's time actually go, relative
// to native, napi, and sass-embedded? Instruments the fsCallbacks JS<->WASM
// boundary (todo #155). No hyperfine (see CLAUDE.md/session notes) -- uses
// plain Node timing loops with warmup + median/min/spread, matching the
// methodology used for the native-binary numbers (each rep forks a fresh
// process, same as hyperfine would).
//
// Usage: node bench-breakdown.js [uswds|bootstrap|all]
import { createRequire } from "module";
import { performance } from "perf_hooks";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { execFileSync } from "child_process";
import {
  writeFileSync,
  unlinkSync,
  readFileSync,
  statSync,
  realpathSync,
} from "fs";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));

const napi = require("../crates/napi/grass.darwin-arm64.node");
const grassWasm = await import("../crates/lib/pkg-publish/index.js");
// Raw wasm-bindgen internals (not the pkg-publish/index.js wrapper) so we can
// swap in a call-counting fs shim.
import {
  initSync,
  compile as rawWasmCompile,
} from "../crates/lib/pkg-publish/grass.js";
initSync({ module: readFileSync(resolve(__dirname, "../crates/lib/pkg-publish/grass_bg.wasm")) });

const GRASS_BIN = resolve(__dirname, "../target/release/grass");
const NATIVE_OUT = resolve(__dirname, "_bench_native_out.css");

const N = 10; // reps per engine, matches the "several runs + median" instruction (no hyperfine)

function median(arr) {
  const sorted = [...arr].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function stddev(arr, mean) {
  const variance = arr.reduce((s, x) => s + (x - mean) ** 2, 0) / arr.length;
  return Math.sqrt(variance);
}

function runN(label, fn) {
  fn(); // warmup
  fn();
  const times = [];
  for (let i = 0; i < N; i++) {
    const s = performance.now();
    fn();
    times.push(performance.now() - s);
  }
  const med = median(times);
  const min = Math.min(...times);
  const sd = stddev(times, times.reduce((a, b) => a + b, 0) / times.length);
  console.log(
    `  ${label.padEnd(24)} median: ${med.toFixed(1)}ms  min: ${min.toFixed(
      1
    )}ms  sd: ${sd.toFixed(1)}ms (n=${N}, noise caveat: no hyperfine, ambient machine load per performance-roadmap.md)`
  );
  return { med, min, sd, times };
}

const WORKLOADS = {
  uswds: () => {
    const loadPaths = [resolve(__dirname, "packages")];
    const entryFile = resolve(__dirname, "packages/uswds/_index-direct.scss");
    // Read the flattened entry directly (matches bench.sh/bench-wasm.js/
    // bench-napi.js/bench-sass.js convention) so every engine compiles
    // exactly the same content, whether fed as a file path or inline string.
    const source = readFileSync(entryFile, "utf8");
    return { name: "USWDS", loadPaths, entryFile, source };
  },
  bootstrap: () => {
    const scssDir = resolve(__dirname, "bootstrap-bench/scss");
    const entryFile = resolve(scssDir, "bootstrap.scss");
    const loadPaths = [scssDir];
    const source = readFileSync(entryFile, "utf8");
    return { name: "Bootstrap v5.0.2", loadPaths, entryFile, source };
  },
};

function benchWorkload(key) {
  const { name, loadPaths, entryFile, source } = WORKLOADS[key]();
  console.log(`\n=== ${name} Time Breakdown ===\n`);

  // Bench-scoped input files (matches each engine's expected entry point).
  const benchFile = resolve(__dirname, `_bench_${key}.scss`);
  writeFileSync(benchFile, source);

  // 1. sass-embedded: measured via a FRESH node process per rep (matching
  // performance-roadmap.md's hyperfine methodology), calling compileAsync on
  // the entry FILE rather than compileStringAsync on inline source.
  //
  // Two methodology traps found and worked around here:
  // (a) repeated in-process compileString/compileAsync calls against the
  //     USWDS workload degrade ~4x after warmup (2.9-3.0s/call vs a fresh
  //     process's ~750-900ms) -- a sass-embedded host-reuse pathology
  //     specific to large @use graphs, not reproduced on Bootstrap.
  // (b) compileStringAsync(source) (inline-string entry) is itself ~4x
  //     slower than compileAsync(path) (file entry) for USWDS specifically
  //     (3100ms vs 765-950ms, confirmed by direct A/B), even in a single
  //     fresh process/single call. compile(path) is what bench-sass.js and
  //     the roadmap baseline actually measured, so it's used here too.
  // Both are sass-embedded's own behavior, not grass's -- out of scope for
  // todo #155, noted for a future sass-embedded-focused investigation.
  const sassScript = resolve(__dirname, `_bench_sass_${key}.mjs`);
  writeFileSync(
    sassScript,
    `import * as sass from "sass-embedded";\n` +
      `await sass.compileAsync(${JSON.stringify(entryFile)}, { loadPaths: ${JSON.stringify(
        loadPaths
      )}, logger: sass.Logger.silent });\n`
  );
  const sassRes = runN("sass-embedded", () =>
    execFileSync("node", [sassScript], { stdio: "ignore", cwd: __dirname })
  );
  try {
    unlinkSync(sassScript);
  } catch {}
  const wasmRes = runN("grass WASM", () =>
    grassWasm.compileString(source, { loadPaths, quiet: true })
  );
  const napiRes = runN("grass napi-rs", () =>
    napi.compile(benchFile, { loadPaths, quiet: true })
  );

  // 2. Native CLI: forks a fresh process per rep (same cost model hyperfine
  // uses), so this is comparable to the hyperfine-measured baseline in
  // performance-roadmap.md without needing hyperfine itself.
  const nativeArgs = [
    entryFile,
    NATIVE_OUT,
    "--style=expanded",
    "-I",
    loadPaths[0],
  ];
  const nativeRes = runN("grass native (CLI)", () =>
    execFileSync(GRASS_BIN, nativeArgs, { stdio: "ignore" })
  );

  // 3. Count fs calls in the WASM path for this workload.
  let fsCallCount = 0;
  const countingFs = {
    is_file(path) {
      fsCallCount++;
      try {
        return statSync(path).isFile();
      } catch {
        return false;
      }
    },
    is_dir(path) {
      fsCallCount++;
      try {
        return statSync(path).isDirectory();
      } catch {
        return false;
      }
    },
    read(path) {
      fsCallCount++;
      return Array.from(readFileSync(path));
    },
    canonicalize(path) {
      fsCallCount++;
      return realpathSync(path);
    },
    resolve_first_existing(candidates) {
      fsCallCount++;
      for (const p of candidates) {
        try {
          if (statSync(p).isFile()) return p;
        } catch {}
      }
      return null;
    },
  };
  fsCallCount = 0;
  rawWasmCompile(source, loadPaths, "expanded", true, false, false, countingFs);
  const totalFsCalls = fsCallCount;

  // 4. Measure fs call overhead in isolation (statSync on nonexistent paths,
  // representative of the is_file/resolve_first_existing miss-heavy pattern
  // that dominates import resolution).
  const testPaths = [];
  for (let i = 0; i < 100; i++)
    testPaths.push(resolve(__dirname, `packages/nonexistent_${i}.scss`));
  const fsStart = performance.now();
  for (let rep = 0; rep < 100; rep++) {
    for (const p of testPaths) {
      try {
        statSync(p);
      } catch {}
    }
  }
  const fsPerCall = (performance.now() - fsStart) / 10000; // 100*100 calls

  console.log(`\n--- ${name} Analysis ---\n`);
  console.log(`grass native (CLI, own process):   ${nativeRes.med.toFixed(0)}ms`);
  console.log(`grass napi-rs (in Node process):    ${napiRes.med.toFixed(0)}ms`);
  console.log(`grass WASM (in Node process):        ${wasmRes.med.toFixed(0)}ms`);
  console.log(`sass-embedded (Dart VM, IPC):        ${sassRes.med.toFixed(0)}ms`);
  console.log(``);
  console.log(`WASM-JS boundary crossings:         ${totalFsCalls} fs calls per compilation`);
  console.log(`Avg time per fs call (statSync):    ${(fsPerCall * 1000).toFixed(1)}µs`);
  const estFsOverhead = totalFsCalls * fsPerCall;
  console.log(`Estimated fs boundary overhead:     ${estFsOverhead.toFixed(1)}ms`);
  console.log(``);
  const pureCompile = nativeRes.med;
  console.log(`--- Where time goes (grass napi-rs ${napiRes.med.toFixed(0)}ms) ---`);
  const nodeOverhead = napiRes.med - pureCompile;
  console.log(
    `  Pure Sass compilation (native CLI proxy): ~${pureCompile.toFixed(
      0
    )}ms (${((pureCompile / napiRes.med) * 100).toFixed(0)}%)`
  );
  console.log(
    `  Node/napi-rs overhead:                    ~${nodeOverhead.toFixed(
      0
    )}ms (${((nodeOverhead / napiRes.med) * 100).toFixed(0)}%)`
  );
  console.log(``);
  console.log(`--- Where time goes (grass WASM ${wasmRes.med.toFixed(0)}ms) ---`);
  const wasmOverhead = wasmRes.med - pureCompile;
  const wasmExecOverhead = wasmOverhead - estFsOverhead;
  console.log(
    `  Pure Sass compilation (native CLI proxy): ~${pureCompile.toFixed(
      0
    )}ms (${((pureCompile / wasmRes.med) * 100).toFixed(0)}%)`
  );
  console.log(
    `  WASM-JS fs boundary overhead:             ~${estFsOverhead.toFixed(
      1
    )}ms (${((estFsOverhead / wasmRes.med) * 100).toFixed(0)}%)`
  );
  console.log(
    `  WASM execution overhead (residual):       ~${wasmExecOverhead.toFixed(
      0
    )}ms (${((wasmExecOverhead / wasmRes.med) * 100).toFixed(0)}%)`
  );

  try {
    unlinkSync(benchFile);
  } catch {}

  return { name, nativeRes, napiRes, wasmRes, sassRes, totalFsCalls, estFsOverhead, wasmExecOverhead };
}

const which = process.argv[2] || "all";
const keys = which === "all" ? Object.keys(WORKLOADS) : [which];
for (const k of keys) {
  if (!WORKLOADS[k]) {
    console.error(`Unknown workload "${k}". Options: ${Object.keys(WORKLOADS).join(", ")}, all`);
    process.exit(1);
  }
}

const results = {};
for (const k of keys) {
  results[k] = benchWorkload(k);
}

try {
  unlinkSync(NATIVE_OUT);
} catch {}
