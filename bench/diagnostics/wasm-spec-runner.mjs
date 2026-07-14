// wasm-spec-runner.mjs
//
// Runs the FULL sass-spec corpus through a single, persistent wasm
// instance, sequentially, one compile() call per test -- unlike
// run-sass-specs.py (which spawns a fresh native process per test, so it
// can never exercise "does compile N+1 see corrupted state left over by
// compile N's allocator reset").
//
// This is the multi-compile-stress correctness gate for todo #282 (the
// wasm bump allocator): a reset bug would show up here as spec failures
// that don't occur when each test is compiled in isolation, since with
// thousands of real, structurally diverse Sass programs compiled
// back-to-back in one instance, any allocator misbehavior that clobbers
// live per-compile data has many chances to manifest.
//
// Mirrors run-sass-specs.py's HRX parsing/extraction/pass-fail semantics
// closely enough for a like-for-like comparison against its "13762/13801"
// baseline, but drives everything through crates/lib/pkg-publish's wasm
// bindings instead of shelling out to the native binary.
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync, existsSync, readdirSync, statSync, rmSync, realpathSync } from "fs";
import { join, dirname, relative, resolve, basename } from "path";
import { tmpdir } from "os";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const SPEC_DIR = resolve(__dirname, "..", "..", "sass-spec", "spec");

function parseHrx(path) {
  const content = readFileSync(path, "utf8");
  const entries = content.split(/^<===>\s*\n?/m);
  const files = {};
  for (let entry of entries) {
    entry = entry.trim();
    if (!entry || entry.startsWith("=")) continue;
    const lines = entry.split("\n");
    const filename = lines[0].trim();
    if (filename.startsWith("=")) continue;
    const bodyLines = lines.slice(1).filter((l) => !l.trim().startsWith("====="));
    let body = bodyLines.join("\n");
    body = body.trim() ? body.replace(/\n+$/, "") + "\n" : "";
    files[filename] = body;
  }
  return files;
}

function extractTests(files) {
  const tests = [];
  const companions = {};
  const inputFiles = Object.keys(files)
    .filter((f) => f.endsWith("/input.scss") || f === "input.scss" || f.endsWith("/input.sass") || f === "input.sass")
    .sort();

  const testPrefixes = new Set();
  for (const f of inputFiles) {
    testPrefixes.add(f.replace(/input\.s[ac]ss$/, ""));
  }

  for (const [fname, content] of Object.entries(files)) {
    if (!(fname.endsWith(".scss") || fname.endsWith(".sass") || fname.endsWith(".css"))) continue;
    if (fname.endsWith("/input.scss") || fname === "input.scss") continue;
    if (fname.endsWith("/input.sass") || fname === "input.sass") continue;
    companions[fname] = content;
  }

  for (const inputFile of inputFiles) {
    const prefix = inputFile.replace(/input\.s[ac]ss$/, "");
    const outputFile = `${prefix}output.css`;
    const errorFile = `${prefix}error`;
    const optionsFile = `${prefix}options.yml`;

    if (files[optionsFile] && files[optionsFile].includes(":todo:")) continue;

    if (files[outputFile] !== undefined) {
      tests.push({
        name: prefix.replace(/\/$/, "") || "root",
        inputPath: inputFile,
        input: files[inputFile],
        expected: files[outputFile],
        type: "success",
      });
    } else if (files[errorFile] !== undefined) {
      tests.push({
        name: prefix.replace(/\/$/, "") || "root",
        inputPath: inputFile,
        input: files[inputFile],
        type: "error",
      });
    }
  }

  return { tests, companions };
}

function normalize(s) {
  return s && s.trim() ? s.replace(/\n+$/, "") + "\n" : "";
}

function* walk(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const full = join(dir, e.name);
    if (e.isDirectory()) yield* walk(full);
    else yield full;
  }
}

function collectHrxFiles(root) {
  const out = [];
  for (const f of walk(root)) {
    if (f.endsWith(".hrx")) out.push(f);
  }
  return out.sort();
}

function collectDiskTests(root, hrxDirs) {
  const out = [];
  const seen = new Set();
  for (const f of walk(root)) {
    if (basename(f) === "input.scss" || basename(f) === "input.sass") {
      const parent = dirname(f);
      if (hrxDirs.has(parent) || seen.has(parent)) continue;
      seen.add(parent);
      out.push(parent);
    }
  }
  return out.sort();
}

// --- fs callbacks for the wasm build (mirrors pkg-publish/index.js) ---
const fsCallbacks = {
  is_file(path) {
    try { return statSync(path).isFile(); } catch { return false; }
  },
  is_dir(path) {
    try { return statSync(path).isDirectory(); } catch { return false; }
  },
  read(path) {
    return readFileSync(path);
  },
  canonicalize(path) {
    return realpathSync(path);
  },
  resolve_first_existing(candidates) {
    for (const p of candidates) {
      try { if (statSync(p).isFile()) return p; } catch {}
    }
    return null;
  },
  readdirSync(dir) {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return [];
    }
    return entries.map((e) => {
      let kind = "o";
      try {
        if (e.isFile()) kind = "f";
        else if (e.isDirectory()) kind = "d";
      } catch {}
      return kind + e.name;
    });
  },
};

async function main() {
  const { initSync, compile_file: compileFile } = await import("../crates/lib/pkg-publish/grass.js");
  const wasmBytes = readFileSync(resolve(__dirname, "../../crates/lib/pkg-publish/grass_bg.wasm"));
  initSync({ module: wasmBytes });

  const categoryArg = process.argv.find((a, i) => i >= 2 && !a.startsWith("--"));
  let root = SPEC_DIR;
  if (categoryArg) root = join(SPEC_DIR, categoryArg);

  const hrxFiles = collectHrxFiles(root);
  const hrxDirs = new Set(hrxFiles.map((h) => dirname(h)));
  const diskTests = collectDiskTests(root, hrxDirs);

  let total = 0;
  let passed = 0;
  const catStats = {};
  const failures = [];

  function category(name) {
    const rel = name.includes("::") ? name.split("::")[0] : name;
    return rel.split("/")[0] || "unknown";
  }

  function record(name, ok, detail) {
    total++;
    if (ok) passed++;
    const cat = category(name);
    catStats[cat] = catStats[cat] || [0, 0];
    catStats[cat][1]++;
    if (ok) catStats[cat][0]++;
    else failures.push({ name, detail });
  }

  let processed = 0;
  const t0 = performance.now();

  for (const hrxPath of hrxFiles) {
    const hrxRel = relative(SPEC_DIR, hrxPath);
    const hrxBase = hrxRel.replace(/\.hrx$/, "");
    const files = parseHrx(hrxPath);
    const { tests, companions } = extractTests(files);
    if (tests.length === 0) continue;

    const tmp = mkdtempSync(join(tmpdir(), "grass_wasm_spec_"));
    try {
      for (const [compPath, content] of Object.entries(companions)) {
        const full = join(tmp, hrxBase, compPath);
        mkdirSync(dirname(full), { recursive: true });
        writeFileSync(full, content);
      }
      // copy on-disk siblings (mirrors run-sass-specs.py)
      const hrxParent = dirname(hrxPath);
      const hrxParentRel = dirname(hrxRel);
      const destParent = join(tmp, hrxParentRel);
      mkdirSync(destParent, { recursive: true });
      for (const sib of readdirSync(hrxParent, { withFileTypes: true })) {
        if (!sib.isFile()) continue;
        if (!/\.(scss|sass|css)$/.test(sib.name)) continue;
        const dest = join(destParent, sib.name);
        if (!existsSync(dest)) {
          writeFileSync(dest, readFileSync(join(hrxParent, sib.name)));
        }
      }

      for (const test of tests) {
        const testName = `${hrxRel}::${test.name}`;
        const inputFull = join(tmp, hrxBase, test.inputPath);
        mkdirSync(dirname(inputFull), { recursive: true });
        writeFileSync(inputFull, test.input);

        const loadPaths = [tmp, SPEC_DIR, hrxParent];
        let threw = false;
        let css = null;
        let errMsg = null;
        try {
          const res = compileFile(inputFull, loadPaths, "expanded", false, false, false, fsCallbacks);
          css = res.css;
        } catch (e) {
          threw = true;
          errMsg = typeof e === "string" ? e : e.message || String(e);
        }
        processed++;

        if (test.type === "success") {
          if (threw) {
            record(testName, false, `ERROR: ${String(errMsg).slice(0, 200)}`);
          } else {
            const actual = normalize(css);
            const expected = normalize(test.expected);
            record(testName, actual === expected, threw ? errMsg : `mismatch`);
          }
        } else {
          record(testName, threw, threw ? null : `Expected error, got success`);
        }
      }
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  }

  // disk tests
  for (const dir of diskTests) {
    let inputFile = join(dir, "input.scss");
    if (!existsSync(inputFile)) inputFile = join(dir, "input.sass");
    const optionsFile = join(dir, "options.yml");
    if (existsSync(optionsFile) && readFileSync(optionsFile, "utf8").includes(":todo:")) continue;

    const outputFile = join(dir, "output.css");
    const errorFile = join(dir, "error");
    const relPath = relative(SPEC_DIR, dir);

    const loadPaths = [SPEC_DIR];
    let threw = false;
    let css = null;
    let errMsg = null;
    try {
      const res = compileFile(inputFile, loadPaths, "expanded", false, false, false, fsCallbacks);
      css = res.css;
    } catch (e) {
      threw = true;
      errMsg = typeof e === "string" ? e : e.message || String(e);
    }
    processed++;

    if (existsSync(outputFile)) {
      if (threw) {
        record(relPath, false, `ERROR: ${String(errMsg).slice(0, 200)}`);
      } else {
        const actual = normalize(css);
        const expected = normalize(readFileSync(outputFile, "utf8"));
        record(relPath, actual === expected, "mismatch");
      }
    } else if (existsSync(errorFile)) {
      record(relPath, threw, threw ? null : "Expected error, got success");
    }
  }

  const t1 = performance.now();

  console.log(`\nProcessed ${processed} compiles through ONE wasm instance in ${((t1 - t0) / 1000).toFixed(1)}s`);
  console.log(`\nResults: ${passed}/${total} passed (${((100 * passed) / total).toFixed(1)}%)`);
  console.log(`Failed: ${total - passed}`);
  console.log("\nBy category:");
  for (const cat of Object.keys(catStats).sort()) {
    const [p, t] = catStats[cat];
    console.log(`  ${cat}: ${p}/${t} (${((100 * p) / t).toFixed(0)}%) [${t - p} failures]`);
  }

  if (process.argv.includes("--failures")) {
    const limit = 40;
    console.log(`\n--- Failures (showing up to ${limit}/${failures.length}) ---`);
    for (const f of failures.slice(0, limit)) {
      console.log(`[FAIL] ${f.name}: ${f.detail || ""}`);
    }
  }

  writeFileSync(resolve(__dirname, "wasm-spec-result.json"), JSON.stringify({ total, passed, failures: failures.map(f => f.name) }, null, 2));
}

main();
