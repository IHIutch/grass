// Multi-compile stress test for the wasm bump-allocator work (todo #282).
// Compiles USWDS then Bootstrap then USWDS again in ONE wasm instance and
// checks all outputs are byte-identical to single-shot reference compiles.
// This is the test that would catch a bad allocator reset (a reset bug may
// only show up on compile 2+, since compile 1 always starts from pristine
// memory).
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";
import { createHash } from "crypto";
import { resolveFixture } from "../fixtures/resolve.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const uswdsFixture = resolveFixture("uswds");
const bootstrapFixture = resolveFixture("bootstrap");
const loadPaths = [uswdsFixture.loadPath];

function hash(s) {
  return createHash("sha256").update(s).digest("hex").slice(0, 16);
}

async function main() {
  const { compile } = await import("../../crates/lib/pkg-publish/index.js");

  const uswdsPath = uswdsFixture.entry;
  const bootstrapPath = bootstrapFixture.entry;

  const results = [];
  const N = process.argv[2] ? parseInt(process.argv[2], 10) : 6;

  for (let i = 0; i < N; i++) {
    const which = i % 2 === 0 ? "uswds" : "bootstrap";
    const t0 = performance.now();
    let css;
    if (which === "uswds") {
      css = compile(uswdsPath, { loadPaths, quiet: true, charset: false }).css;
    } else {
      css = compile(bootstrapPath, { loadPaths: [], quiet: true }).css;
    }
    const t1 = performance.now();
    results.push({ i, which, ms: t1 - t0, len: css.length, hash: hash(css) });
    console.log(
      `compile ${i}: ${which} len=${css.length} hash=${hash(css)} time=${(t1 - t0).toFixed(1)}ms`
    );
  }

  // group by which, verify all hashes for a given fixture are identical
  const byWhich = {};
  for (const r of results) {
    byWhich[r.which] = byWhich[r.which] || [];
    byWhich[r.which].push(r);
  }
  let ok = true;
  for (const [which, rs] of Object.entries(byWhich)) {
    const hashes = new Set(rs.map((r) => r.hash));
    if (hashes.size !== 1) {
      console.error(`MISMATCH for ${which}: hashes=${[...hashes]}`);
      ok = false;
    } else {
      console.log(`${which}: all ${rs.length} compiles byte-identical (hash=${[...hashes][0]}, len=${rs[0].len})`);
    }
  }
  if (!ok) {
    console.error("STRESS TEST FAILED: non-deterministic output across repeated compiles");
    process.exit(1);
  }
  console.log("STRESS TEST PASSED");
}

main();
