// Benchmark: grass WASM
// Usage: node bench-wasm.js
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { compileString } from "../crates/lib/pkg-publish/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const loadPaths = [resolve(__dirname, "packages")];

compileString('@use "uswds";', { loadPaths, quiet: true });
