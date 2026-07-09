import {
  initSync,
  compile as wasmCompile,
} from "./grass.js";

let initialized = false;

/**
 * Initialize the WASM module for Cloudflare Workers.
 *
 * @example
 * ```js
 * import { init, compileString } from 'ihiutch-grass/workers';
 * import wasmModule from 'ihiutch-grass/grass_bg.wasm';
 *
 * // Call once at startup
 * init(wasmModule);
 *
 * export default {
 *   async fetch(request) {
 *     const result = compileString('a { color: red; }');
 *     return new Response(result.css, {
 *       headers: { 'Content-Type': 'text/css' },
 *     });
 *   }
 * };
 * ```
 *
 * @param {WebAssembly.Module} wasmModule - Pre-compiled WASM module from a static import
 */
export function init(wasmModule) {
  if (initialized) return;
  initSync({ module: wasmModule });
  initialized = true;
}

function ensureInit() {
  if (!initialized) {
    throw new Error(
      "WASM not initialized. Call init(wasmModule) with a static WASM import first."
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
  read(p) { throw new Error(`Cannot read file "${p}" in Workers environment.`); },
  canonicalize(p) { return p; },
};

/**
 * Compile a Sass string to CSS.
 *
 * Note: @use/@import will not resolve files in Workers (no filesystem).
 * Pass all Sass source as a single string.
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

export function compileStringAsync(source, options = {}) {
  return Promise.resolve().then(() => compileString(source, options));
}
