#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::HashMap;

use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;

use grass_compiler::{
    from_path_with_source_map, from_string_with_source_map, from_string_with_url_and_source_map,
    Deprecation, Options, OutputStyle, SourceMapData,
};

mod functions;
mod importers;
mod values;
mod wire;

pub use values::{SassList, SassNumber, SassNumberUnits, SassString};

/// A Dart Sass compiler version, per the Sass JS API's `Version` class
/// (`major`/`minor`/`patch`, e.g. `new sass.Version(1, 95, 0)`).
///
/// Only meaningful in `CompileOptions.fatal_deprecations` — the real JS API's
/// `silenceDeprecations`/`futureDeprecations` accept `DeprecationOrId` only,
/// never `Version` (verified against the real `sass` npm package's type
/// declarations, `types/options.d.ts`: `fatalDeprecations?: (DeprecationOrId
/// | Version)[]` vs. `silenceDeprecations?: DeprecationOrId[]`).
///
/// This is a plain `#[napi(object)]`, so it also structurally accepts any
/// `{major, minor, patch}`-shaped plain object, not just a constructed
/// `Version` instance — property access can't distinguish the two, and the
/// real API's semantics don't depend on it being a `Version` per se.
#[napi(object)]
pub struct DeprecationVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

/// The `context` argument passed to a JS `importers` entry's
/// `canonicalize`/`findFileUrl` method, per the Sass JS API's
/// `CanonicalizeContext`. Never constructed from this struct directly (the
/// actual context object handed to JS is built by hand in `importers.rs`,
/// which needs no `FromNapiValue`/`ToNapiValue` round trip for it) — this
/// type exists purely so `napi build` emits a named `CanonicalizeContext`
/// TypeScript interface instead of `importers`' `ts_type` needing to inline
/// the shape.
#[napi(object)]
pub struct CanonicalizeContext {
    pub from_import: bool,
    pub containing_url: Option<String>,
}

// `object_to_js = false`: `CompileOptions` is only ever received from JS,
// never returned to it. This matters because `functions` (added below)
// holds a `JsFunctionRef`, which only implements `FromNapiValue` (see
// `functions.rs`) — a `ToNapiValue` impl would need a way to hand a live
// callback *back* to JS as a plain value, which nothing here needs.
#[napi(object, object_to_js = false)]
pub struct CompileOptions {
    pub style: Option<String>,
    pub load_paths: Option<Vec<String>>,
    pub quiet: Option<bool>,
    pub charset: Option<bool>,
    /// Deprecation IDs to silence, per the Sass JS API's `silenceDeprecations`
    /// option (string-ID form only — the real API never accepts `Version`
    /// here, see `DeprecationVersion`'s doc comment).
    pub silence_deprecations: Option<Vec<String>>,
    /// Deprecations to treat as fatal errors, per the Sass JS API's
    /// `fatalDeprecations` option. Each entry is either a string ID (e.g.
    /// `"slash-div"`) or a `{major, minor, patch}` version, which fatalizes
    /// every deprecation introduced at or before that version (dart-sass's
    /// `Deprecation.forVersion`; mirrors `--fatal-deprecation=<version>`'s
    /// range-expansion form in `crates/lib/src/main.rs`). The boundary is
    /// inclusive, verified against the real `sass` npm package (1.97.3):
    /// `Version(1, 95, 0)` fatalizes `if-function`, introduced in exactly
    /// 1.95.0; `Version(1, 94, 9)` does not.
    ///
    /// A version-shaped *string* here (e.g. `"1.95.0"`) is NOT parsed as a
    /// version — verified against the real API, which warns
    /// (`WARNING: Invalid deprecation "1.95.0".`) and continues, same as any
    /// other unrecognized ID.
    pub fatal_deprecations: Option<Vec<Either<String, DeprecationVersion>>>,
    /// Deprecation IDs to opt into early, per the Sass JS API's
    /// `futureDeprecations` option.
    pub future_deprecations: Option<Vec<String>>,
    /// Whether to generate a source map, per the Sass JS API's `sourceMap`
    /// option. When `false`/omitted (the default), `CompileResult.sourceMap`
    /// is absent, matching `sass.compile(..., {})`'s result having no
    /// `sourceMap` key at all (verified via the real `sass` npm package).
    pub source_map: Option<bool>,
    /// Whether to embed the verbatim source text in the generated map's
    /// `sourcesContent`, per the Sass JS API's `sourceMapIncludeSources`
    /// option. Has no effect unless `sourceMap` is also `true`.
    pub source_map_include_sources: Option<bool>,
    /// Custom Sass functions callable from stylesheets, per the Sass JS
    /// API's `functions` option (todo #221 slices 2-3). Keys are full
    /// function signatures (e.g. `"sum($a, $b)"` — the same grammar as an
    /// `@function` parameter list, including `$rest...`); values are JS
    /// callbacks invoked with pre-bound, declaration-ordered arguments. See
    /// `SassNumber`/`SassString`/`SassList` for the supported argument/
    /// return value shapes — `SassColor`/`SassMap`/`SassCalculation`/
    /// `SassFunction`/`SassMixin` are not yet supported and produce a clear
    /// compile error if a callback receives or returns one.
    ///
    /// Supported by all four entry points (`compile`/`compileString`/
    /// `compileAsync`/`compileStringAsync`). `compileAsync`/
    /// `compileStringAsync` use a different calling convention under the
    /// hood (`ThreadsafeFunction` + a blocking channel round-trip, since
    /// `Task::compute()` runs off the JS thread — see
    /// `crates/napi/src/functions.rs`'s module doc comment) with one
    /// additional constraint the sync entries don't have: a callback that
    /// is itself `async`/returns a `Promise` is not supported and produces
    /// a clear compile error rather than being awaited (real dart-sass
    /// awaits it; grass's blocking-channel bridge cannot do so safely yet).
    #[napi(
        ts_type = "Record<string, (args: Array<SassNumber | SassString | SassList | boolean | null>) => SassNumber | SassString | SassList | boolean | null | Array<unknown>>"
    )]
    pub functions: Option<HashMap<String, functions::JsFunctionRef>>,
    /// Custom import resolvers for `@use`/`@forward`/`@import`, per the Sass
    /// JS API's `importers` option (todo #221 slices 4-5b). Checked in array
    /// order, ahead of `loadPaths`. Two mutually-exclusive shapes per entry:
    /// a `FileImporter` (`{findFileUrl(url, context)}`, may return a `file:`
    /// URL string — or any string, treated as a path, an ergonomic
    /// relaxation beyond the real API — or `null`/`undefined` to decline;
    /// the compiler then applies normal partial/extension/index-file
    /// resolution on top, exactly like a load path) or a full `Importer`
    /// (`{canonicalize(url, context), load(canonicalUrl),
    /// nonCanonicalScheme?: string | string[]}`, arbitrary non-`file:`
    /// schemes: `canonicalize` returns a canonical URL string or
    /// `null`/`undefined` to decline; if a URL, `load` is called with it and
    /// must return `{contents, syntax: 'scss'|'sass'|'css'}` or
    /// `null`/`undefined`).
    ///
    /// Supported by all four entry points (`compile`/`compileString`/
    /// `compileAsync`/`compileStringAsync`). `compileAsync`/
    /// `compileStringAsync` use the same `ThreadsafeFunction` + blocking
    /// channel calling convention as `functions` (see `importers.rs`'s
    /// module doc comment) — a full `Importer` needs two sequential round
    /// trips (`canonicalize` then `load`) per resolution attempt — with the
    /// same constraint: a `canonicalize`/`load`/`findFileUrl` that is itself
    /// `async`/returns a `Promise` is not supported and produces a clear
    /// compile error rather than being awaited.
    #[napi(
        ts_type = "Array<{ findFileUrl(url: string, context: CanonicalizeContext): string | null | undefined } | { canonicalize(url: string, context: CanonicalizeContext): string | null | undefined, load(canonicalUrl: string): { contents: string, syntax: 'scss' | 'sass' | 'css' } | null | undefined, nonCanonicalScheme?: string | string[] }>"
    )]
    pub importers: Option<Vec<importers::ImporterRef>>,
    /// Entrypoint canonical URL for `compileString`/`compileStringAsync`, per
    /// the Sass JS API's `StringOptions.url`. Seeds the base for the source
    /// string's own relative `@use`/`@import` (it is the `containingUrl`
    /// handed to custom importers for the entry's loads) and the source map's
    /// entrypoint `sources` entry. Ignored by the path entry points. When
    /// omitted, `compileString` behaves exactly as before (a synthetic
    /// `stdin`/`data:` entry).
    pub url: Option<String>,
    /// Entrypoint importer for `compileString`/`compileStringAsync`, per the
    /// Sass JS API's `StringOptions.importer` — the resolver consulted for the
    /// source string's OWN relative loads. Same two shapes as `importers`
    /// (`FileImporter` or full `Importer`), registered ahead of `importers`.
    /// Ignored by the path entry points.
    #[napi(
        ts_type = "{ findFileUrl(url: string, context: CanonicalizeContext): string | null | undefined } | { canonicalize(url: string, context: CanonicalizeContext): string | null | undefined, load(canonicalUrl: string): { contents: string, syntax: 'scss' | 'sass' | 'css' } | null | undefined, nonCanonicalScheme?: string | string[] }"
    )]
    pub importer: Option<importers::ImporterRef>,
}

#[napi(object)]
pub struct CompileResult {
    pub css: String,
    /// Present only when `CompileOptions.sourceMap` was `true`. Shaped like
    /// the real Sass JS API's `sourceMap` result (`{version, sourceRoot,
    /// sources, names, mappings}`, optionally `sourcesContent` — verified
    /// via `sass.compileString(..., {sourceMap: true})`, whose result never
    /// has a `file` key; that field is CLI-only).
    pub source_map: Option<serde_json::Value>,
}

/// Builds the JS-API-shaped `sourceMap` result value: `None` when maps
/// weren't requested at all, `Some` otherwise (never a `file` key, per the
/// JS API contract — see `CompileResult::source_map`'s doc comment).
fn source_map_result(
    map: Option<SourceMapData>,
    include_sources: bool,
) -> Option<serde_json::Value> {
    map.map(|m| m.to_json_value(None, include_sources))
}

/// dart-sass's JS API returns CSS without a trailing newline, while grass's
/// Rust/CLI surface includes one (matching dart-sass's CLI). The JS bindings
/// must strip exactly one to preserve JS-API parity.
fn js_css(mut css: String) -> String {
    if css.ends_with('\n') {
        css.pop();
    }
    css
}

// `AssertUnwindSafe`/`UnwindSafe` bounds below are sound: these closures only
// read owned inputs, and on panic the compiler state being unwound through is
// discarded rather than reused, so there is no observable broken invariant.
fn catch<T>(f: impl FnOnce() -> napi::Result<T> + std::panic::UnwindSafe) -> napi::Result<T> {
    match std::panic::catch_unwind(f) {
        Ok(r) => r,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "internal compiler panic".to_string());
            Err(napi::Error::from_reason(format!(
                "grass internal error: {msg}"
            )))
        }
    }
}

/// Resolves a `silenceDeprecations`/`fatalDeprecations`/`futureDeprecations`
/// array of string IDs, per the Sass JS API. Unlike the CLI
/// (crates/lib/src/main.rs, which hard-errors), the real JS API prints
/// `WARNING: Invalid deprecation "<id>".` to stderr for an unrecognized ID
/// and continues compiling, simply ignoring that ID (verified against the
/// real `sass` npm package, 1.97.3, via `compileString`). This binding
/// matches that: unrecognized IDs are dropped with a matching warning rather
/// than failing the compile.
fn resolve_deprecation_ids(ids: &[String]) -> Vec<Deprecation> {
    ids.iter()
        .filter_map(|id| {
            let resolved = Deprecation::from_id(id);
            if resolved.is_none() {
                eprintln!("WARNING: Invalid deprecation \"{id}\".");
            }
            resolved
        })
        .collect()
}

/// Resolves a `fatalDeprecations` array, which — unlike
/// `silenceDeprecations`/`futureDeprecations` — accepts `Version` entries
/// alongside string IDs (see `DeprecationVersion`'s doc comment). A `Version`
/// entry expands to every deprecation introduced at or before it, mirroring
/// `crates/lib/src/main.rs`'s `--fatal-deprecation=<version>` handling; a
/// string entry goes through the same warn-and-continue lookup as
/// `resolve_deprecation_ids`.
fn resolve_fatal_deprecations(entries: &[Either<String, DeprecationVersion>]) -> Vec<Deprecation> {
    let mut resolved = Vec::new();
    for entry in entries {
        match entry {
            Either::A(id) => {
                if let Some(deprecation) = Deprecation::from_id(id) {
                    resolved.push(deprecation);
                } else {
                    eprintln!("WARNING: Invalid deprecation \"{id}\".");
                }
            }
            Either::B(version) => {
                resolved.extend(Deprecation::for_version((
                    version.major,
                    version.minor,
                    version.patch,
                )));
            }
        }
    }
    resolved
}

fn build_options(opts: Option<CompileOptions>) -> napi::Result<Options<'static>> {
    let mut options = Options::default();

    if let Some(opts) = opts {
        if let Some(ref style) = opts.style {
            if style == "compressed" {
                options = options.style(OutputStyle::Compressed);
            }
        }

        if let Some(ref paths) = opts.load_paths {
            for p in paths {
                options = options.load_path(p);
            }
        }

        if let Some(quiet) = opts.quiet {
            options = options.quiet(quiet);
        }

        if let Some(charset) = opts.charset {
            options = options.allows_charset(charset);
        }

        if let Some(ref ids) = opts.silence_deprecations {
            for deprecation in resolve_deprecation_ids(ids) {
                options = options.silence_deprecation(deprecation);
            }
        }

        if let Some(ref entries) = opts.fatal_deprecations {
            for deprecation in resolve_fatal_deprecations(entries) {
                options = options.fatal_deprecation(deprecation);
            }
        }

        if let Some(ref ids) = opts.future_deprecations {
            for deprecation in resolve_deprecation_ids(ids) {
                options = options.future_deprecation(deprecation);
            }
        }

        if let Some(source_map) = opts.source_map {
            options = options.source_map(source_map);
        }
    }

    Ok(options)
}

/// Pops `functions`/`importers` off of `options` (if present) and registers
/// each onto `opts` via their sync-only bridges (`functions.rs`/
/// `importers.rs`). Only safe to call from `compile`/`compileString` — see
/// those modules' doc comments.
fn build_options_with_functions(
    mut options: Option<CompileOptions>,
) -> napi::Result<Options<'static>> {
    let funcs = options.as_mut().and_then(|o| o.functions.take());
    let importer = options.as_mut().and_then(|o| o.importer.take());
    let imps = options.as_mut().and_then(|o| o.importers.take());
    let opts = build_options(options)?;

    let opts = match funcs {
        Some(f) if !f.is_empty() => functions::register_functions(opts, f)?,
        _ => opts,
    };

    // Entrypoint importer (`StringOptions.importer`) first, then the
    // `importers` array — a single importer is just a one-element registration.
    let opts = match importer {
        Some(imp) => importers::register_importers(opts, vec![imp]),
        None => opts,
    };

    let opts = match imps {
        Some(list) if !list.is_empty() => importers::register_importers(opts, list),
        _ => opts,
    };

    Ok(opts)
}

#[napi]
pub fn compile(path: String, options: Option<CompileOptions>) -> napi::Result<CompileResult> {
    catch(|| {
        let include_sources = options
            .as_ref()
            .and_then(|o| o.source_map_include_sources)
            .unwrap_or(false);
        let opts = build_options_with_functions(options)?;

        let (css, map) = from_path_with_source_map(&path, &opts)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult {
            css: js_css(css),
            source_map: source_map_result(map, include_sources),
        })
    })
}

#[napi]
pub fn compile_string(
    source: String,
    options: Option<CompileOptions>,
) -> napi::Result<CompileResult> {
    catch(|| {
        let include_sources = options
            .as_ref()
            .and_then(|o| o.source_map_include_sources)
            .unwrap_or(false);
        let url = options.as_ref().and_then(|o| o.url.clone());
        let opts = build_options_with_functions(options)?;

        // With `url`, seed the entry's canonical URL / relative-import base
        // (and the source-map `sources` entry). Without it, keep the existing
        // synthetic `stdin`/`data:` behavior.
        let (css, map) = match url.as_deref() {
            Some(u) => from_string_with_url_and_source_map(source, u, &opts),
            None => from_string_with_source_map(source, &opts),
        }
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult {
            css: js_css(css),
            source_map: source_map_result(map, include_sources),
        })
    })
}

/// Pops `functions` off of `options` (if present) and upgrades each JS
/// callback into a [`functions::AsyncJsFunctionRef`] (todo #221 slice 3) —
/// a JS-thread-only step (`JsFunctionRef::into_threadsafe` needs `Env` to
/// build a `ThreadsafeFunction`), so this must run in `compile_async`/
/// `compile_string_async`'s synchronous body, before the `AsyncTask`/
/// `Task::compute()` (which runs off the JS thread) is ever constructed.
/// The actual registration onto `Options` (`functions::register_functions_async`)
/// happens later, inside `compute()` — that part touches no `Env` and is
/// safe there.
fn take_async_functions(
    options: &mut Option<CompileOptions>,
) -> napi::Result<Option<HashMap<String, functions::AsyncJsFunctionRef>>> {
    let funcs = options.as_mut().and_then(|o| o.functions.take());

    match funcs {
        Some(f) if !f.is_empty() => {
            let mut upgraded = HashMap::with_capacity(f.len());
            for (signature, func_ref) in f {
                upgraded.insert(signature, func_ref.into_threadsafe()?);
            }
            Ok(Some(upgraded))
        }
        _ => Ok(None),
    }
}

/// Pops `importers` off of `options` (if present) and upgrades each entry
/// into its async counterpart (todo #221 slice 5b) — a JS-thread-only step
/// (`ImporterRef::into_threadsafe` needs `Env` to build a
/// `ThreadsafeFunction`), so this must run in `compile_async`/
/// `compile_string_async`'s synchronous body, before the `AsyncTask`/
/// `Task::compute()` (which runs off the JS thread) is ever constructed.
/// The actual registration onto `Options`
/// (`importers::register_importers_async`) happens later, inside
/// `compute()` — that part touches no `Env` and is safe there.
fn take_async_importers(
    options: &mut Option<CompileOptions>,
) -> napi::Result<Option<Vec<importers::AsyncImporterRef>>> {
    let imps = options.as_mut().and_then(|o| o.importers.take());

    match imps {
        Some(list) if !list.is_empty() => {
            let mut upgraded = Vec::with_capacity(list.len());
            for imp in list {
                upgraded.push(imp.into_threadsafe()?);
            }
            Ok(Some(upgraded))
        }
        _ => Ok(None),
    }
}

/// Singular-`importer` counterpart to [`take_async_importers`] — upgrades the
/// entrypoint `importer` (if any) to its async form on the JS thread, before
/// the `AsyncTask` is constructed. See `take_async_importers`' doc comment for
/// why this must run here and not inside `compute()`.
fn take_async_importer(
    options: &mut Option<CompileOptions>,
) -> napi::Result<Option<importers::AsyncImporterRef>> {
    match options.as_mut().and_then(|o| o.importer.take()) {
        Some(imp) => Ok(Some(imp.into_threadsafe()?)),
        None => Ok(None),
    }
}

pub struct CompileTask {
    path: String,
    options: Option<CompileOptions>,
    async_functions: Option<HashMap<String, functions::AsyncJsFunctionRef>>,
    async_importers: Option<Vec<importers::AsyncImporterRef>>,
    /// Read once at task-construction time, since `compute()` consumes
    /// `options` via `take()` before `resolve()` runs.
    include_sources: bool,
}

impl Task for CompileTask {
    type Output = (String, Option<SourceMapData>);
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let path = &self.path;
        let opts = build_options(self.options.take())?;
        let opts = match self.async_functions.take() {
            Some(f) if !f.is_empty() => functions::register_functions_async(opts, f)?,
            _ => opts,
        };
        let opts = match self.async_importers.take() {
            Some(list) if !list.is_empty() => importers::register_importers_async(opts, list),
            _ => opts,
        };
        catch(std::panic::AssertUnwindSafe(|| {
            from_path_with_source_map(path, &opts)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let (css, map) = output;
        Ok(CompileResult {
            css: js_css(css),
            source_map: source_map_result(map, self.include_sources),
        })
    }
}

pub struct CompileStringTask {
    source: String,
    options: Option<CompileOptions>,
    async_functions: Option<HashMap<String, functions::AsyncJsFunctionRef>>,
    async_importers: Option<Vec<importers::AsyncImporterRef>>,
    async_importer: Option<importers::AsyncImporterRef>,
    url: Option<String>,
    /// Read once at task-construction time, since `compute()` consumes
    /// `options` via `take()` before `resolve()` runs.
    include_sources: bool,
}

impl Task for CompileStringTask {
    type Output = (String, Option<SourceMapData>);
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take())?;
        let opts = match self.async_functions.take() {
            Some(f) if !f.is_empty() => functions::register_functions_async(opts, f)?,
            _ => opts,
        };
        let opts = match self.async_importer.take() {
            Some(imp) => importers::register_importers_async(opts, vec![imp]),
            None => opts,
        };
        let opts = match self.async_importers.take() {
            Some(list) if !list.is_empty() => importers::register_importers_async(opts, list),
            _ => opts,
        };
        let source = std::mem::take(&mut self.source);
        let url = self.url.take();
        catch(std::panic::AssertUnwindSafe(|| {
            match url.as_deref() {
                Some(u) => from_string_with_url_and_source_map(source, u, &opts),
                None => from_string_with_source_map(source, &opts),
            }
            .map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let (css, map) = output;
        Ok(CompileResult {
            css: js_css(css),
            source_map: source_map_result(map, self.include_sources),
        })
    }
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_async(
    path: String,
    mut options: Option<CompileOptions>,
) -> napi::Result<AsyncTask<CompileTask>> {
    let include_sources = options
        .as_ref()
        .and_then(|o| o.source_map_include_sources)
        .unwrap_or(false);
    let async_functions = take_async_functions(&mut options)?;
    let async_importers = take_async_importers(&mut options)?;
    Ok(AsyncTask::new(CompileTask {
        path,
        options,
        async_functions,
        async_importers,
        include_sources,
    }))
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_string_async(
    source: String,
    mut options: Option<CompileOptions>,
) -> napi::Result<AsyncTask<CompileStringTask>> {
    let include_sources = options
        .as_ref()
        .and_then(|o| o.source_map_include_sources)
        .unwrap_or(false);
    let url = options.as_ref().and_then(|o| o.url.clone());
    let async_functions = take_async_functions(&mut options)?;
    let async_importer = take_async_importer(&mut options)?;
    let async_importers = take_async_importers(&mut options)?;
    Ok(AsyncTask::new(CompileStringTask {
        source,
        options,
        async_functions,
        async_importer,
        async_importers,
        url,
        include_sources,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_opts() -> CompileOptions {
        CompileOptions {
            style: None,
            load_paths: None,
            quiet: None,
            charset: None,
            silence_deprecations: None,
            fatal_deprecations: None,
            future_deprecations: None,
            source_map: None,
            source_map_include_sources: None,
            functions: None,
            importers: None,
            url: None,
            importer: None,
        }
    }

    #[test]
    fn compile_string_produces_css() {
        let res = compile_string("a { b: c }".to_owned(), None).unwrap();
        assert_eq!(res.css, "a {\n  b: c;\n}");
    }

    #[test]
    fn compile_string_compressed_style() {
        let opts = CompileOptions {
            style: Some("compressed".to_owned()),
            ..base_opts()
        };
        let res = compile_string("a { b: c }".to_owned(), Some(opts)).unwrap();
        assert_eq!(res.css, "a{b:c}");
    }

    #[test]
    fn compile_string_silence_deprecations_removes_warning() {
        let source = "$a: 1;\nb { c: $a/2; }".to_owned();

        let with_warning = compile_string(source.clone(), None).unwrap();
        assert_eq!(with_warning.css, "b {\n  c: 0.5;\n}");

        let opts = CompileOptions {
            silence_deprecations: Some(vec!["slash-div".to_owned()]),
            ..base_opts()
        };
        let res = compile_string(source, Some(opts)).unwrap();
        assert_eq!(res.css, "b {\n  c: 0.5;\n}");
    }

    #[test]
    fn compile_string_fatal_deprecations_errors() {
        let opts = CompileOptions {
            fatal_deprecations: Some(vec![Either::A("slash-div".to_owned())]),
            ..base_opts()
        };
        let res = compile_string("$a: 1;\nb { c: $a/2; }".to_owned(), Some(opts));
        assert!(res.is_err());
    }

    #[test]
    fn compile_string_fatal_deprecations_version_expands_range() {
        // if-function was introduced in exactly Dart Sass 1.95.0 (verified
        // against the real `sass` npm package, 1.97.3: `fatalDeprecations:
        // [new sass.Version(1, 95, 0)]` fatalizes it, `Version(1, 94, 9)`
        // does not).
        let source = "a { b: if(true, 1, 2) }".to_owned();

        let below = CompileOptions {
            fatal_deprecations: Some(vec![Either::B(DeprecationVersion {
                major: 1,
                minor: 94,
                patch: 9,
            })]),
            ..base_opts()
        };
        assert!(compile_string(source.clone(), Some(below)).is_ok());

        let at_boundary = CompileOptions {
            fatal_deprecations: Some(vec![Either::B(DeprecationVersion {
                major: 1,
                minor: 95,
                patch: 0,
            })]),
            ..base_opts()
        };
        assert!(compile_string(source, Some(at_boundary)).is_err());
    }

    #[test]
    fn compile_string_fatal_deprecations_mixed_string_and_version() {
        // A bogus string ID and a Version entry in the same array: the bogus
        // ID warns-and-continues (no effect) while the Version still expands
        // to fatalize `if-function` (introduced 1.95.0, included in the
        // 1.95.0 range) — verified against the real `sass` npm package.
        let opts = CompileOptions {
            fatal_deprecations: Some(vec![
                Either::A("bogus-id".to_owned()),
                Either::B(DeprecationVersion {
                    major: 1,
                    minor: 95,
                    patch: 0,
                }),
            ]),
            ..base_opts()
        };
        let res = compile_string("a { b: if(true, 1, 2) }".to_owned(), Some(opts));
        assert!(res.is_err());
    }

    #[test]
    fn compile_string_unknown_deprecation_id_warns_and_continues() {
        // Matches the real `sass` JS API (verified via `compileString` against
        // the npm package, 1.97.3): an unrecognized deprecation ID is not a
        // hard error, it's ignored with a `WARNING: Invalid deprecation "…".`
        // printed to stderr (not observable from a Rust unit test, but the
        // compile itself must still succeed).
        for field in [
            "silence_deprecations",
            "fatal_deprecations",
            "future_deprecations",
        ] {
            let opts = match field {
                "silence_deprecations" => CompileOptions {
                    silence_deprecations: Some(vec!["bogus-id".to_owned()]),
                    ..base_opts()
                },
                "fatal_deprecations" => CompileOptions {
                    fatal_deprecations: Some(vec![Either::A("bogus-id".to_owned())]),
                    ..base_opts()
                },
                _ => CompileOptions {
                    future_deprecations: Some(vec!["bogus-id".to_owned()]),
                    ..base_opts()
                },
            };
            let res = compile_string("a { b: c }".to_owned(), Some(opts));
            assert!(res.is_ok(), "field {field} unexpectedly errored");
            assert_eq!(res.unwrap().css, "a {\n  b: c;\n}");
        }
    }

    #[test]
    fn compile_string_unknown_deprecation_id_alongside_valid_one() {
        // A bogus ID mixed with a real one: the real one still takes effect
        // (verified against dart-sass: `fatalDeprecations: ["slash-div",
        // "bogus-id"]` both warns AND fatalizes on `slash-div`).
        let opts = CompileOptions {
            fatal_deprecations: Some(vec![
                Either::A("slash-div".to_owned()),
                Either::A("bogus-id".to_owned()),
            ]),
            ..base_opts()
        };
        let res = compile_string("$a: 1;\nb { c: $a/2; }".to_owned(), Some(opts));
        assert!(res.is_err());
    }

    #[test]
    fn compile_string_invalid_input_is_err_not_panic() {
        assert!(compile_string("a { b: ".to_owned(), None).is_err());
    }

    #[test]
    fn compile_task_compute_ok_and_err() {
        let mut task = CompileStringTask {
            source: "a { b: c }".to_owned(),
            options: None,
            async_functions: None,
            async_importers: None,
            async_importer: None,
            url: None,
            include_sources: false,
        };
        assert!(task.compute().is_ok());
        let mut bad = CompileStringTask {
            source: "a {".to_owned(),
            options: None,
            async_functions: None,
            async_importers: None,
            async_importer: None,
            url: None,
            include_sources: false,
        };
        assert!(bad.compute().is_err());
    }

    #[test]
    fn compile_string_source_map_absent_by_default() {
        let res = compile_string("a { b: c }".to_owned(), None).unwrap();
        assert!(res.source_map.is_none());
    }

    #[test]
    fn compile_string_source_map_shape_matches_js_api() {
        // Verified against `sass.compileString('a { b: c; }', {sourceMap:
        // true})`: result keys are exactly `{css, sourceMap, loadedUrls}`,
        // and `sourceMap` is `{version, sourceRoot, sources, names,
        // mappings}` with sources[0] a `data:` URL (no `url` option given)
        // and NO `file` key (unlike the CLI's written .map file).
        let opts = CompileOptions {
            source_map: Some(true),
            ..base_opts()
        };
        let res = compile_string("a {\n  b: c;\n}\n".to_owned(), Some(opts)).unwrap();
        let map = res
            .source_map
            .expect("source_map must be Some when requested");

        assert_eq!(map["version"], 3);
        assert_eq!(map["sourceRoot"], "");
        assert_eq!(map["names"], serde_json::json!([]));
        assert_eq!(map["mappings"], "AAAA;EACE");
        assert!(
            map.get("file").is_none(),
            "JS API shape must omit file, got: {map}"
        );
        let sources = map["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 1);
        assert!(
            sources[0]
                .as_str()
                .unwrap()
                .starts_with("data:;charset=utf-8,"),
            "got: {map}"
        );
        assert!(map.get("sourcesContent").is_none());
    }

    #[test]
    fn compile_string_source_map_include_sources_adds_sources_content() {
        let opts = CompileOptions {
            source_map: Some(true),
            source_map_include_sources: Some(true),
            ..base_opts()
        };
        let res = compile_string("a {\n  b: c;\n}\n".to_owned(), Some(opts)).unwrap();
        let map = res.source_map.unwrap();
        assert_eq!(
            map["sourcesContent"],
            serde_json::json!(["a {\n  b: c;\n}\n"])
        );
    }

    #[test]
    fn compile_string_async_source_map_matches_sync() {
        let opts = CompileOptions {
            source_map: Some(true),
            ..base_opts()
        };
        let mut task = CompileStringTask {
            source: "a {\n  b: c;\n}\n".to_owned(),
            options: Some(opts),
            async_functions: None,
            async_importers: None,
            async_importer: None,
            url: None,
            include_sources: false,
        };
        let output = task.compute().unwrap();
        assert!(output.1.is_some());
    }

    /// Returns a path under the OS temp dir that's unique to this test
    /// process/thread, so parallel `cargo test` runs never collide.
    fn unique_temp_path(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "grass_napi_test_{}_{}_{name}",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn compile_on_real_temp_file() {
        let path = unique_temp_path("real.scss");
        std::fs::write(&path, "a { b: c }").unwrap();

        let res = compile(path.to_string_lossy().into_owned(), None).unwrap();
        assert_eq!(res.css, "a {\n  b: c;\n}");

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn compile_missing_path_is_err_not_panic() {
        let path = unique_temp_path("does_not_exist.scss");
        assert!(!path.exists());

        let res = compile(path.to_string_lossy().into_owned(), None);
        assert!(res.is_err());
    }

    // Mirrors `crates/lib/tests/cli.rs`'s `fatal_wins_over_silence_for_same_id`
    // — verified against the real `sass` npm package (1.97.3):
    // `compileString(..., {fatalDeprecations: ["slash-div"],
    // silenceDeprecations: ["slash-div"]})` still throws (fatal wins), same
    // as the CLI's `--fatal-deprecation=slash-div
    // --silence-deprecation=slash-div` precedence.
    #[test]
    fn compile_string_fatal_wins_over_silence_for_same_id() {
        let opts = CompileOptions {
            fatal_deprecations: Some(vec![Either::A("slash-div".to_owned())]),
            silence_deprecations: Some(vec!["slash-div".to_owned()]),
            ..base_opts()
        };
        let res = compile_string("$a: 1;\nb { c: $a/2; }".to_owned(), Some(opts));
        assert!(res.is_err());
    }
}
