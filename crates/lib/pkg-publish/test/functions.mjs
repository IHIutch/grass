import assert from "assert";
import * as grass from "../index.js";

// Native-vs-WASM detection, same pattern as smoke.mjs: the native binding is
// a bundled `.node` file next to index.js.
import { readdirSync, writeFileSync, rmSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { pathToFileURL } from "url";
const hasNative = readdirSync(new URL("..", import.meta.url)).some((f) => f.endsWith(".node"));

if (hasNative) {
  // --- functions (sync) ---

  {
    const opts = {
      functions: {
        "double($n)": (args) => new grass.SassNumber(args[0].value * 2, args[0].numeratorUnits.length ? args[0].numeratorUnits[0] : undefined),
      },
    };
    const res = grass.compileString("a { b: double(5px); }", opts);
    assert.equal(res.css, "a {\n  b: 10px;\n}\n");
  }

  // --- functions (async) ---

  {
    const opts = {
      functions: {
        "triple($n)": (args) => new grass.SassNumber(args[0].value * 3),
      },
    };
    const res = await grass.compileStringAsync("a { b: triple(2); }", opts);
    assert.equal(res.css, "a {\n  b: 6;\n}\n");
  }

  // --- throwing function callback surfaces as a clean error ---

  {
    const opts = {
      functions: {
        "boom()": () => {
          throw new Error("kaboom");
        },
      },
    };
    assert.throws(() => grass.compileString("a { b: boom(); }", opts), /kaboom/);
  }

  // --- importers: FileImporter shape (findFileUrl) ---

  {
    const path = join(tmpdir(), `grass-pkg-importer-redirect-${process.pid}.scss`);
    writeFileSync(path, "$a: red;");
    try {
      const opts = {
        importers: [
          {
            findFileUrl(url) {
              return url === "virtual:thing" ? pathToFileURL(path).href : null;
            },
          },
        ],
      };
      const res = grass.compileString('@import "virtual:thing";\na { b: $a; }', opts);
      assert.equal(res.css, "a {\n  b: red;\n}\n");
    } finally {
      rmSync(path);
    }
  }

  // --- importers: full Importer shape (canonicalize + load) ---

  {
    const opts = {
      importers: [
        {
          canonicalize(url) {
            return url === "db:colors" ? "db:colors" : null;
          },
          load(canonicalUrl) {
            assert.equal(canonicalUrl, "db:colors");
            return { contents: "$c: red;", syntax: "scss" };
          },
        },
      ],
    };
    const res = grass.compileString('@use "db:colors" as colors;\na { b: colors.$c; }', opts);
    assert.equal(res.css, "a {\n  b: red;\n}\n");
  }

  // --- importers (async): full Importer shape over compileStringAsync ---

  {
    const opts = {
      importers: [
        {
          canonicalize(url) {
            return url === "db:colors-async" ? "db:colors-async" : null;
          },
          load(canonicalUrl) {
            assert.equal(canonicalUrl, "db:colors-async");
            return { contents: "$c: blue;", syntax: "scss" };
          },
        },
      ],
    };
    const res = await grass.compileStringAsync('@use "db:colors-async" as colors;\na { b: colors.$c; }', opts);
    assert.equal(res.css, "a {\n  b: blue;\n}\n");
  }

  // --- throwing importer callback surfaces as a clean error ---

  {
    const opts = {
      importers: [
        {
          canonicalize: () => "db:boom",
          load() {
            throw new Error("load kaboom");
          },
        },
      ],
    };
    assert.throws(() => grass.compileString('@use "db:boom" as boom;\na { b: c; }', opts), /load kaboom/);
  }

  // --- StringOptions.url + StringOptions.importer ---

  {
    const opts = {
      url: "my://entry",
      importer: {
        canonicalize(url, context) {
          assert.equal(context.containingUrl, "my://entry");
          return url === "dep" ? "my://dep" : null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "my://dep");
          return { contents: "$c: green;", syntax: "scss" };
        },
      },
    };
    const res = grass.compileString('@use "dep" as d;\na { b: d.$c; }', opts);
    assert.equal(res.css, "a {\n  b: green;\n}\n");
  }

  // --- StringOptions.url + StringOptions.importer over the async entry point ---

  {
    const opts = {
      url: "my://entry-async",
      importer: {
        canonicalize(url, context) {
          assert.equal(context.containingUrl, "my://entry-async");
          return url === "dep" ? "my://dep-async" : null;
        },
        load(canonicalUrl) {
          assert.equal(canonicalUrl, "my://dep-async");
          return { contents: "$c: purple;", syntax: "scss" };
        },
      },
    };
    const res = await grass.compileStringAsync('@use "dep" as d;\na { b: d.$c; }', opts);
    assert.equal(res.css, "a {\n  b: purple;\n}\n");
  }

  console.log("functions native ok");
} else {
  // --- WASM-fallback rejection: functions/importers/importer/url must throw
  // a clear error naming the native-binding requirement, never silently
  // drop and change compile output. The callbacks below are never invoked
  // (the guard throws before compiling), so they don't need real bodies.

  assert.throws(
    () => grass.compileString("a { b: c }", { functions: { "f()": () => null } }),
    /native binding/,
  );
  assert.throws(
    () => grass.compileString("a { b: c }", { importers: [{ findFileUrl: () => null }] }),
    /native binding/,
  );
  assert.throws(
    () => grass.compileString("a { b: c }", { importer: { canonicalize: () => null, load: () => null } }),
    /native binding/,
  );
  assert.throws(
    () => grass.compileString("a { b: c }", { url: "my://entry" }),
    /native binding/,
  );
  assert.throws(
    () => grass.compile("a.scss", { functions: { "f()": () => null } }),
    /native binding/,
  );

  await assert.rejects(
    grass.compileStringAsync("a { b: c }", { functions: { "f()": () => null } }),
    /native binding/,
  );

  console.log("functions wasm-fallback ok");
}
