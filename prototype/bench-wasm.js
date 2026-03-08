// Benchmark: grass WASM
// Usage: node bench-wasm.js
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { compile, compileAsync } from "../crates/lib/pkg-publish/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const loadPaths = [resolve(__dirname, "packages")];

compile("packages/uswds/_index-direct.scss", {
    loadPaths,
    quiet: true,
    charset: false,
});
