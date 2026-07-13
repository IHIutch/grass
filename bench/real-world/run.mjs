import { createHash } from "crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, openSync, closeSync } from "fs";
import { join, resolve } from "path";
import { spawnSync } from "child_process";
import { performance } from "perf_hooks";

const DIR = new URL(".", import.meta.url).pathname;
const ROOT = resolve(DIR, "../..");
const CACHE = join(DIR, ".cache");
const MANIFEST = JSON.parse(readFileSync(join(DIR, "manifest.json"), "utf8"));
const GRASS = process.env.GRASS_BINARY || join(ROOT, "target/release/grass");
const RUNS = 5;
const WARMUPS = 2;

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

function firstDiff(a, b) {
  const x = readFileSync(a); const y = readFileSync(b);
  const n = Math.min(x.length, y.length);
  let i = 0; while (i < n && x[i] === y[i]) i++;
  return `${x.length}/${y.length}@${i}`;
}

function oneProject(project) {
  const record = { name: project.name, commit: project.commit, status: "ERROR", grassMedianMs: null, dartMedianMs: null, ratio: null, signature: null, error: null };
  const work = join(DIR, `.tmp-${project.name}-${process.pid}`);
  mkdirSync(work, { recursive: true });
  try {
    const repo = ensureRepo(project);
    const prep = join(work, "prep");
    prepare(project, repo, prep);
    const entry = project.entry.startsWith("@prep/") ? join(prep, project.entry.slice("@prep/".length)) : resolve(repo, project.entry);
    const paths = loadPaths(project, repo, prep);
    if (!existsSync(entry)) throw new Error(`entry not found: ${project.entry}`);
    const grassTimes = []; const dartTimes = [];
    let grassError = null; let dartError = null;
    const grassOut = join(work, "grass.css"); const dartOut = join(work, "dart.css");
    const grassErr = join(work, "grass.stderr"); const dartErr = join(work, "dart.stderr");
    for (let i = 0; i < WARMUPS + RUNS; i++) {
      const g = compile("grass", entry, paths, grassOut, grassErr, repo);
      if (g.result.status !== 0) grassError ||= `grass exit ${g.result.status}: ${errorSummary(grassErr)}`;
      else if (i >= WARMUPS) grassTimes.push(g.ms);
      const d = compile("dart", entry, paths, dartOut, dartErr, repo);
      if (d.result.status !== 0) dartError ||= `dart-sass exit ${d.result.status}: ${errorSummary(dartErr)}`;
      else if (i >= WARMUPS) dartTimes.push(d.ms);
      if (i === WARMUPS && !grassError && !dartError && !readFileSync(grassOut).equals(readFileSync(dartOut))) {
        record.status = "DIFF";
        record.signature = firstDiff(grassOut, dartOut);
      }
    }
    if (grassError || dartError) {
      record.error = [grassError, dartError].filter(Boolean).join("; ");
      return record;
    }
    const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
    record.grassMedianMs = Number(median(grassTimes).toFixed(1));
    record.dartMedianMs = Number(median(dartTimes).toFixed(1));
    record.ratio = Number((record.dartMedianMs / record.grassMedianMs).toFixed(2));
    if (record.status !== "DIFF") record.status = "PASS";
  } catch (error) {
    record.error = String(error?.message || error);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  return record;
}

function writeResults(records) {
  const rows = ["# Real-world parity results", "", "Generated by `node bench/real-world/run.mjs all` against `npx -y sass@1.101.0`.", "", "| Project | Commit | Status | Grass median (ms) | Dart median (ms) | Dart/Grass | Signature/error |", "|---|---|---:|---:|---:|---:|---|"];
  for (const r of records) rows.push(`| ${r.name} | ${r.commit.slice(0, 12)} | ${r.status} | ${r.grassMedianMs ?? "—"} | ${r.dartMedianMs ?? "—"} | ${r.ratio ?? "—"} | ${r.signature || r.error || ""} |`);
  rows.push("", `Runs: ${RUNS} measured after ${WARMUPS} warmups per engine; raw byte comparison; stderr captured to per-run files; no source maps; quiet machine recommended.`);
  writeFileSync(join(DIR, "results.md"), rows.join("\n") + "\n");
}

function ratchet(records) {
  const path = join(DIR, "BASELINE.json");
  const current = Object.fromEntries(records.map((r) => [r.name, r]));
  if (!existsSync(path) || process.env.UPDATE_BASELINE === "1") {
    writeFileSync(path, JSON.stringify({ schema: 1, sass: "1.101.0", projects: current }, null, 2) + "\n");
    return 0;
  }
  const baseline = JSON.parse(readFileSync(path, "utf8")).projects || {};
  let regressions = 0;
  for (const r of records) {
    const b = baseline[r.name];
    if (!b) { console.log(`RATCHET: new project ${r.name}; consider updating BASELINE.json`); continue; }
    if (b.status === "PASS" && r.status !== "PASS") { console.error(`REGRESSION: ${r.name} ${b.status} -> ${r.status}`); regressions++; }
    if (r.status === "PASS" && b.grassMedianMs && r.grassMedianMs > b.grassMedianMs * 1.2) console.log(`RATCHET: ${r.name} is slower than baseline; review before updating baseline`);
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
