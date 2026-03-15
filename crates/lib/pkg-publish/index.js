import { readFileSync, statSync, realpathSync } from "fs";
import { resolve, dirname } from "path";
import { pathToFileURL, fileURLToPath } from "url";
import { createRequire } from "module";

const __dirname = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

// --- Native binding loader (try napi first) ---

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

  // Build the platform-specific package name
  let suffix;
  switch (platform) {
    case "darwin":
      suffix = arch === "arm64" ? "darwin-arm64" : "darwin-x64";
      break;
    case "linux":
      const musl = isMusl();
      if (arch === "arm64" || arch === "aarch64") {
        suffix = musl ? "linux-arm64-musl" : "linux-arm64-gnu";
      } else {
        suffix = musl ? "linux-x64-musl" : "linux-x64-gnu";
      }
      break;
    case "win32":
      suffix = "win32-x64-msvc";
      break;
    default:
      return null;
  }

  try {
    return require(`ihiutch-grass-napi-${suffix}`);
  } catch {
    return null;
  }
}

nativeBinding = tryLoadNative();

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
};

// --- Public API ---

function makeResult(css, inputPath) {
  const loadedUrls = [];
  if (inputPath) loadedUrls.push(pathToFileURL(resolve(inputPath)));
  return { css, loadedUrls, sourceMap: undefined };
}

function buildOptions(options) {
  return {
    style: options.style || "expanded",
    loadPaths: options.loadPaths || [],
    quiet: options.quietDeps || options.quiet || false,
    charset: options.charset !== undefined ? options.charset : true,
  };
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
      });
      return makeResult(result.css, path);
    } catch (e) {
      throw new Error(typeof e === "string" ? e : e.message || String(e));
    }
  }

  // WASM fallback
  const wasm = loadWasm();
  try {
    const css = wasm.compile_file(path, opts.loadPaths, opts.style, opts.quiet, fsCallbacks);
    return makeResult(css, path);
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
      });
      return makeResult(result.css, null);
    } catch (e) {
      throw new Error(typeof e === "string" ? e : e.message || String(e));
    }
  }

  // WASM fallback
  const wasm = loadWasm();
  try {
    const css = wasm.compile(source, opts.loadPaths, opts.style, opts.quiet, fsCallbacks);
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
