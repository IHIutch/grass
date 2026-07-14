// Does the shipped grass WASM surface beat the only dart-sass build available
// in browsers and Workers? Pure-JS `sass` is the right peer because it shares
// WASM's runtime class; sass-embedded is a native subprocess and cannot run
// in those runtimes. All four engines are measured warm and in-process.

import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { resolve } from "node:path";
import * as sassEmbedded from "sass-embedded";
import * as sass from "sass";
import { resolveFixture, REPO_ROOT } from "../fixtures/resolve.mjs";

const WARMUPS = 2;
const REPS = 5;
const require = createRequire(import.meta.url);

const fixture = resolveFixture("bootstrap");
const loadPaths = [fixture.loadPath];

// index.js reads GRASS_FORCE_WASM while the module is loaded. This must happen
// before importing the package surface, even though the N-API binding is also
// present in this worktree.
process.env.GRASS_FORCE_WASM = "1";
const grassWasm = await import(resolve(REPO_ROOT, "crates/lib/pkg-publish/index.js"));
if (grassWasm.SassNumber !== undefined) {
  throw new Error("FAIL: GRASS_FORCE_WASM did not select the WASM path (SassNumber was defined)");
}

const grassNapi = require(resolve(REPO_ROOT, "crates/napi/grass.darwin-arm64.node"));

const engines = [
  {
    name: "grass-WASM",
    compile() {
      return grassWasm.compile(fixture.entry, { loadPaths, quiet: true });
    },
  },
  {
    name: "grass-napi",
    compile() {
      return grassNapi.compile(fixture.entry, { loadPaths, quiet: true });
    },
  },
  {
    name: "sass-embedded",
    compile() {
      return sassEmbedded.compile(fixture.entry, {
        loadPaths,
        quietDeps: true,
        logger: sassEmbedded.Logger.silent,
      });
    },
  },
  {
    name: "pure-JS-sass",
    compile() {
      return sass.compile(fixture.entry, {
        loadPaths,
        quietDeps: true,
        logger: sass.Logger.silent,
      });
    },
  },
];

function median(values) {
  return [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
}

function assertByteIdentical(expected, actual, label) {
  if (expected.css !== actual.css) {
    const expectedBytes = Buffer.byteLength(expected.css);
    const actualBytes = Buffer.byteLength(actual.css);
    let offset = 0;
    const limit = Math.min(expected.css.length, actual.css.length);
    while (offset < limit && expected.css[offset] === actual.css[offset]) offset++;
    throw new Error(`FAIL: ${label} CSS differs (${expectedBytes}/${actualBytes} bytes, first difference at ${offset})`);
  }
}

const outputs = new Map();
for (const engine of engines) {
  outputs.set(engine.name, engine.compile());
}
const expected = outputs.get("sass-embedded");
for (const engine of engines) {
  assertByteIdentical(expected, outputs.get(engine.name), engine.name);
}

function measure(engine) {
  const times = [];
  for (let i = 0; i < WARMUPS + REPS; i++) {
    const started = performance.now();
    const output = engine.compile();
    const elapsed = performance.now() - started;
    assertByteIdentical(expected, output, `${engine.name} measured compile`);
    if (i >= WARMUPS) times.push(elapsed);
  }
  return median(times);
}

const medians = new Map(engines.map((engine) => [engine.name, measure(engine)]));
const wasmMs = medians.get("grass-WASM");
const embeddedMs = medians.get("sass-embedded");
const pureJsMs = medians.get("pure-JS-sass");

console.log("Bootstrap warm in-process comparison (2 warmups, 5 measured reps, medians)");
console.log("WASM path assertion: PASS (GRASS_FORCE_WASM=1; SassNumber is undefined)");
console.log("Byte-equality assertion: PASS (all four engines)");
console.log("");
console.log("engine              median ms  WASM vs embedded  WASM vs pure-JS");
for (const engine of engines) {
  const medianMs = medians.get(engine.name);
  const embeddedRatio = embeddedMs / medianMs;
  const pureJsRatio = pureJsMs / medianMs;
  console.log(
    `${engine.name.padEnd(19)} ${medianMs.toFixed(1).padStart(9)}  ${embeddedRatio.toFixed(2).padStart(16)}x  ${pureJsRatio.toFixed(2).padStart(15)}x`,
  );
}
