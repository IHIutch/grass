import {
  compile,
  compileAsync,
  compileString,
  compileStringAsync,
  SassList,
  SassNumber,
  SassString,
  type FileImporter,
  type Importer,
} from "ihiutch-grass";
import * as browser from "../../browser.js";
import * as workers from "ihiutch-grass/workers";

const fileImporter: FileImporter = {
  findFileUrl(url) {
    return url === "virtual:theme" ? "file:///tmp/theme.scss" : null;
  },
};

const importer: Importer = {
  canonicalize(url) {
    return url === "virtual:theme" ? "virtual:theme" : null;
  },
  load(canonicalUrl) {
    return canonicalUrl === "virtual:theme"
      ? { contents: "$color: red;", syntax: "scss" }
      : null;
  },
};

const functions = {
  "number($value)": (_args: Array<SassNumber | SassString | SassList | boolean | null>) =>
    new SassNumber(1, "px"),
};

const fileOptions = {
  functions,
  importers: [fileImporter, importer],
  sourceMap: true,
  sourceMapIncludeSources: true,
};

compile("styles.scss", fileOptions);
compileAsync("styles.scss", fileOptions);

const stringOptions = {
  ...fileOptions,
  url: "file:///entry.scss",
  importer,
};

compileString("a { width: number(1); }", stringOptions);
compileStringAsync("a { width: number(1); }", stringOptions);

await browser.init(new Uint8Array());
browser.compileString("a { color: red; }", { sourceMap: true });

declare const wasmModule: WebAssembly.Module;
workers.init(wasmModule);
await workers.compileStringAsync("a { color: red; }", { sourceMap: true });

// Browser and Workers are WASM-only surfaces.
// @ts-expect-error browser options do not accept JavaScript functions.
browser.compileString("a { color: red; }", { functions: {} });
// @ts-expect-error Workers do not expose path-based compileAsync.
 workers.compileAsync("styles.scss");
