export interface CompileResult {
  css: string;
  loadedUrls: URL[];
  sourceMap?: object;
}

export type OutputStyle = "expanded" | "compressed";

export interface Options {
  style?: OutputStyle;
  loadPaths?: string[];
  quiet?: boolean;
  quietDeps?: boolean;
  charset?: boolean;
  sourceMap?: boolean;
  sourceMapIncludeSources?: boolean;
  // TODO(#285): functions/importers options (native async surfaces support
  // them; not yet threaded through this Node entry point).
}

export interface StringOptions extends Options {}

export function compile(path: string, options?: Options): CompileResult;
export function compileString(source: string, options?: StringOptions): CompileResult;
export function compileAsync(path: string, options?: Options): Promise<CompileResult>;
export function compileStringAsync(source: string, options?: StringOptions): Promise<CompileResult>;
