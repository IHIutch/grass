#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
