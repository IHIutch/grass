import { readFileSync, statSync, realpathSync, existsSync, readdirSync } from "fs";
import { resolve, dirname } from "path";
import { pathToFileURL, fileURLToPath } from "url";
import { createRequire } from "module";

const __dirname = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

// --- Native binding loader ---

let nativeBinding = null;

function tryLoadNative() {
  const { platform, arch } = process;

  function isMusl() {
    if (!process.report || typeof process.report.getReport !== "function") {
      try {
        const lddPath = require("child_process").execSync("which ldd").toString().trim();
        return readFileSync(lddPath, "utf8").includes("musl");
      } catch {
        return true;
      }
    } else {
      const { glibcVersionRuntime } = process.report.getReport().header;
      return !glibcVersionRuntime;
    }
  }

  let suffix;
  switch (platform) {
    case "darwin":
      suffix = arch === "arm64" ? "darwin-arm64" : "darwin-x64";
      break;
    case "linux": {
      const musl = isMusl();
      if (arch === "arm64" || arch === "aarch64") {
        suffix = musl ? "linux-arm64-musl" : "linux-arm64-gnu";
      } else {
        suffix = musl ? "linux-x64-musl" : "linux-x64-gnu";
      }
      break;
    }
    case "win32":
      suffix = "win32-x64-msvc";
      break;
    default:
      return null;
  }

  // Load bundled .node file from this package
  const nodePath = resolve(__dirname, `grass.${suffix}.node`);
  try {
    if (existsSync(nodePath)) {
      return require(nodePath);
    }
  } catch {}

  return null;
}

nativeBinding = tryLoadNative();

// Value classes for `options.functions`/`options.importers` return values.
// The native binding validates callback return values by identity against
// these exact classes (napi's generated `instanceof` check) — constructing
// a return value requires the class from THIS binding, so it must be
// re-exported rather than reimplemented. Undefined on the WASM fallback,
// where `functions`/`importers` are unsupported (see assertWasmSupportsOptions).
export const SassNumber = nativeBinding ? nativeBinding.SassNumber : undefined;
export const SassString = nativeBinding ? nativeBinding.SassString : undefined;
export const SassList = nativeBinding ? nativeBinding.SassList : undefined;

// --- WASM fallback ---

let wasmBinding = null;

function loadWasm() {
  if (wasmBinding) return wasmBinding;

  const { initSync, compile: wasmCompile, compile_file: wasmCompileFile } =
    require("./grass.js");

  const wasmBytes = readFileSync(resolve(__dirname, "grass_bg.wasm"));
  initSync({ module: wasmBytes });

  wasmBinding = { compile: wasmCompile, compile_file: wasmCompileFile };
  return wasmBinding;
}

// --- Filesystem callbacks for WASM ---

const fsCallbacks = {
  is_file(path) {
    try { return statSync(path).isFile(); } catch { return false; }
  },
  is_dir(path) {
    try { return statSync(path).isDirectory(); } catch { return false; }
  },
  read(path) {
    return readFileSync(path);
  },
  canonicalize(path) {
    return realpathSync(path);
  },
  resolve_first_existing(candidates) {
    for (const p of candidates) {
      try { if (statSync(p).isFile()) return p; } catch {}
    }
    return null;
  },
  // Batches many per-candidate is_file/is_dir crossings into a single
  // directory read: each entry is a 1-char kind tag ("f"/"d"/other) followed
  // by the entry name.
  readdirSync(dir) {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return [];
    }
    return entries.map((e) => {
      let kind = "o";
      try {
        if (e.isFile()) kind = "f";
        else if (e.isDirectory()) kind = "d";
      } catch {}
      return kind + e.name;
    });
  },
};

// --- Public API ---

function makeResult(css, inputPath, sourceMap) {
  const loadedUrls = [];
  if (inputPath) loadedUrls.push(pathToFileURL(resolve(inputPath)));
  return { css, loadedUrls, sourceMap };
}

function buildOptions(options) {
  return {
    style: options.style || "expanded",
    loadPaths: options.loadPaths || [],
    quiet: options.quietDeps || options.quiet || false,
    charset: options.charset !== undefined ? options.charset : true,
    sourceMap: options.sourceMap || false,
    sourceMapIncludeSources: options.sourceMapIncludeSources || false,
    functions: options.functions,
    importers: options.importers,
  };
}

// options.functions/importers/importer/url require the native binding —
// silently dropping them on the WASM fallback would change compile output
// (a missing custom function/importer is a different Sass program), so we
// reject instead.
function assertWasmSupportsOptions(options) {
  if (options.functions !== undefined || options.importers !== undefined || options.importer !== undefined) {
    throw new Error(
      "options.functions and options.importers require the native binding, which is unavailable on this platform (WASM fallback active)"
    );
  }
  if (options.url !== undefined) {
    throw new Error(
      "options.url requires the native binding, which is unavailable on this platform (WASM fallback active)"
    );
  }
}

export function compile(path, options = {}) {
  const opts = buildOptions(options);

  if (nativeBinding) {
    try {
      const result = nativeBinding.compile(path, {
        style: opts.style,
        loadPaths: opts.loadPaths,
        quiet: opts.quiet,
        charset: opts.charset,
        sourceMap: opts.sourceMap,
        sourceMapIncludeSources: opts.sourceMapIncludeSources,
        functions: opts.functions,
        importers: opts.importers,
      });
      return makeResult(result.css, path, result.sourceMap);
    } catch (e) {
      throw new Error(typeof e === "string" ? e : e.message || String(e));
    }
  }

  assertWasmSupportsOptions(options);
  const wasm = loadWasm();
  try {
    const result = wasm.compile_file(
      path,
      opts.loadPaths,
      opts.style,
      opts.quiet,
      opts.sourceMap,
      opts.sourceMapIncludeSources,
      fsCallbacks
    );
    return makeResult(result.css, path, result.sourceMap);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

export function compileString(source, options = {}) {
  const opts = buildOptions(options);

  if (nativeBinding) {
    try {
      const result = nativeBinding.compileString(source, {
        style: opts.style,
        loadPaths: opts.loadPaths,
        quiet: opts.quiet,
        charset: opts.charset,
        sourceMap: opts.sourceMap,
        sourceMapIncludeSources: opts.sourceMapIncludeSources,
        functions: opts.functions,
        importers: opts.importers,
        url: options.url,
        importer: options.importer,
      });
      return makeResult(result.css, null, result.sourceMap);
    } catch (e) {
      throw new Error(typeof e === "string" ? e : e.message || String(e));
    }
  }

  assertWasmSupportsOptions(options);
  const wasm = loadWasm();
  try {
    const result = wasm.compile(
      source,
      opts.loadPaths,
      opts.style,
      opts.quiet,
      opts.sourceMap,
      opts.sourceMapIncludeSources,
      fsCallbacks
    );
    return makeResult(result.css, null, result.sourceMap);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

export function compileAsync(path, options = {}) {
  if (nativeBinding && typeof nativeBinding.compileAsync === "function") {
    const opts = buildOptions(options);
    return nativeBinding
      .compileAsync(path, {
        style: opts.style,
        loadPaths: opts.loadPaths,
        quiet: opts.quiet,
        charset: opts.charset,
        sourceMap: opts.sourceMap,
        sourceMapIncludeSources: opts.sourceMapIncludeSources,
        functions: opts.functions,
        importers: opts.importers,
      })
      .then(
        (result) => makeResult(result.css, path, result.sourceMap),
        (e) => {
          throw new Error(typeof e === "string" ? e : e.message || String(e));
        }
      );
  }
  // WASM (or missing-export) fallback: sync compile off a microtask.
  return Promise.resolve().then(() => compile(path, options));
}

export function compileStringAsync(source, options = {}) {
  if (nativeBinding && typeof nativeBinding.compileStringAsync === "function") {
    const opts = buildOptions(options);
    return nativeBinding
      .compileStringAsync(source, {
        style: opts.style,
        loadPaths: opts.loadPaths,
        quiet: opts.quiet,
        charset: opts.charset,
        sourceMap: opts.sourceMap,
        sourceMapIncludeSources: opts.sourceMapIncludeSources,
        functions: opts.functions,
        importers: opts.importers,
        url: options.url,
        importer: options.importer,
      })
      .then(
        (result) => makeResult(result.css, null, result.sourceMap),
        (e) => {
          throw new Error(typeof e === "string" ? e : e.message || String(e));
        }
      );
  }
  return Promise.resolve().then(() => compileString(source, options));
}
