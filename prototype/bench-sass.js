// Benchmark: sass-embedded (dart-sass)
// Usage: node bench-sass.js
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { writeFileSync } from "fs";
import * as sass from "sass-embedded";

writeFileSync("/tmp/_grass_bench.scss", '@use "uswds";');

const __dirname = dirname(fileURLToPath(import.meta.url));
const loadPaths = [resolve(__dirname, "packages")];

await sass.compileAsync('packages/uswds/_index-direct.scss', {
    loadPaths: loadPaths,
    logger: sass.Logger.silent,
    charset: false,
});