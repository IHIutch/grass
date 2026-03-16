#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;

use grass_compiler::{
    from_path, from_path_parallel, from_paths, from_string_with_file_name, Options, OutputStyle,
};

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

#[napi]
pub fn compile_parallel(
    path: String,
    options: Option<CompileOptions>,
    num_threads: Option<u32>,
    min_frontier: Option<u32>,
) -> napi::Result<CompileResult> {
    let opts = build_options(options);
    let threads = num_threads.unwrap_or(0) as usize;
    let frontier = min_frontier.unwrap_or(4) as usize;

    let css = from_path_parallel(&path, &opts, threads, frontier)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(CompileResult { css })
}

pub struct CompileParallelTask {
    path: String,
    options: Option<CompileOptions>,
    num_threads: usize,
    min_frontier: usize,
}

impl Task for CompileParallelTask {
    type Output = String;
    type JsValue = CompileResult;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take());
        from_path_parallel(&self.path, &opts, self.num_threads, self.min_frontier)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(CompileResult { css: output })
    }
}

#[napi(ts_return_type = "Promise<CompileResult>")]
pub fn compile_parallel_async(
    path: String,
    options: Option<CompileOptions>,
    num_threads: Option<u32>,
    min_frontier: Option<u32>,
) -> AsyncTask<CompileParallelTask> {
    AsyncTask::new(CompileParallelTask {
        path,
        options,
        num_threads: num_threads.unwrap_or(0) as usize,
        min_frontier: min_frontier.unwrap_or(4) as usize,
    })
}

#[napi]
pub fn compile_many(
    paths: Vec<String>,
    options: Option<CompileOptions>,
) -> napi::Result<Vec<CompileResult>> {
    let opts = build_options(options);

    let results = from_paths(&paths, &opts);

    results
        .into_iter()
        .map(|r| {
            r.map(|css| CompileResult { css })
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
        .collect()
}

pub struct CompileManyTask {
    paths: Vec<String>,
    options: Option<CompileOptions>,
}

impl Task for CompileManyTask {
    type Output = Vec<String>;
    type JsValue = Vec<CompileResult>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let opts = build_options(self.options.take());
        let results = from_paths(&self.paths, &opts);

        results
            .into_iter()
            .map(|r| r.map_err(|e| napi::Error::from_reason(e.to_string())))
            .collect()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into_iter().map(|css| CompileResult { css }).collect())
    }
}

#[napi(ts_return_type = "Promise<CompileResult[]>")]
pub fn compile_many_async(
    paths: Vec<String>,
    options: Option<CompileOptions>,
) -> AsyncTask<CompileManyTask> {
    AsyncTask::new(CompileManyTask { paths, options })
}
