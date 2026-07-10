import {
  default as initWasm,
  initSync,
  compile as wasmCompile,
  compile_file as wasmCompileFile,
} from "./grass.js";

let initialized = false;

/**
 * Initialize the WASM module. Must be called before any compile functions.
 *
 * @param {BufferSource | WebAssembly.Module} [input] - WASM bytes or module.
 *   If not provided, fetches grass_bg.wasm relative to this module.
 */
export async function init(input) {
  if (initialized) return;
  if (input) {
    initSync({ module: input });
  } else {
    await initWasm(new URL("./grass_bg.wasm", import.meta.url));
  }
  initialized = true;
}

function ensureInit() {
  if (!initialized) {
    throw new Error(
      "WASM not initialized. Call `await init()` before using compile functions."
    );
  }
}

function makeResult(css, sourceMap) {
  return { css, loadedUrls: [], sourceMap };
}

function buildOptions(options) {
  return {
    style: options.style || "expanded",
    loadPaths: options.loadPaths || [],
    quiet: options.quietDeps || options.quiet || false,
    sourceMap: options.sourceMap || false,
    sourceMapIncludeSources: options.sourceMapIncludeSources || false,
  };
}

const nullFs = {
  is_file() { return false; },
  is_dir() { return false; },
  read() { throw new Error("No filesystem available. Pass options.fs or use loadPaths."); },
  canonicalize(p) { return p; },
};

/**
 * Compile a Sass string to CSS.
 *
 * @param {string} source - SCSS source code
 * @param {object} [options]
 * @param {string} [options.style] - 'expanded' or 'compressed'
 * @param {string[]} [options.loadPaths] - Paths to resolve @use/@import
 * @param {boolean} [options.quiet] - Suppress warnings
 * @param {boolean} [options.sourceMap] - Generate a source map
 * @param {boolean} [options.sourceMapIncludeSources] - Embed sources in the source map
 * @param {object} [options.fs] - Custom filesystem callbacks: { is_file, is_dir, read, canonicalize }
 */
export function compileString(source, options = {}) {
  ensureInit();
  const opts = buildOptions(options);
  const fs = options.fs || nullFs;

  try {
    const result = wasmCompile(
      source,
      opts.loadPaths,
      opts.style,
      opts.quiet,
      opts.sourceMap,
      opts.sourceMapIncludeSources,
      fs
    );
    return makeResult(result.css, result.sourceMap);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

/**
 * Compile a Sass file to CSS. Requires fs callbacks in options.
 */
export function compile(path, options = {}) {
  ensureInit();
  const opts = buildOptions(options);
  const fs = options.fs;
  if (!fs) {
    throw new Error(
      "compile() requires options.fs with { is_file, is_dir, read, canonicalize } callbacks in browser/bundler environments."
    );
  }

  try {
    const result = wasmCompileFile(
      path,
      opts.loadPaths,
      opts.style,
      opts.quiet,
      opts.sourceMap,
      opts.sourceMapIncludeSources,
      fs
    );
    return makeResult(result.css, result.sourceMap);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

export function compileStringAsync(source, options = {}) {
  return Promise.resolve().then(() => compileString(source, options));
}

export function compileAsync(path, options = {}) {
  return Promise.resolve().then(() => compile(path, options));
}
