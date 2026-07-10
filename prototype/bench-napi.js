// Benchmark: grass napi-rs (native Node.js addon)
// Usage: node bench-napi.js
import { createRequire } from "module";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));
const napi = require("../crates/napi/grass.darwin-arm64.node");

const loadPaths = [resolve(__dirname, "packages")];

napi.compile("packages/uswds/_index-direct.scss", {
  loadPaths,
  quiet: true,
  charset: false,
});
