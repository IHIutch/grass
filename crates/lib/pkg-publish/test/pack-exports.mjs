// Static manifest gate: every file package.json's `exports`/`types`/`main`
// promise to consumers must actually be present in the packed tarball.
// This is the test class that would have caught the ihiutch-grass@0.14.1
// exports-dangling-for-5-files breakage — verified against the REAL
// `npm pack` output, not just the source tree (files can be present on
// disk but excluded by .gitignore/`files` field mismatches).
import assert from "assert";
import { execFileSync } from "child_process";
import { readFileSync } from "fs";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const pkgDir = join(__dirname, "..");

const pkg = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8"));

// Walk exports (and any nested condition object) collecting every string
// leaf — these are the paths package.json promises resolve to.
function collectExportPaths(node, out) {
  if (typeof node === "string") {
    out.add(node.replace(/^\.\//, ""));
  } else if (node && typeof node === "object") {
    for (const value of Object.values(node)) collectExportPaths(value, out);
  }
}

const required = new Set();
collectExportPaths(pkg.exports, required);
if (pkg.types) required.add(pkg.types.replace(/^\.\//, ""));
if (pkg.main) required.add(pkg.main.replace(/^\.\//, ""));
// Not surfaced directly in `exports`, but loaded at runtime by
// index.js/browser.js/workers.js (import "./grass.js") — required all the
// same for the package to actually work.
required.add("grass.js");
required.add("README.md");
required.add("LICENSE");

const packOutput = execFileSync("npm", ["pack", "--dry-run", "--json"], {
  cwd: pkgDir,
  encoding: "utf8",
});
const [{ files }] = JSON.parse(packOutput);
const packed = new Set(files.map((f) => f.path));

const missing = [...required].filter((f) => !packed.has(f));
assert.deepEqual(missing, [], `files required by package.json are missing from the npm pack manifest: ${missing.join(", ")}`);

console.log(`pack-exports ok: ${required.size} required files all present in the pack manifest`);
