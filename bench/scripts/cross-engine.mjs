// Time-breakdown benchmark: where does grass WASM's time actually go, relative
// to native, napi, and sass-embedded? The default WASM leg uses the shipped
// pkg-publish surface; --diagnose-fs opts into the fsCallbacks JS<->WASM
// boundary diagnostic (todo #155). No hyperfine (see CLAUDE.md/session notes) -- uses
// plain Node timing loops with warmup + median/min/spread, matching the
// methodology used for the native-binary numbers (each rep forks a fresh
// process, same as hyperfine would).
//
// Usage: node cross-engine.mjs --engine <native|napi|wasm|wasm-string|sass-embedded|breakdown> --fixture <uswds|bootstrap>
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
  existsSync,
} from "fs";
import { resolveFixture } from "../fixtures/resolve.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");
const GRASS_BIN = resolve(REPO_ROOT, "target/release/grass");
const NATIVE_OUT = "/tmp/grass-bench-native-out.css";
const SURFACE_LABEL = "grass WASM (pkg-publish surface)";
const DIAGNOSTIC_LABEL = "fs-boundary diagnostic (shimmed, NOT a surface number)";

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
    )}ms  sd: ${sd.toFixed(1)}ms (n=${N}, noise caveat: no hyperfine, ambient machine load per bench/README.md)`
  );
  return { med, min, sd, times };
}

const WORKLOADS = {
  uswds: () => {
    const fixture = resolveFixture("uswds");
    const loadPaths = [fixture.loadPath];
    const entryFile = fixture.entry;
    // Read the flattened entry directly so every engine compiles
    // exactly the same content, whether fed as a file path or inline string.
    const source = readFileSync(entryFile, "utf8");
    return { name: "USWDS", loadPaths, entryFile, source };
  },
  bootstrap: () => {
    const fixture = resolveFixture("bootstrap");
    const entryFile = fixture.entry;
    const loadPaths = [fixture.loadPath];
    const source = readFileSync(entryFile, "utf8");
    return { name: "Bootstrap v5.0.2", loadPaths, entryFile, source };
  },
};

async function benchWorkload(key, diagnoseFs = false) {
  const require = createRequire(import.meta.url);
  const napi = require(resolve(REPO_ROOT, "crates/napi/grass.darwin-arm64.node"));
  process.env.GRASS_FORCE_WASM = "1";
  const grassWasm = await import(resolve(REPO_ROOT, "crates/lib/pkg-publish/index.js"));
  const { initSync, compile: rawWasmCompile } = await import(resolve(REPO_ROOT, "crates/lib/pkg-publish/grass.js"));
  initSync({ module: readFileSync(resolve(REPO_ROOT, "crates/lib/pkg-publish/grass_bg.wasm")) });
  const { name, loadPaths, entryFile, source } = WORKLOADS[key]();
  console.log(`\n=== ${name} Time Breakdown ===\n`);

  // Bench-scoped input files (matches each engine's expected entry point).
  const benchFile = resolve(__dirname, `_bench_${key}.scss`);
  writeFileSync(benchFile, source);

  // 1. sass-embedded: measured via a FRESH node process per rep (matching
  // bench/README.md's hyperfine methodology), calling compileAsync on
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
  //     fresh process/single call. compile(path) is what the original
  //     benchmark and roadmap baseline measured, so it's used here too.
  // Both are sass-embedded's own behavior, not grass's -- out of scope for
  // todo #155, noted for a future sass-embedded-focused investigation.
  // sass-embedded is an OPTIONAL reference engine: it's a devDependency that
  // may not be installed (todo #277). If it can't be resolved, or a rep fails,
  // skip it with a SKIPPED line and still bench the grass engines below -- a
  // missing reference engine must never take down the whole run.
  let sassRes = null;
  let sassAvailable = true;
  try {
    require.resolve("sass-embedded");
  } catch {
    sassAvailable = false;
  }
  if (sassAvailable) {
    const sassScript = resolve(__dirname, `_bench_sass_${key}.mjs`);
    writeFileSync(
      sassScript,
      `import * as sass from "sass-embedded";\n` +
        `await sass.compileAsync(${JSON.stringify(entryFile)}, { loadPaths: ${JSON.stringify(
          loadPaths
        )}, logger: sass.Logger.silent });\n`
    );
    try {
      sassRes = runN("sass-embedded", () =>
        execFileSync("node", [sassScript], { stdio: "ignore", cwd: __dirname })
      );
    } catch (e) {
      console.log(
        `  ${"sass-embedded".padEnd(24)} SKIPPED (run failed: ${
          String(e.message).split("\n")[0]
        })`
      );
    }
    try {
      unlinkSync(sassScript);
    } catch {}
  } else {
    console.log(
      `  ${"sass-embedded".padEnd(24)} SKIPPED (not installed -- run \`npm ci\` in bench/ to enable this comparison)`
    );
  }
  const wasmRes = runN(SURFACE_LABEL, () =>
    grassWasm.compileString(source, { loadPaths, quiet: true })
  );
  const napiRes = runN("grass napi-rs", () =>
    napi.compile(benchFile, { loadPaths, quiet: true })
  );

  // 2. Native CLI: forks a fresh process per rep (same cost model hyperfine
  // uses), so this is comparable to the hyperfine-measured baseline in
  // bench/README.md without needing hyperfine itself.
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

  // 3. The old per-call counting shim is deliberately opt-in. It bypasses the
  // batched directory-listing path used by pkg-publish/index.js and therefore
  // cannot be reported as a surface timing or overhead number.
  let diagnosticRes = null;
  if (diagnoseFs) {
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
    try {
      fsCallCount = 0;
      rawWasmCompile(source, loadPaths, "expanded", true, false, false, countingFs);
      const totalFsCalls = fsCallCount;

      // Measure fs call overhead in isolation (statSync on nonexistent paths,
      // representative of the is_file/resolve_first_existing miss-heavy pattern
      // that dominates import resolution).
      const testPaths = [];
      for (let i = 0; i < 100; i++)
        testPaths.push(resolve(FIXTURE_ROOT, `packages/nonexistent_${i}.scss`));
      const fsStart = performance.now();
      for (let rep = 0; rep < 100; rep++) {
        for (const p of testPaths) {
          try {
            statSync(p);
          } catch {}
        }
      }
      const fsPerCall = (performance.now() - fsStart) / 10000; // 100*100 calls
      diagnosticRes = {
        totalFsCalls,
        fsPerCall,
        estFsOverhead: totalFsCalls * fsPerCall,
      };
    } catch (error) {
      console.log(
        `  ${DIAGNOSTIC_LABEL}: diagnostic leg unsupported for this workload (${String(
          error?.message || error
        ).split("\\n")[0]})`
      );
    }
  }

  console.log(`\n--- ${name} Analysis ---\n`);
  console.log(`grass native (CLI, own process):   ${nativeRes.med.toFixed(0)}ms`);
  console.log(`grass napi-rs (in Node process):    ${napiRes.med.toFixed(0)}ms`);
  console.log(`${SURFACE_LABEL.padEnd(36)} ${wasmRes.med.toFixed(0)}ms`);
  console.log(
    `sass-embedded (Dart VM, IPC):        ${
      sassRes ? `${sassRes.med.toFixed(0)}ms` : "SKIPPED (not installed)"
    }`
  );
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
  if (diagnosticRes) {
    const { totalFsCalls, fsPerCall, estFsOverhead } = diagnosticRes;
    console.log(`\n--- ${DIAGNOSTIC_LABEL} ---\n`);
    console.log(`${DIAGNOSTIC_LABEL}: ${totalFsCalls} fs calls per compilation`);
    console.log(`Avg time per fs call (statSync):    ${(fsPerCall * 1000).toFixed(1)}µs`);
    console.log(`Estimated fs boundary overhead:     ${estFsOverhead.toFixed(1)}ms`);
    console.log(`These figures describe the shimmed diagnostic leg only; they are NOT surface numbers.`);
  }

  try {
    unlinkSync(benchFile);
  } catch {}

  return { name, nativeRes, napiRes, wasmRes, sassRes, diagnosticRes };
}

function option(args, name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : fallback;
}

async function runWorker(engine, fixture) {
  const { loadPaths, entryFile } = WORKLOADS[fixture]();
  if (engine === "native") {
    execFileSync(GRASS_BIN, [entryFile, "--style=expanded", "-I", loadPaths[0]], { stdio: "ignore" });
    return;
  }
  if (engine === "sass-embedded") {
    const sass = await import("sass-embedded");
    await sass.compileAsync(entryFile, { loadPaths, logger: sass.Logger.silent, charset: false });
    return;
  }
  if (engine === "napi") {
    const require = createRequire(import.meta.url);
    const napi = require(resolve(REPO_ROOT, "crates/napi/grass.darwin-arm64.node"));
    napi.compile(entryFile, { loadPaths, quiet: true, charset: false });
    return;
  }
  if (engine === "wasm" || engine === "wasm-string") {
    const { compile } = await import(resolve(REPO_ROOT, "crates/lib/pkg-publish/index.js"));
    if (engine === "wasm-string") {
      compile(entryFile, { loadPaths, quiet: true });
      const times = [];
      for (let i = 0; i < 5; i++) {
        const start = performance.now();
        compile(entryFile, { loadPaths, quiet: true });
        times.push(performance.now() - start);
      }
      times.sort((a, b) => a - b);
      const median = times[Math.floor(times.length / 2)];
      console.log(`Runs: ${times.map((t) => t.toFixed(0) + "ms").join(", ")}`);
      console.log(`WASM compile (median, no startup): ${median.toFixed(0)}ms`);
    } else {
      compile(entryFile, { loadPaths, quiet: true, charset: false });
    }
    return;
  }
  throw new Error(`Unknown engine: ${engine}`);
}

async function main() {
  const args = process.argv.slice(2);
  const engine = option(args, "--engine", "breakdown");
  const fixture = option(args, "--fixture", "all");
  const diagnoseFs = args.includes("--diagnose-fs");
  const keys = fixture === "all" ? Object.keys(WORKLOADS) : [fixture];
  for (const k of keys) {
    if (!WORKLOADS[k]) throw new Error(`Unknown fixture "${k}". Options: ${Object.keys(WORKLOADS).join(", ")}, all`);
  }
  if (args.includes("--worker")) {
    if (keys.length !== 1) throw new Error("--worker requires one fixture");
    await runWorker(engine, keys[0]);
    return;
  }
  if (engine === "breakdown") {
    for (const k of keys) await benchWorkload(k, diagnoseFs);
  } else if (engine === "wasm-string") {
    for (const k of keys) await runWorker(engine, k);
  } else {
    for (const k of keys) {
      const command = `${JSON.stringify(process.execPath)} ${JSON.stringify(process.argv[1])} --worker --engine ${engine} --fixture ${k}`;
      const output = `/tmp/grass-bench-${engine}-${k}.md`;
      execFileSync("hyperfine", ["--warmup", "1", "--runs", "5", "--export-markdown", output, command], { stdio: "inherit" });
    }
  }
  try { unlinkSync(NATIVE_OUT); } catch {}
}

main().catch((error) => {
  console.error(error?.stack || error);
  process.exitCode = 1;
});
