// Benchmark: WASM compile time excluding Node.js startup
// Simulates what matters for Workers: pure compile time
import { readFileSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { compile } from "../crates/lib/pkg-publish/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const loadPaths = [resolve(__dirname, "packages")];

// Warmup
compile("packages/uswds/_index-direct.scss", { loadPaths, quiet: true });

// Timed runs
const times = [];
for (let i = 0; i < 5; i++) {
  const start = performance.now();
  compile("packages/uswds/_index-direct.scss", { loadPaths, quiet: true });
  times.push(performance.now() - start);
}

times.sort((a, b) => a - b);
const median = times[Math.floor(times.length / 2)];
console.log(`Runs: ${times.map(t => t.toFixed(0) + "ms").join(", ")}`);
console.log(`WASM compile (median, no startup): ${median.toFixed(0)}ms`);
