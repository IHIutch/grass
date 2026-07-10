import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import * as browser from "../../crates/lib/pkg-publish/browser.js";

const runs = Number.parseInt(process.env.FUZZ_RUNS || "64", 10);
await browser.init(readFileSync(new URL("../../crates/lib/pkg-publish/grass_bg.wasm", import.meta.url)));

function nextSeed(index) {
  let value = (0x85ebca6b * (index + 1)) >>> 0;
  return () => {
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;
    return value >>> 0;
  };
}

let completed = 0;
for (let index = 0; index < runs; index += 1) {
  const next = nextSeed(index);
  const value = 1 + (next() % 32);
  const style = next() % 2 === 0 ? "expanded" : "compressed";
  const source = `@use "dep" as dep;\na { value: ${value}px; imported: dep.$color; }`;
  const files = new Map([
    ["/virtual/main.scss", source],
    ["/virtual/_dep.scss", `$color: ${["red", "teal", "navy"][next() % 3]};`],
  ]);
  const encoder = new TextEncoder();
  const fs = {
    is_file(path) { return files.has(path); },
    is_dir(path) { return path === "/virtual"; },
    read(path) {
      assert.equal(files.has(path), true);
      return encoder.encode(files.get(path));
    },
    canonicalize(path) { return path; },
    resolve_first_existing(candidates) {
      return candidates.find((candidate) => files.has(candidate)) ?? null;
    },
    readdirSync(dir) {
      if (dir !== "/virtual") return [];
      return ["f_dep.scss", "f_main.scss"];
    },
  };

  const options = { fs, loadPaths: ["/virtual"], style };
  const result = index % 2 === 0
    ? browser.compileString(source, options)
    : await browser.compileAsync("/virtual/main.scss", options);
  assert.match(result.css, /imported/);
  completed += 1;
}

console.log(`wasm_callbacks completed ${completed} cases expected_errors=0`);
