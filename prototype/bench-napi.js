// Benchmark: grass napi-rs (native Node.js addon)
// Usage: node bench-napi.js
import { createRequire } from "module";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { writeFileSync, unlinkSync } from "fs";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));
const napi = require("../crates/napi/grass.darwin-arm64.node");

const loadPaths = [resolve(__dirname, "packages")];
const benchFile = resolve(__dirname, "_bench_uswds.scss");
writeFileSync(benchFile, '@use "uswds";');

try {
  napi.compile(benchFile, { loadPaths, quiet: true });
} finally {
  try { unlinkSync(benchFile); } catch {}
}
