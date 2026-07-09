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

export interface StringOptions {
  style?: OutputStyle;
  loadPaths?: string[];
  quiet?: boolean;
  quietDeps?: boolean;
  sourceMap?: boolean;
  sourceMapIncludeSources?: boolean;
  fs?: FsCallbacks;
}

export function init(wasmModule: WebAssembly.Module): void;
export function compileString(source: string, options?: StringOptions): CompileResult;
export function compileStringAsync(source: string, options?: StringOptions): Promise<CompileResult>;
