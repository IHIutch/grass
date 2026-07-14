// Miniaturized version of bench/diagnostics/multi-compile-stress.mjs: alternates two
// small compiles N times in ONE wasm instance and checks every output is
// byte-identical to the first occurrence of its input. A bad allocator
// reset (todo #282) may only show up on compile 2+, since compile 1 always
// starts from pristine memory — that's what this guards against, without
// the USWDS/Bootstrap fixtures that make the prototype version too heavy
// for CI.
import assert from "assert";
import { spawnSync } from "child_process";
import { writeFileSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { fileURLToPath } from "url";
import { createHash } from "crypto";

const __filename = fileURLToPath(import.meta.url);
const N = 10;
const TIMEOUT_MS = 30_000;

if (process.argv[2] !== "--worker") {
  const result = spawnSync(process.execPath, [__filename, "--worker"], {
    stdio: "inherit",
    env: { ...process.env, GRASS_FORCE_WASM: "1" },
    timeout: TIMEOUT_MS,
  });
  if (result.signal) {
    console.error(`persistent-instance worker was killed by ${result.signal} (exceeded ${TIMEOUT_MS}ms guard)`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error("persistent-instance worker failed");
    process.exit(result.status ?? 1);
  }
  console.log("persistent-instance ok");
  process.exit(0);
}

// --- Worker: runs under GRASS_FORCE_WASM=1 ---

assert.equal(process.env.GRASS_FORCE_WASM, "1");

const grass = await import("../index.js");
assert.equal(grass.SassNumber, undefined, "GRASS_FORCE_WASM did not disable the native binding");

function hash(s) {
  return createHash("sha256").update(s).digest("hex").slice(0, 16);
}

const depName = `grass-persistent-dep-${process.pid}`;
const depPath = join(tmpdir(), `${depName}.scss`);
const entryPath = join(tmpdir(), `grass-persistent-entry-${process.pid}.scss`);
writeFileSync(depPath, "$c: teal;");
writeFileSync(entryPath, `@use "${depName}" as dep;\na { b: dep.$c; }`);

try {
  const t0 = performance.now();
  const results = [];

  for (let i = 0; i < N; i++) {
    const which = i % 2 === 0 ? "string" : "file";
    const css =
      which === "string"
        ? grass.compileString("a { b: c; }\n.x { y: 1 + 2; }").css
        : grass.compile(entryPath).css;
    results.push({ i, which, hash: hash(css) });
  }

  const elapsed = performance.now() - t0;

  const byWhich = {};
  for (const r of results) {
    byWhich[r.which] = byWhich[r.which] || [];
    byWhich[r.which].push(r);
  }
  for (const [which, rs] of Object.entries(byWhich)) {
    const hashes = new Set(rs.map((r) => r.hash));
    assert.equal(
      hashes.size,
      1,
      `non-deterministic output across repeated compiles for "${which}": hashes=${[...hashes]}`,
    );
  }

  assert.ok(
    elapsed < TIMEOUT_MS,
    `persistent-instance loop took ${elapsed.toFixed(1)}ms, exceeding the ${TIMEOUT_MS}ms guard`,
  );

  console.log(
    `persistent-instance worker ok: N=${N} compiles, ${Object.keys(byWhich).length} fixtures, all byte-identical, ${elapsed.toFixed(1)}ms`,
  );
} finally {
  rmSync(depPath);
  rmSync(entryPath);
}
