import { readFileSync, statSync, realpathSync } from "fs";
import { resolve, dirname } from "path";
import { pathToFileURL, fileURLToPath } from "url";
import {
  initSync,
  compile as wasmCompile,
  compile_file as wasmCompileFile,
} from "./grass.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const wasmBytes = readFileSync(resolve(__dirname, "grass_bg.wasm"));
initSync({ module: wasmBytes });

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
};

function makeResult(css, inputPath) {
  const loadedUrls = [];
  if (inputPath) loadedUrls.push(pathToFileURL(resolve(inputPath)));
  return { css, loadedUrls, sourceMap: undefined };
}

export function compile(path, options = {}) {
  const style = options.style || "expanded";
  const loadPaths = options.loadPaths || [];
  const quiet = options.quietDeps || options.quiet || false;
  const charset = options.charset !== undefined ? options.charset : true;

  try {
    const css = wasmCompileFile(path, loadPaths, style, quiet, fsCallbacks);
    return makeResult(css, path);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

export function compileString(source, options = {}) {
  const style = options.style || "expanded";
  const loadPaths = options.loadPaths || [];
  const quiet = options.quietDeps || options.quiet || false;
  const charset = options.charset !== undefined ? options.charset : true;

  try {
    const css = wasmCompile(source, loadPaths, style, quiet, fsCallbacks);
    return makeResult(css, null);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e.message || String(e));
  }
}

export function compileAsync(path, options = {}) {
  return Promise.resolve().then(() => compile(path, options));
}

export function compileStringAsync(source, options = {}) {
  return Promise.resolve().then(() => compileString(source, options));
}
