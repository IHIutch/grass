#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;

use grass_compiler::{from_path, from_string_with_file_name, Options, OutputStyle};

#[napi(object)]
pub struct CompileOptions {
    pub style: Option<String>,
    pub load_paths: Option<Vec<String>>,
    pub quiet: Option<bool>,
    pub charset: Option<bool>,
}

#[napi(object)]
pub struct CompileResult {
    pub css: String,
}

fn build_options(opts: Option<CompileOptions>) -> Options<'static> {
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
    }

    options
}

#[napi]
pub fn compile(path: String, options: Option<CompileOptions>) -> napi::Result<CompileResult> {
    let opts = build_options(options);

    let css = from_path(&path, &opts).map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(CompileResult { css })
}

#[napi]
pub fn compile_string(
    source: String,
    options: Option<CompileOptions>,
) -> napi::Result<CompileResult> {
    let opts = build_options(options);

    let cwd = std::env::current_dir().unwrap_or_default();
    let css = from_string_with_file_name(source, cwd.join("stdin"), &opts)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(CompileResult { css })
}

pub struct CompileTask {
    path: String,
    options: Option<CompileOptions>,
}

impl Task for CompileTask {
    type Output = String;
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take());
        from_path(&self.path, &opts).map_err(|e| napi::Error::from_reason(e.to_string()))
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
        let opts = build_options(self.options.take());
        let cwd = std::env::current_dir().unwrap_or_default();
        from_string_with_file_name(self.source.clone(), cwd.join("stdin"), &opts)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
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
