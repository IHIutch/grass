#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;

use grass_compiler::{from_path, from_string_with_file_name, Deprecation, Options, OutputStyle};

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
    /// like any other unrecognized ID. This napi binding currently hard-errors
    /// on any unrecognized string (see `resolve_deprecation_ids`), which is a
    /// pre-existing divergence from Plan 044/#186, not something this pass
    /// (#188, version-range fatalization) changes.
    pub fatal_deprecations: Option<Vec<String>>,
    /// Deprecation IDs to opt into early, per the Sass JS API's
    /// `futureDeprecations` option.
    pub future_deprecations: Option<Vec<String>>,
}

#[napi(object)]
pub struct CompileResult {
    pub css: String,
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
/// array of string IDs, per the Sass JS API. Mirrors the CLI's behavior
/// (crates/lib/src/main.rs): an unrecognized ID is a hard error rather than a
/// silent no-op.
fn resolve_deprecation_ids(ids: &[String]) -> napi::Result<Vec<Deprecation>> {
    ids.iter()
        .map(|id| {
            Deprecation::from_id(id)
                .ok_or_else(|| napi::Error::from_reason(format!("Invalid deprecation \"{id}\".")))
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
            for deprecation in resolve_deprecation_ids(ids)? {
                options = options.silence_deprecation(deprecation);
            }
        }

        if let Some(ref ids) = opts.fatal_deprecations {
            for deprecation in resolve_deprecation_ids(ids)? {
                options = options.fatal_deprecation(deprecation);
            }
        }

        if let Some(ref ids) = opts.future_deprecations {
            for deprecation in resolve_deprecation_ids(ids)? {
                options = options.future_deprecation(deprecation);
            }
        }
    }

    Ok(options)
}

#[napi]
pub fn compile(path: String, options: Option<CompileOptions>) -> napi::Result<CompileResult> {
    catch(|| {
        let opts = build_options(options)?;

        let css =
            from_path(&path, &opts).map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult { css })
    })
}

#[napi]
pub fn compile_string(
    source: String,
    options: Option<CompileOptions>,
) -> napi::Result<CompileResult> {
    catch(|| {
        let opts = build_options(options)?;

        let cwd = std::env::current_dir().unwrap_or_default();
        let css = from_string_with_file_name(source, cwd.join("stdin"), &opts)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        Ok(CompileResult { css })
    })
}

pub struct CompileTask {
    path: String,
    options: Option<CompileOptions>,
}

impl Task for CompileTask {
    type Output = String;
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let path = &self.path;
        let opts = build_options(self.options.take())?;
        catch(std::panic::AssertUnwindSafe(|| {
            from_path(path, &opts).map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(CompileResult { css: output })
    }
}

pub struct CompileStringTask {
    source: String,
    options: Option<CompileOptions>,
}

impl Task for CompileStringTask {
    type Output = String;
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take())?;
        let cwd = std::env::current_dir().unwrap_or_default();
        let source = self.source.clone();
        catch(std::panic::AssertUnwindSafe(|| {
            from_string_with_file_name(source, cwd.join("stdin"), &opts)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        }))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(CompileResult { css: output })
    }
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_async(path: String, options: Option<CompileOptions>) -> AsyncTask<CompileTask> {
    AsyncTask::new(CompileTask { path, options })
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_string_async(
    source: String,
    options: Option<CompileOptions>,
) -> AsyncTask<CompileStringTask> {
    AsyncTask::new(CompileStringTask { source, options })
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
    fn compile_string_unknown_deprecation_id_is_err() {
        let opts = CompileOptions {
            silence_deprecations: Some(vec!["bogus-id".to_owned()]),
            ..base_opts()
        };
        let res = compile_string("a { b: c }".to_owned(), Some(opts));
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
        };
        assert!(task.compute().is_ok());
        let mut bad = CompileStringTask {
            source: "a {".to_owned(),
            options: None,
        };
        assert!(bad.compute().is_err());
    }
}
