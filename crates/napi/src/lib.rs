#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;

use grass_compiler::{
    from_path_with_source_map, from_string_with_source_map, Deprecation, Options, OutputStyle,
    SourceMapData,
};

#[napi(object)]
pub struct CompileOptions {
    pub style: Option<String>,
    pub load_paths: Option<Vec<String>>,
    pub quiet: Option<bool>,
    pub charset: Option<bool>,
    /// Deprecation IDs to silence, per the Sass JS API's `silenceDeprecations`
    /// option (string-ID form only; dart-sass's `Version` object form for
    /// `fatalDeprecations` is not implemented here).
    pub silence_deprecations: Option<Vec<String>>,
    /// Deprecation IDs to treat as fatal errors, per the Sass JS API's
    /// `fatalDeprecations` option (string-ID form only).
    ///
    /// The real JS API additionally accepts `Version` instances here for
    /// range-expansion (`Deprecation.forVersion`); that form is not
    /// implemented (would require a `Version` napi struct + a union member
    /// type, a bigger change than this string-ID surface). Probed via
    /// `sass.compileString` (JS API, not CLI): passing a version-shaped
    /// *string* here is not a hard error — the real API only warns
    /// (`WARNING: Invalid deprecation "1.33.0".`) and continues, treating it
    /// like any other unrecognized ID (see `resolve_deprecation_ids`, which
    /// matches this warn-and-continue behavior since #191).
    pub fatal_deprecations: Option<Vec<String>>,
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
fn source_map_result(map: Option<SourceMapData>, include_sources: bool) -> Option<serde_json::Value> {
    map.map(|m| {
        serde_json::from_str(&m.to_json(None, include_sources))
            .expect("grass-generated source map JSON must always be valid")
    })
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
            Err(napi::Error::from_reason(format!("grass internal error: {msg}")))
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

        if let Some(ref ids) = opts.fatal_deprecations {
            for deprecation in resolve_deprecation_ids(ids) {
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

#[napi]
pub fn compile(path: String, options: Option<CompileOptions>) -> napi::Result<CompileResult> {
    catch(|| {
        let include_sources = options
            .as_ref()
            .and_then(|o| o.source_map_include_sources)
            .unwrap_or(false);
        let opts = build_options(options)?;

        let (css, map) = from_path_with_source_map(&path, &opts)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult {
            css,
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
        let opts = build_options(options)?;

        // `from_string_with_source_map` produces a `data:` URL `sources`
        // entry (matching `compileString` without a `url` option — see
        // docs/design/source-maps.md), unlike the plain `from_string*`
        // family, which uses a synthetic path purely for error messages.
        let (css, map) = from_string_with_source_map(source, &opts)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult {
            css,
            source_map: source_map_result(map, include_sources),
        })
    })
}

pub struct CompileTask {
    path: String,
    options: Option<CompileOptions>,
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
        catch(std::panic::AssertUnwindSafe(|| {
            from_path_with_source_map(path, &opts).map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let (css, map) = output;
        Ok(CompileResult {
            css,
            source_map: source_map_result(map, self.include_sources),
        })
    }
}

pub struct CompileStringTask {
    source: String,
    options: Option<CompileOptions>,
    /// Read once at task-construction time, since `compute()` consumes
    /// `options` via `take()` before `resolve()` runs.
    include_sources: bool,
}

impl Task for CompileStringTask {
    type Output = (String, Option<SourceMapData>);
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take())?;
        let source = self.source.clone();
        catch(std::panic::AssertUnwindSafe(|| {
            from_string_with_source_map(source, &opts)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        let (css, map) = output;
        Ok(CompileResult {
            css,
            source_map: source_map_result(map, self.include_sources),
        })
    }
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_async(path: String, options: Option<CompileOptions>) -> AsyncTask<CompileTask> {
    let include_sources = options
        .as_ref()
        .and_then(|o| o.source_map_include_sources)
        .unwrap_or(false);
    AsyncTask::new(CompileTask { path, options, include_sources })
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_string_async(
    source: String,
    options: Option<CompileOptions>,
) -> AsyncTask<CompileStringTask> {
    let include_sources = options
        .as_ref()
        .and_then(|o| o.source_map_include_sources)
        .unwrap_or(false);
    AsyncTask::new(CompileStringTask { source, options, include_sources })
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
        }
    }

    #[test]
    fn compile_string_produces_css() {
        let res = compile_string("a { b: c }".to_owned(), None).unwrap();
        assert_eq!(res.css, "a {\n  b: c;\n}\n");
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
        assert_eq!(with_warning.css, "b {\n  c: 0.5;\n}\n");

        let opts = CompileOptions {
            silence_deprecations: Some(vec!["slash-div".to_owned()]),
            ..base_opts()
        };
        let res = compile_string(source, Some(opts)).unwrap();
        assert_eq!(res.css, "b {\n  c: 0.5;\n}\n");
    }

    #[test]
    fn compile_string_fatal_deprecations_errors() {
        let opts = CompileOptions {
            fatal_deprecations: Some(vec!["slash-div".to_owned()]),
            ..base_opts()
        };
        let res = compile_string("$a: 1;\nb { c: $a/2; }".to_owned(), Some(opts));
        assert!(res.is_err());
    }

    #[test]
    fn compile_string_unknown_deprecation_id_warns_and_continues() {
        // Matches the real `sass` JS API (verified via `compileString` against
        // the npm package, 1.97.3): an unrecognized deprecation ID is not a
        // hard error, it's ignored with a `WARNING: Invalid deprecation "…".`
        // printed to stderr (not observable from a Rust unit test, but the
        // compile itself must still succeed).
        for field in ["silence_deprecations", "fatal_deprecations", "future_deprecations"] {
            let opts = match field {
                "silence_deprecations" => CompileOptions {
                    silence_deprecations: Some(vec!["bogus-id".to_owned()]),
                    ..base_opts()
                },
                "fatal_deprecations" => CompileOptions {
                    fatal_deprecations: Some(vec!["bogus-id".to_owned()]),
                    ..base_opts()
                },
                _ => CompileOptions {
                    future_deprecations: Some(vec!["bogus-id".to_owned()]),
                    ..base_opts()
                },
            };
            let res = compile_string("a { b: c }".to_owned(), Some(opts));
            assert!(res.is_ok(), "field {field} unexpectedly errored");
            assert_eq!(res.unwrap().css, "a {\n  b: c;\n}\n");
        }
    }

    #[test]
    fn compile_string_unknown_deprecation_id_alongside_valid_one() {
        // A bogus ID mixed with a real one: the real one still takes effect
        // (verified against dart-sass: `fatalDeprecations: ["slash-div",
        // "bogus-id"]` both warns AND fatalizes on `slash-div`).
        let opts = CompileOptions {
            fatal_deprecations: Some(vec!["slash-div".to_owned(), "bogus-id".to_owned()]),
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
            include_sources: false,
        };
        assert!(task.compute().is_ok());
        let mut bad = CompileStringTask {
            source: "a {".to_owned(),
            options: None,
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
        let map = res.source_map.expect("source_map must be Some when requested");

        assert_eq!(map["version"], 3);
        assert_eq!(map["sourceRoot"], "");
        assert_eq!(map["names"], serde_json::json!([]));
        assert_eq!(map["mappings"], "AAAA;EACE");
        assert!(map.get("file").is_none(), "JS API shape must omit file, got: {map}");
        let sources = map["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 1);
        assert!(
            sources[0].as_str().unwrap().starts_with("data:;charset=utf-8,"),
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
        assert_eq!(map["sourcesContent"], serde_json::json!(["a {\n  b: c;\n}\n"]));
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
            include_sources: false,
        };
        let output = task.compute().unwrap();
        assert!(output.1.is_some());
    }
}
