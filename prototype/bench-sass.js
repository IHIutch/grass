// Benchmark: sass-embedded (dart-sass)
// Usage: node bench-sass.js
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import * as sass from "sass-embedded";

const __dirname = dirname(fileURLToPath(import.meta.url));
const loadPaths = [resolve(__dirname, "packages")];

sass.compileString('@use "uswds";', { loadPaths, quietDeps: true });
