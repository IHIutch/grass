import { readFileSync, statSync, realpathSync, existsSync, readdirSync } from "fs";
import { resolve, dirname, relative } from "path";
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

nativeBinding = process.env.GRASS_FORCE_WASM === "1" ? null : tryLoadNative();

// Value classes for `options.functions`/`options.importers` return values.
// The native binding validates callback return values by identity against
// these exact classes (napi's generated `instanceof` check) — constructing
// a return value requires the class from THIS binding, so it must be
// re-exported rather than reimplemented. Undefined on the WASM fallback,
// where `functions`/`importers` are unsupported (see assertWasmSupportsOptions).
export const SassNumber = nativeBinding ? nativeBinding.SassNumber : undefined;
export const SassString = nativeBinding ? nativeBinding.SassString : undefined;
export const SassList = nativeBinding ? nativeBinding.SassList : undefined;

// --- Node package importer ---

const sassFileExtension = /\.(?:scss|sass|css)$/i;

function packageSubpathCandidates(subpath) {
  const candidates = [subpath];
  const extensions = [".scss", ".sass", ".css"];
  const hasExtension = extensions.some((extension) => subpath.endsWith(extension));
  const slash = subpath.lastIndexOf("/");
  const parent = slash === -1 ? "" : subpath.slice(0, slash + 1);
  const leaf = slash === -1 ? subpath : subpath.slice(slash + 1);

  if (!hasExtension) {
    for (const extension of extensions) candidates.push(`${subpath}${extension}`);
    candidates.push(`${parent}${leaf}/index`);
    for (const extension of extensions) candidates.push(`${parent}${leaf}/index${extension}`);
    candidates.push(`${parent}_${leaf}`);
    for (const extension of extensions) candidates.push(`${parent}_${leaf}${extension}`);
  }

  return candidates.map((candidate) => `./${candidate}`);
}

function packageRootFrom(baseDirectory, packageName) {
  let directory = resolve(baseDirectory);
  while (true) {
    const candidate = resolve(directory, "node_modules", packageName);
    try {
      if (statSync(candidate).isDirectory()) return candidate;
    } catch {}

    const parent = dirname(directory);
    if (parent === directory) return null;
    directory = parent;
  }
}

function packageTarget(packageRoot, target, { exportTarget = false, pattern = null, fromImport = false } = {}) {
  if (typeof target !== "string") {
    throw new Error(`Invalid package target in ${packageRoot}/package.json`);
  }

  if (pattern !== null) target = target.replaceAll("*", pattern);
  if (!target.startsWith("./")) {
    throw new Error(`Invalid package target ${JSON.stringify(target)} in ${packageRoot}/package.json`);
  }

  const targetPath = resolve(packageRoot, target);
  const outsidePackage = relative(packageRoot, targetPath).startsWith("..");
  if (outsidePackage) {
    throw new Error(`Invalid package target ${JSON.stringify(target)} in ${packageRoot}/package.json`);
  }
  if (exportTarget && !sassFileExtension.test(targetPath)) {
    throw new Error(
      `The export in ${packageRoot}/package.json resolved to ${JSON.stringify(targetPath)}, which is not a '.scss', '.sass', or '.css' file.`
    );
  }

  let resolvedTarget = targetPath;
  if (fromImport && /\.(?:scss|sass)$/i.test(targetPath)) {
    const importOnlyTarget = targetPath.replace(/\.(scss|sass)$/i, ".import.$1");
    if (existsSync(importOnlyTarget)) resolvedTarget = importOnlyTarget;
  }

  return pathToFileURL(resolvedTarget).href;
}

function resolveConditionalExport(packageRoot, value, pattern = null, fromImport = false) {
  if (typeof value === "string") return packageTarget(packageRoot, value, { exportTarget: true, pattern, fromImport });
  if (Array.isArray(value)) {
    for (const candidate of value) {
      const resolved = resolveConditionalExport(packageRoot, candidate, pattern, fromImport);
      if (resolved !== null) return resolved;
    }
    return null;
  }
  if (value === null) return null;
  if (typeof value !== "object") {
    throw new Error(`Invalid package target in ${packageRoot}/package.json`);
  }

  // Sass deliberately selects the first relevant condition in manifest order.
  for (const [condition, candidate] of Object.entries(value)) {
    if (condition !== "sass" && condition !== "style" && condition !== "default") continue;
    return resolveConditionalExport(packageRoot, candidate, pattern, fromImport);
  }
  return null;
}

function resolvePackageExport(packageRoot, exports, subpath, fromImport) {
  if (subpath === "") {
    if (typeof exports === "string" || Array.isArray(exports)) {
      return resolveConditionalExport(packageRoot, exports, null, fromImport);
    }
    if (exports && typeof exports === "object" && "." in exports) {
      return resolveConditionalExport(packageRoot, exports["."], null, fromImport);
    }
    return resolveConditionalExport(packageRoot, exports, null, fromImport);
  }

  if (!exports || typeof exports !== "object" || Array.isArray(exports)) return null;
  const candidates = packageSubpathCandidates(subpath);

  // Exact keys always take precedence over pattern keys in Node package maps.
  for (const candidate of candidates) {
    if (Object.prototype.hasOwnProperty.call(exports, candidate)) {
      return resolveConditionalExport(packageRoot, exports[candidate], null, fromImport);
    }
  }

  for (const [key, value] of Object.entries(exports)) {
    const star = key.indexOf("*");
    if (star === -1) continue;
    const prefix = key.slice(0, star);
    const suffix = key.slice(star + 1);
    for (const candidate of candidates) {
      if (candidate.startsWith(prefix) && candidate.endsWith(suffix) && candidate.length >= prefix.length + suffix.length) {
        const match = candidate.slice(prefix.length, candidate.length - suffix.length || undefined);
        return resolveConditionalExport(packageRoot, value, match, fromImport);
      }
    }
  }

  return null;
}

function parsePackageUrl(url) {
  if (!url.startsWith("pkg:")) return null;
  const specifier = url.slice(4);
  const parts = specifier.split("/");
  const packageParts = specifier.startsWith("@") ? 2 : 1;
  if (parts.length < packageParts || parts.slice(0, packageParts).some((part) => !part)) return null;

  const packageName = parts.slice(0, packageParts).join("/");
  const subpath = parts.slice(packageParts).join("/");
  if (packageName.includes("\\") || packageName.includes("..")) return null;
  return { packageName, subpath };
}

function containingDirectory(context, fallback) {
  if (context?.containingUrl?.startsWith("file:")) {
    try { return dirname(fileURLToPath(context.containingUrl)); } catch {}
  }
  return fallback;
}

function lowerNodePackageImporters(importers) {
  if (!importers) return importers;
  return importers.map((importer) => {
    if (!(importer instanceof NodePackageImporter)) return importer;
    return {
      findFileUrl(url, context) {
        return importer.findFileUrl(url, context);
      },
    };
  });
}

export class NodePackageImporter {
  constructor(entryPointDirectory) {
    if (entryPointDirectory === undefined) {
      if (!process.argv[1]) {
        throw new Error("NodePackageImporter requires a Node.js entrypoint directory");
      }
      entryPointDirectory = dirname(resolve(process.argv[1]));
    } else {
      entryPointDirectory = resolve(entryPointDirectory);
    }
    this.entryPointDirectory = entryPointDirectory;
  }

  findFileUrl(url, context) {
    const parsed = parsePackageUrl(url);
    if (!parsed) return null;

    const baseDirectory = containingDirectory(context, this.entryPointDirectory);
    const packageRoot = packageRootFrom(baseDirectory, parsed.packageName);
    if (!packageRoot) return null;

    let manifest = {};
    const manifestPath = resolve(packageRoot, "package.json");
    if (existsSync(manifestPath)) {
      manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    }

    if (manifest.exports !== undefined) {
      return resolvePackageExport(packageRoot, manifest.exports, parsed.subpath, context?.fromImport === true);
    }

    if (parsed.subpath === "") {
      const entry = manifest.sass || manifest.style;
      if (entry) return packageTarget(packageRoot, entry, { fromImport: context?.fromImport === true });
      return pathToFileURL(resolve(packageRoot, "index")).href;
    }

    const subpath = resolve(packageRoot, parsed.subpath);
    if (relative(packageRoot, subpath).startsWith("..")) return null;
    return pathToFileURL(subpath).href;
  }
}

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
        importers: lowerNodePackageImporters(opts.importers),
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
        importers: lowerNodePackageImporters(opts.importers),
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
        importers: lowerNodePackageImporters(opts.importers),
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
        importers: lowerNodePackageImporters(opts.importers),
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
