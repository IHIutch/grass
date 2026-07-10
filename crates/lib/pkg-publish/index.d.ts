export interface CompileResult {
  css: string;
  loadedUrls: URL[];
  sourceMap?: object;
}

export type OutputStyle = "expanded" | "compressed";

/**
 * `SassNumber(value, unit?)`'s second argument, matching the real Sass JS
 * API's `{numeratorUnits, denominatorUnits}` shape for compound units.
 */
export interface SassNumberUnits {
  numeratorUnits?: string[];
  denominatorUnits?: string[];
}

/**
 * A Sass number, mirroring the real Sass JS API's `SassNumber` class
 * (`value`, `numeratorUnits`, `denominatorUnits`). Constructed either with
 * a single unit string (`new SassNumber(1, "px")`) or explicit
 * numerator/denominator arrays for compound units
 * (`new SassNumber(1, {numeratorUnits: ["px"], denominatorUnits: ["s"]})`).
 */
export class SassNumber {
  value: number;
  numeratorUnits: string[];
  denominatorUnits: string[];
  constructor(value: number, unit?: string | SassNumberUnits | null);
}

/**
 * A Sass string, mirroring the real Sass JS API's `SassString` class
 * (`text`, `hasQuotes`). `hasQuotes` defaults to `true`, matching the real
 * API's default (an unquoted string must opt in explicitly).
 */
export class SassString {
  text: string;
  hasQuotes: boolean;
  constructor(text: string, hasQuotes?: boolean | null);
}

/**
 * A Sass list, mirroring the real Sass JS API's `SassList` class
 * (`contents`, `separator`, `brackets`). Unlike the real API, `separator`
 * is a readable word (`"comma"`/`"space"`/`"slash"`/`""` for undecided)
 * rather than a single-character token, to avoid ambiguity with an actual
 * comma/space/slash appearing as data — a grass-specific divergence.
 */
export class SassList {
  separator: string;
  brackets: boolean;
  constructor(contents: unknown[], separator?: string | null, brackets?: boolean | null);
  get contents(): unknown[];
}

/**
 * The `context` argument passed to a JS `importers` entry's
 * `canonicalize`/`findFileUrl` method, per the Sass JS API's
 * `CanonicalizeContext`.
 */
export interface CanonicalizeContext {
  fromImport: boolean;
  containingUrl?: string;
}

/**
 * Resolves `pkg:` URLs from Node packages and delegates stylesheet loading to
 * the native FileImporter bridge. This is available only from the Node entrypoint.
 */
export class NodePackageImporter {
  constructor(entryPointDirectory?: string);
}

export type FileImporter = {
  findFileUrl(url: string, context: CanonicalizeContext): string | null | undefined;
};

export type Importer = {
  canonicalize(url: string, context: CanonicalizeContext): string | null | undefined;
  load(canonicalUrl: string): { contents: string; syntax: "scss" | "sass" | "css" } | null | undefined;
};

export interface Options {
  style?: OutputStyle;
  loadPaths?: string[];
  quiet?: boolean;
  quietDeps?: boolean;
  charset?: boolean;
  sourceMap?: boolean;
  sourceMapIncludeSources?: boolean;
  /**
   * Custom Sass functions callable from stylesheets, per the Sass JS API's
   * `functions` option. Keys are full function signatures (e.g.
   * `"sum($a, $b)"`); values are JS callbacks invoked with pre-bound,
   * declaration-ordered arguments. Requires the native binding — throws on
   * the WASM fallback (see README).
   */
  functions?: Record<
    string,
    (args: Array<SassNumber | SassString | SassList | boolean | null>) =>
      | SassNumber
      | SassString
      | SassList
      | boolean
      | null
      | unknown[]
  >;
  /**
   * Custom import resolvers for `@use`/`@forward`/`@import`, per the Sass JS
   * API's `importers` option. Checked in array order, ahead of `loadPaths`.
   * Requires the native binding — throws on the WASM fallback (see README).
   */
  importers?: Array<FileImporter | Importer | NodePackageImporter>;
}

export interface StringOptions extends Options {
  /**
   * Entrypoint canonical URL for `compileString`/`compileStringAsync`, per
   * the Sass JS API's `StringOptions.url`. Requires the native binding —
   * throws on the WASM fallback (see README).
   */
  url?: string;
  /**
   * Entrypoint importer for `compileString`/`compileStringAsync`, per the
   * Sass JS API's `StringOptions.importer`. Requires the native binding —
   * throws on the WASM fallback (see README).
   */
  importer?: FileImporter | Importer;
}

export function compile(path: string, options?: Options): CompileResult;
export function compileString(source: string, options?: StringOptions): CompileResult;
export function compileAsync(path: string, options?: Options): Promise<CompileResult>;
export function compileStringAsync(source: string, options?: StringOptions): Promise<CompileResult>;
