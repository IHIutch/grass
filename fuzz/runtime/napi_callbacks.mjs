import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const binding = require("../../crates/napi");
const runs = Number.parseInt(process.env.FUZZ_RUNS || "64", 10);

assert.equal(typeof binding.compileString, "function");
assert.equal(typeof binding.compileStringAsync, "function");
assert.equal(typeof binding.SassNumber, "function");
assert.equal(typeof binding.SassString, "function");
assert.equal(typeof binding.SassList, "function");

function nextSeed(index) {
  let value = (0x9e3779b9 * (index + 1)) >>> 0;
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
  const number = 1 + (next() % 32);
  const label = ["red", "green", "blue", "purple"][next() % 4];
  const color = ["red", "teal", "orange", "navy"][next() % 4];
  const source = `@use "db:dep" as dep;
a {
  number: scale(${number}px);
  label: label("${label}");
  pair: pair(${number});
  imported: dep.$color;
}`;
  const options = {
    functions: {
      "scale($n)": ([value]) => new binding.SassNumber(value.value * 2, {
        numeratorUnits: value.numeratorUnits,
        denominatorUnits: value.denominatorUnits,
      }),
      "label($value)": ([value]) => new binding.SassString(
        value.text.toUpperCase(),
        value.hasQuotes,
      ),
      "pair($value)": ([value]) => new binding.SassList(
        [value, new binding.SassNumber(value.value + 1)],
        "space",
        false,
      ),
    },
    importers: [{
      canonicalize(url, context) {
        assert.equal(typeof url, "string");
        assert.equal(typeof context.fromImport, "boolean");
        return url === "db:dep" ? "db:dep" : null;
      },
      load(canonicalUrl) {
        assert.equal(canonicalUrl, "db:dep");
        return { contents: `$color: ${color};`, syntax: "scss" };
      },
    }],
  };

  const result = index % 2 === 0
    ? binding.compileString(source, options)
    : await binding.compileStringAsync(source, options);
  assert.match(result.css, /number:/);
  assert.match(result.css, /imported:/);
  completed += 1;
}

console.log(`napi_callbacks completed ${completed} cases expected_errors=0`);
