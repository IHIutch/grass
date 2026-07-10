export interface CompileResult {
  css: string;
  loadedUrls: URL[];
  sourceMap?: object;
}

export type OutputStyle = "expanded" | "compressed";

export interface FsCallbacks {
  is_file(path: string): boolean;
  is_dir(path: string): boolean;
  read(path: string): number[] | Uint8Array;
  canonicalize(path: string): string;
}

/**
 * Note: this WASM-only entry point does not support `functions`,
 * `importers`, `importer`, or `url` — those require the native binding
 * (see the Node entry point's `Options`/`StringOptions` in `index.d.ts`).
 */
export interface Options {
  style?: OutputStyle;
  loadPaths?: string[];
  quiet?: boolean;
  quietDeps?: boolean;
  sourceMap?: boolean;
  sourceMapIncludeSources?: boolean;
  fs?: FsCallbacks;
}

export interface StringOptions extends Options {}

export function init(input?: BufferSource | WebAssembly.Module): Promise<void>;
export function compile(path: string, options: Options & { fs: FsCallbacks }): CompileResult;
export function compileString(source: string, options?: StringOptions): CompileResult;
export function compileAsync(path: string, options: Options & { fs: FsCallbacks }): Promise<CompileResult>;
export function compileStringAsync(source: string, options?: StringOptions): Promise<CompileResult>;
