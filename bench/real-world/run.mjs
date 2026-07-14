import { createHash } from "crypto";
import { createRequire } from "module";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, openSync, closeSync, readdirSync, statSync } from "fs";
import { join, resolve } from "path";
import { spawnSync } from "child_process";
import { performance } from "perf_hooks";
import { pathToFileURL } from "url";

delete process.env.NO_COLOR;
process.env.FORCE_COLOR = "0";
const sassEmbedded = await import("sass-embedded");

const DIR = new URL(".", import.meta.url).pathname;
const ROOT = resolve(DIR, "../..");

// The WASM leg measures the shipped pkg-publish surface. GRASS_FORCE_WASM is
// read once at module load (index.js), so it must be set before this import;
// SassNumber is only defined when the native binding loaded, so it doubles as
// proof that these timings actually go through WASM.
process.env.GRASS_FORCE_WASM = "1";
const grassWasm = await import(pathToFileURL(join(ROOT, "crates/lib/pkg-publish/index.js")).href);
delete process.env.GRASS_FORCE_WASM;
if (grassWasm.SassNumber !== undefined) {
  console.error("FATAL: pkg-publish loaded the native binding despite GRASS_FORCE_WASM=1; refusing to label napi timings as WASM");
  process.exit(2);
}

// grass_bg.wasm is gitignored and does NOT rebuild on checkout or merge; a
// stale binary has repeatedly produced false parity failures. Warn loudly.
{
  const wasmMtime = statSync(join(ROOT, "crates/lib/pkg-publish/grass_bg.wasm")).mtimeMs;
  const srcDir = join(ROOT, "crates/compiler/src");
  const newestSrc = Math.max(
    ...readdirSync(srcDir, { recursive: true, encoding: "utf8" }).map((f) => {
      try { return statSync(join(srcDir, f)).mtimeMs; } catch { return 0; }
    }),
  );
  if (wasmMtime < newestSrc) {
    console.error("WARNING: crates/lib/pkg-publish/grass_bg.wasm is OLDER than crates/compiler/src. It is gitignored and never rebuilds on checkout/merge — rebuild it with wasm-pack (see bench/README.md) before trusting any WASM result.");
  }
}
const CACHE = process.env.CORPUS_CACHE || join(DIR, ".cache");
const MANIFEST = JSON.parse(readFileSync(join(DIR, "manifest.json"), "utf8"));
const GRASS = process.env.GRASS_BINARY || join(ROOT, "target/release/grass");
const require = createRequire(import.meta.url);
const grassNapi = require(resolve(ROOT, "crates/napi/grass.darwin-arm64.node"));
const RUNS = 5;
const WARMUPS = 2;
const SILENCED_DEPRECATIONS = [
  "call-string", "moz-document", "relative-canonical", "new-global",
  "color-module-compat", "slash-div", "bogus-combinators", "strict-unary",
  "function-units", "duplicate-var-flags", "null-alpha", "abs-percent",
  "fs-importer-cwd", "feature-exists", "color-4-api", "color-functions",
  "legacy-js-api", "global-builtin",
  "compile-string-relative-url", "misplaced-rest",
  "with-private", "if-function", "function-name", "adjacent-compounds",
];

function hash(s) { return createHash("sha256").update(s).digest("hex").slice(0, 16); }

function relativizePath(value) {
  const root = ROOT.replaceAll("\\", "/");
  return value.replaceAll("./" + root + "/", "").replaceAll(root + "/", "");
}

function errorSummary(path) {
  const lines = readFileSync(path, "utf8").split("\n").map((line) => line.trim()).filter(Boolean);
  const errorIndex = lines.findIndex((line) => /\bError:/.test(line) && !/WARNING|DEPRECATION/.test(line));
  if (errorIndex < 0) return lines.find((line) => !/WARNING|DEPRECATION|More info|Suggestion/.test(line)) || "no Error line captured";
  const context = lines.slice(errorIndex + 1).find((line) => /(?:^|\s)[^ ]+:\d+(?::\d+)?/.test(line));
  return [lines[errorIndex], context].filter(Boolean).map(relativizePath).join(" | ");
}

function run(command, args, cwd, stdout, stderr, timeout = 180000) {
  const out = openSync(stdout, "w");
  const err = openSync(stderr, "w");
  const started = performance.now();
  const result = spawnSync(command, args, { cwd, stdio: ["ignore", out, err], timeout });
  closeSync(out);
  closeSync(err);
  return { result, ms: performance.now() - started };
}

function loadPaths(project, repo, prepDir) {
  return (project.loadPaths || []).map((p) => {
    if (p === "@node_modules") return join(repo, "node_modules");
    if (p === "@prep") return prepDir;
    return resolve(repo, p);
  });
}

function prepare(project, repo, prepDir) {
  mkdirSync(prepDir, { recursive: true });
  if (!project.prep) return;
  if (project.prep.content !== undefined) {
    writeFileSync(join(prepDir, project.prep.file), project.prep.content);
    return;
  }
  let content = readFileSync(resolve(repo, project.prep.from), "utf8");
  content = content.replace(/^---[\s\S]*?---\s*/m, "");
  const liquidValue = project.prep.liquidDefault || "";
  content = content.replace(/\{\{[\s\S]*?\}\}/g, liquidValue).replace(/\{%[\s\S]*?%\}/g, "");
  writeFileSync(join(prepDir, project.prep.file), content);
}

function ensureRepo(project) {
  mkdirSync(CACHE, { recursive: true });
  const repo = join(CACHE, project.name);
  if (!existsSync(join(repo, ".git"))) {
    const clone = spawnSync("git", ["clone", "--depth=1", "--no-tags", project.git, repo], { stdio: "ignore", timeout: 180000 });
    if (clone.status !== 0) throw new Error("shallow clone failed");
  }
  let head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8" }).stdout.trim();
  if (head !== project.commit) {
    const fetched = spawnSync("git", ["fetch", "--depth=1", "origin", project.commit], { cwd: repo, stdio: "ignore", timeout: 180000 });
    if (fetched.status !== 0) throw new Error(`pinned commit ${project.commit} unavailable`);
    const checked = spawnSync("git", ["checkout", "--detach", project.commit], { cwd: repo, stdio: "ignore" });
    if (checked.status !== 0) throw new Error("checkout of pinned commit failed");
    head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8" }).stdout.trim();
  }
  if (head !== project.commit) throw new Error(`checkout is ${head}, expected ${project.commit}`);
  const lock = existsSync(join(repo, "package-lock.json"));
  if (lock) {
    const npm = spawnSync("npm", ["ci", "--ignore-scripts", "--no-audit", "--no-fund"], { cwd: repo, stdio: "ignore", timeout: 180000 });
    if (npm.status !== 0) throw new Error("npm ci failed");
  }
  return repo;
}

function compile(kind, entry, paths, out, err, cwd) {
  const include = paths.flatMap((p) => ["-I", p]);
  if (kind === "grass") {
    return run(GRASS, [entry, "--style=expanded", "--no-source-map", ...include], cwd, out, err);
  }
  return run("npx", ["-y", "sass@1.101.0", "--style=expanded", "--no-source-map", ...include, entry, out], cwd, out + ".stdout", err);
}

function compileWarm(kind, entry, paths) {
  if (kind === "grass") return grassNapi.compile(entry, { loadPaths: paths, quiet: true });
  if (kind === "wasm") return grassWasm.compile(entry, { loadPaths: paths, quiet: true });
  return sassEmbedded.compile(entry, {
    loadPaths: paths,
    quietDeps: true,
    silenceDeprecations: SILENCED_DEPRECATIONS,
    logger: sassEmbedded.Logger.silent,
  });
}

function firstDiff(a, b) {
  const x = readFileSync(a); const y = readFileSync(b);
  const n = Math.min(x.length, y.length);
  let i = 0; while (i < n && x[i] === y[i]) i++;
  return `${x.length}/${y.length}@${i}`;
}

function oneProject(project) {
  const record = { name: project.name, commit: project.commit, status: "ERROR", dartWarmMs: null, grassWarmMs: null, speedup: null, wasmWarmMs: null, wasmSpeedup: null, signature: null, error: null };
  const work = join(DIR, `.tmp-${project.name}-${process.pid}`);
  mkdirSync(work, { recursive: true });
  try {
    const repo = ensureRepo(project);
    const prep = join(work, "prep");
    prepare(project, repo, prep);
    const entry = project.entry.startsWith("@prep/") ? join(prep, project.entry.slice("@prep/".length)) : resolve(repo, project.entry);
    const paths = loadPaths(project, repo, prep);
    if (!existsSync(entry)) throw new Error(`entry not found: ${project.entry}`);
    const grassOut = join(work, "grass.css"); const dartOut = join(work, "dart.css");
    const grassErr = join(work, "grass.stderr"); const dartErr = join(work, "dart.stderr");

    // Parity is a gate: keep these CLI invocations and the raw byte comparison
    // separate from the timing leg below.
    const g = compile("grass", entry, paths, grassOut, grassErr, repo);
    const d = compile("dart", entry, paths, dartOut, dartErr, repo);
    const grassError = g.result.status !== 0 ? `grass exit ${g.result.status}: ${errorSummary(grassErr)}` : null;
    const dartError = d.result.status !== 0 ? `dart-sass exit ${d.result.status}: ${errorSummary(dartErr)}` : null;
    if (grassError || dartError) {
      record.error = [grassError, dartError].filter(Boolean).join("; ");
      return record;
    }
    if (!readFileSync(grassOut).equals(readFileSync(dartOut))) {
      record.status = "DIFF";
      record.signature = firstDiff(grassOut, dartOut);
      return record;
    }

    // Timing is a separate, warm, in-process comparison: no CLI, npx, or VM
    // startup is included in either engine's measured calls.
    const grassTimes = []; const dartTimes = [];
    for (let i = 0; i < WARMUPS + RUNS; i++) {
      const grassStart = performance.now();
      compileWarm("grass", entry, paths);
      const grassMs = performance.now() - grassStart;
      const dartStart = performance.now();
      compileWarm("dart", entry, paths);
      const dartMs = performance.now() - dartStart;
      if (i >= WARMUPS) {
        grassTimes.push(grassMs);
        dartTimes.push(dartMs);
      }
    }
    const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
    record.grassWarmMs = Number(median(grassTimes).toFixed(1));
    record.dartWarmMs = Number(median(dartTimes).toFixed(1));
    record.speedup = Number((record.dartWarmMs / record.grassWarmMs).toFixed(2));

    // WASM leg: same warm in-process method, through the shipped pkg-publish
    // surface (GRASS_FORCE_WASM=1, asserted at import). Byte-compared against
    // the dart CLI reference above; the CLI terminates output with exactly one
    // newline that the JS API omits, so strip it to compare like for like.
    let dartRef = readFileSync(dartOut);
    if (dartRef[dartRef.length - 1] === 0x0a) dartRef = dartRef.subarray(0, dartRef.length - 1);
    const wasmTimes = [];
    let wasmCss = null;
    for (let i = 0; i < WARMUPS + RUNS; i++) {
      const wasmStart = performance.now();
      const result = compileWarm("wasm", entry, paths);
      const wasmMs = performance.now() - wasmStart;
      if (wasmCss === null) wasmCss = Buffer.from(result.css, "utf8");
      if (i >= WARMUPS) wasmTimes.push(wasmMs);
    }
    if (!wasmCss.equals(dartRef)) {
      record.status = "DIFF";
      record.signature = `wasm:${wasmCss.length}/${dartRef.length}`;
      return record;
    }
    record.wasmWarmMs = Number(median(wasmTimes).toFixed(1));
    record.wasmSpeedup = Number((record.dartWarmMs / record.wasmWarmMs).toFixed(2));
    record.status = "PASS";
  } catch (error) {
    record.error = String(error?.message || error);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  return record;
}

function writeResults(records) {
  const passing = records.filter((r) => r.status === "PASS");
  const allPass = passing.length === records.length;
  const rows = [
    "# Real-world parity results",
    "",
    allPass
      ? `All ${records.length} projects compile byte-identical to dart-sass 1.101.0.`
      : `Only ${passing.length} of ${records.length} projects compile byte-identical to dart-sass 1.101.0.`,
    "",
    "Generated by `node bench/real-world/run.mjs all`; parity uses the `npx -y sass@1.101.0` CLI.",
    "",
  ];
  if (!allPass) {
    rows.push("## Failures", "", "| Project | Status | Error signature |", "|---|---|---|");
    for (const r of records.filter((r) => r.status !== "PASS")) {
      const detail = r.status === "DIFF" ? r.signature || "unknown diff" : r.error || "unknown error";
      rows.push(`| ${r.name} | ${r.status} | ${detail} |`);
    }
    rows.push("");
  }
  rows.push(
    "| Project | Commit | dart-sass (warm, ms) | grass (warm, ms) | speedup |",
    "|---|---|---:|---:|---:|",
  );
  for (const r of records) rows.push(`| ${r.name} | ${r.commit.slice(0, 12)} | ${r.dartWarmMs ?? "—"} | ${r.grassWarmMs ?? "—"} | ${r.speedup ? `${r.speedup}x` : "—"} |`);
  rows.push(
    "",
    `Timing method: warm in-process medians (${WARMUPS} warmups, ${RUNS} reps), grass via the N-API binding and dart via sass-embedded 1.100.0; neither includes process startup. Parity is separately byte-compared via the CLIs against dart-sass 1.101.0. Raw bytes, stderr capture, no source maps; quiet machine recommended.`,
  );
  const wasmPassing = records.filter((r) => r.wasmWarmMs !== null);
  rows.push(
    "",
    "## WASM engine",
    "",
    `grass's WASM build produced output byte-identical to dart-sass 1.101.0 on ${wasmPassing.length} of ${records.length} projects.`,
    "",
    "| Project | dart-sass (warm, ms) | grass WASM (warm, ms) | speedup |",
    "|---|---:|---:|---:|",
  );
  for (const r of records) rows.push(`| ${r.name} | ${r.dartWarmMs ?? "—"} | ${r.wasmWarmMs ?? "—"} | ${r.wasmSpeedup ? `${r.wasmSpeedup}x` : "—"} |`);
  rows.push(
    "",
    `WASM method: same warm in-process medians (${WARMUPS} warmups, ${RUNS} reps) against the same dart-sass (sass-embedded 1.100.0) reference times, but grass measured through the shipped \`crates/lib/pkg-publish\` JS API with \`GRASS_FORCE_WASM=1\` (the run aborts if the native binding loads instead). Its output is byte-compared against the dart-sass 1.101.0 CLI reference from the parity leg.`,
  );
  writeFileSync(join(DIR, "results.md"), rows.join("\n") + "\n");
}

function ratchet(records) {
  const path = join(DIR, "BASELINE.json");
  const current = Object.fromEntries(records.map((r) => [r.name, r]));
  if (!existsSync(path) || process.env.UPDATE_BASELINE === "1") {
    writeFileSync(path, JSON.stringify({ schema: 3, paritySass: "1.101.0", timingSass: "sass-embedded 1.100.0", projects: current }, null, 2) + "\n");
    return 0;
  }
  const baseline = JSON.parse(readFileSync(path, "utf8")).projects || {};
  let regressions = 0;
  for (const r of records) {
    const b = baseline[r.name];
    if (!b) { console.log(`RATCHET: new project ${r.name}; consider updating BASELINE.json`); continue; }
    if (b.status === "PASS" && r.status !== "PASS") { console.error(`REGRESSION: ${r.name} ${b.status} -> ${r.status}`); regressions++; }
    if (r.status === "PASS" && b.grassWarmMs && r.grassWarmMs > b.grassWarmMs * 1.2) console.log(`RATCHET: ${r.name} is slower than baseline; review before updating BASELINE.json`);
    if (b.status !== "PASS" && r.status === "PASS") console.log(`RATCHET: ${r.name} improved to PASS; update BASELINE.json`);
  }
  return regressions;
}

const args = process.argv.slice(2);
const filter = args[0] === "--project" ? args[1] : args[0];
const projects = filter && filter !== "all" ? MANIFEST.projects.filter((p) => p.name === filter) : MANIFEST.projects;
if (projects.length === 0) { console.error(`Unknown project: ${filter}`); process.exit(2); }
if (!existsSync(GRASS)) { console.error(`Grass binary not found: ${GRASS}`); process.exit(2); }
const records = projects.map(oneProject);
writeResults(records);
const regressions = ratchet(records);
for (const r of records) console.log(`${r.name}: ${r.status}${r.error ? ` (${r.error})` : ""}`);
process.exitCode = regressions ? 1 : 0;
