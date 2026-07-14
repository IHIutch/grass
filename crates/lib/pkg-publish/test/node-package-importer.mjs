import assert from "assert";
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "fs";
import { spawnSync } from "child_process";
import { tmpdir } from "os";
import { dirname, join } from "path";
import { fileURLToPath, pathToFileURL } from "url";
import * as grass from "../index.js";

function writePackage(root, name, manifest, files) {
  const packageRoot = join(root, "node_modules", name);
  mkdirSync(packageRoot, { recursive: true });
  writeFileSync(join(packageRoot, "package.json"), JSON.stringify({ name, ...manifest }));
  for (const [file, contents] of Object.entries(files)) {
    const path = join(packageRoot, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, contents);
  }
  return packageRoot;
}

function referenceCss(entry) {
  const result = spawnSync("npx", ["-y", "sass@1.101.0", "--pkg-importer=node", entry], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  // dart-sass's CLI terminates stdout with a newline; its JS API does not return one
  // (`compile(...).css` ends at the closing brace). We compare this CLI stdout against
  // grass's *JS API* result, so strip exactly one trailing newline to compare like for
  // like. grass matches both contracts: its CLI emits the newline, its JS API does not.
  return result.stdout.replace(/\n$/, "");
}

const hasNative = process.env.GRASS_FORCE_WASM !== "1" &&
  readdirSync(new URL("..", import.meta.url)).some((file) => file.endsWith(".node"));

if (!hasNative) {
  assert.throws(
    () => grass.compileString("a { color: red; }", {
      importers: [new grass.NodePackageImporter(process.cwd())],
    }),
    /native binding/,
  );
  console.log("node-package-importer wasm-fallback ok");
} else {
  const root = mkdtempSync(join(tmpdir(), "grass-node-package-importer-"));
  try {
  const exportsEntry = join(root, "exports-entry.scss");
  writeFileSync(exportsEntry, '@use "pkg:theme" as theme; a { color: theme.$color; }\n');
  writePackage(root, "theme", {
    exports: {
      ".": { sass: "./src/index.scss", default: "./src/default.js" },
      "./colors.scss": { sass: "./src/_colors.scss" },
    },
  }, {
    "src/index.scss": "$color: red;\n",
    "src/_colors.scss": "$color: blue;\n",
    "src/default.js": "not Sass",
  });

  // F-307(1): the Node entrypoint accepts the helper and resolves an exports sass condition.
  const importer = new grass.NodePackageImporter(root);
  const exportsResult = grass.compile(exportsEntry, { importers: [importer] });
  assert.equal(exportsResult.css, referenceCss(exportsEntry));

  // F-307(2): package subpaths use package entrypoints, while Sass resolves the partial.
  const subpathEntry = join(root, "subpath-entry.scss");
  writeFileSync(subpathEntry, '@use "pkg:theme/colors" as colors; a { color: colors.$color; }\n');
  const subpathResult = grass.compile(subpathEntry, { importers: [importer] });
  assert.equal(subpathResult.css, referenceCss(subpathEntry));

  // F-307(1): scoped package names use the complete @scope/name package key.
  writePackage(root, "@org/theme", { sass: "./theme.scss" }, {
    "theme.scss": ".scoped { color: orange; }\n",
  });
  const scopedResult = grass.compileString('@use "pkg:@org/theme" as scoped;', {
    importers: [importer],
  });
  assert.equal(scopedResult.css, ".scoped {\n  color: orange;\n}");

  // Root manifest fallback checks sass before style when exports is absent.
  const fallbackRoot = join(root, "fallback");
  mkdirSync(fallbackRoot, { recursive: true });
  const fallbackEntry = join(fallbackRoot, "entry.scss");
  writeFileSync(fallbackEntry, '@use "pkg:fallback-theme" as fallback;\n');
  writePackage(fallbackRoot, "fallback-theme", {
    sass: "./fallback.scss",
    style: "./style.scss",
  }, {
    "fallback.scss": ".fallback { color: red; }\n",
    "style.scss": ".style { color: blue; }\n",
  });
  const fallbackResult = grass.compile(fallbackEntry, { importers: [new grass.NodePackageImporter(fallbackRoot)] });
  assert.equal(fallbackResult.css, referenceCss(fallbackEntry));

  // Sass 1.101.0: @import selects an existing import-only sibling declared by
  // a package sass/style/exports target, while @use keeps the normal target.
  const importOnlyRoot = join(root, "import-only");
  mkdirSync(importOnlyRoot, { recursive: true });
  const importOnlyEntry = join(importOnlyRoot, "entry.scss");
  writeFileSync(importOnlyEntry, '@import "pkg:import-only-theme";\n');
  writePackage(importOnlyRoot, "import-only-theme", { sass: "./src/theme.scss" }, {
    "src/theme.scss": ".normal { color: red; }\n",
    "src/theme.import.scss": ".import-only { color: blue; }\n",
  });
  const importOnlyResult = grass.compile(importOnlyEntry, {
    importers: [new grass.NodePackageImporter(importOnlyRoot)],
  });
  assert.equal(importOnlyResult.css, referenceCss(importOnlyEntry));

  // F-307(3): an explicit entryPointDirectory controls the node_modules search parent.
  const explicitRoot = join(root, "explicit");
  mkdirSync(explicitRoot, { recursive: true });
  writePackage(explicitRoot, "explicit-theme", { sass: "./theme.scss" }, {
    "theme.scss": ".explicit { color: green; }\n",
  });
  const explicitResult = grass.compileString('@use "pkg:explicit-theme" as explicit;', {
    importers: [new grass.NodePackageImporter(explicitRoot)],
  });
  assert.equal(explicitResult.css, ".explicit {\n  color: green;\n}");

  // F-307(4): a preceding FileImporter wins at its supplied array position.
  const virtualFile = join(root, "virtual.scss");
  writeFileSync(virtualFile, ".preceding { color: purple; }\n");
  const precedingResult = grass.compileString('@use "pkg:theme" as theme;', {
    importers: [
      { findFileUrl(url) { return url === "pkg:theme" ? pathToFileURL(virtualFile).href : null; } },
      importer,
    ],
  });
  assert.equal(precedingResult.css, ".preceding {\n  color: purple;\n}");

  // F-307(5): missing packages and blocked exports decline into Sass's normal error;
  // malformed package metadata is surfaced from the Node resolver.
  assert.throws(
    () => grass.compileString('@use "pkg:does-not-exist" as missing;', { importers: [importer] }),
    /Can't find stylesheet to import/,
  );
  const blockedRoot = join(root, "blocked");
  mkdirSync(blockedRoot, { recursive: true });
  writePackage(blockedRoot, "blocked-theme", { exports: { ".": { sass: "./index.scss" } } }, {
    "index.scss": "$color: red;\n",
  });
  assert.throws(
    () => grass.compileString('@use "pkg:blocked-theme/private" as blocked;', {
      importers: [new grass.NodePackageImporter(blockedRoot)],
    }),
    /Can't find stylesheet to import/,
  );
  const invalidRoot = join(root, "invalid");
  mkdirSync(join(invalidRoot, "node_modules/invalid-theme"), { recursive: true });
  writeFileSync(join(invalidRoot, "node_modules/invalid-theme/package.json"), "{ invalid json");
  assert.throws(
    () => grass.compileString('@use "pkg:invalid-theme" as invalid;', {
      importers: [new grass.NodePackageImporter(invalidRoot)],
    }),
    /Unexpected token|JSON|parse/,
  );

  // F-307(8): sync and current callback-based async entrypoints use the same adapter.
  const asyncResult = await grass.compileStringAsync('@use "pkg:theme" as theme; a { color: theme.$color; }', {
    importers: [importer],
  });
  assert.equal(asyncResult.css, "a {\n  color: red;\n}");
  const asyncFileResult = await grass.compileAsync(exportsEntry, { importers: [importer] });
  assert.equal(asyncFileResult.css, exportsResult.css);

  // F-307(6): Node resolution is absent from browser and Workers bundles.
  const browser = await import("../browser.js");
  const workers = await import("../workers.js");
  assert.equal("NodePackageImporter" in browser, false);
  assert.equal("NodePackageImporter" in workers, false);

  // The helper returns an absolute file URL for the existing FileImporter bridge.
  assert.equal(importer.findFileUrl("theme:other"), null);
  assert.equal(importer.findFileUrl("pkg:theme").startsWith("file:///"), true);
  assert.equal(importer.findFileUrl("pkg:theme/colors").startsWith("file:///"), true);
  assert.equal(importer.findFileUrl("pkg:theme/unknown"), null);

  console.log("node package importer ok");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
