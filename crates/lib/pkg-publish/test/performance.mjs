// Informational WASM performance protocol. This is deliberately separate from
// `npm test`: timings are useful for regression review, but are not stable
// enough to be a correctness gate across hosts or build profiles.
import assert from "assert";
import { execFileSync, spawnSync } from "child_process";
import { createHash } from "crypto";
import { readFileSync, rmSync, writeFileSync, mkdtempSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);

if (process.argv[2] !== "--worker") {
  const result = spawnSync(process.execPath, [__filename, "--worker"], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, GRASS_FORCE_WASM: "1" },
    encoding: "utf8",
  });
  process.stdout.write(result.stdout || "");
  if (result.status !== 0) process.exit(result.status ?? 1);
  process.exit(0);
}

assert.equal(process.env.GRASS_FORCE_WASM, "1");
const grass = await import("../index.js");
assert.equal(grass.SassNumber, undefined);

function toolVersion(command, args) {
  try {
    return execFileSync(command, args, { encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function percentile(values, p) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1)];
}

const samples = Math.max(3, Number.parseInt(process.env.GRASS_WASM_PERF_SAMPLES || "5", 10));
const warmups = Math.max(1, Number.parseInt(process.env.GRASS_WASM_PERF_WARMUPS || "1", 10));
const repeatedRules = Array.from(
  { length: 600 },
  (_, i) => `.component-${i} { color: ${i % 2 ? "#123456" : "#abcdef"}; padding: ${i % 8}px; }`,
).join("\n");
const stringSource = `$gap: 8px;\n${repeatedRules}\n.page { margin: $gap; }\n`;

const fixtureDir = mkdtempSync(join(tmpdir(), "grass-wasm-perf-"));
const entryPath = join(fixtureDir, "entry.scss");
const tokensPath = join(fixtureDir, "_tokens.scss");
const componentsPath = join(fixtureDir, "_components.scss");
writeFileSync(tokensPath, "$gap: 8px;\n$accent: #123456;\n");
writeFileSync(
  componentsPath,
  `@use "tokens" as t;\n${repeatedRules.replaceAll("#123456", "t.$accent")}\n`,
);
writeFileSync(entryPath, '@use "components";\n.page { margin: 8px; }\n');

const wasmPath = join(fileURLToPath(new URL("..", import.meta.url)), "grass_bg.wasm");
const wasmBytes = readFileSync(wasmPath);
const workloads = [
  { name: "compileString", run: () => grass.compileString(stringSource).css },
  { name: "compileFileWithImports", run: () => grass.compile(entryPath).css },
];

try {
  const measurements = [];
  for (const workload of workloads) {
    for (let i = 0; i < warmups; i++) workload.run();

    const runs = [];
    for (let i = 0; i < samples; i++) {
      const before = process.memoryUsage();
      const started = performance.now();
      const css = workload.run();
      const elapsedMs = performance.now() - started;
      const after = process.memoryUsage();
      runs.push({
        elapsedMs: Number(elapsedMs.toFixed(3)),
        outputBytes: Buffer.byteLength(css),
        rssBeforeBytes: before.rss,
        rssAfterBytes: after.rss,
        heapUsedAfterBytes: after.heapUsed,
      });
    }

    measurements.push({
      name: workload.name,
      samples: runs,
      medianMs: Number(median(runs.map((run) => run.elapsedMs)).toFixed(3)),
      p95Ms: Number(percentile(runs.map((run) => run.elapsedMs), 0.95).toFixed(3)),
      peakRssBytes: Math.max(...runs.flatMap((run) => [run.rssBeforeBytes, run.rssAfterBytes])),
      peakHeapUsedBytes: Math.max(...runs.map((run) => run.heapUsedAfterBytes)),
    });
  }

  console.log(JSON.stringify({
    kind: "grass-wasm-performance",
    forcedWasm: true,
    node: process.version,
    platform: `${process.platform}-${process.arch}`,
    rustc: toolVersion("rustc", ["--version"]),
    wasmPack: toolVersion("wasm-pack", ["--version"]),
    wasmBytes: wasmBytes.length,
    wasmSha256: createHash("sha256").update(wasmBytes).digest("hex"),
    samples,
    warmups,
    measurements,
  }, null, 2));
} finally {
  rmSync(fixtureDir, { recursive: true, force: true });
}
