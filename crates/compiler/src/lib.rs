/*!
This crate provides functionality for compiling [Sass](https://sass-lang.com/) to CSS.

This crate targets compatibility with the reference implementation in Dart. If
upgrading from the [now deprecated](https://sass-lang.com/blog/libsass-is-deprecated)
`libsass`, one may have to modify their stylesheets. These changes will not differ
from those necessary to upgrade to `dart-sass`, and in general such changes should
be quite rare.

This crate is capable of compiling Bootstrap 4 and 5, bulma and bulma-scss, Bourbon,
as well as most other large Sass libraries with complete accuracy. For the vast
majority of use cases there should be no perceptible differences from the reference
implementation.

## Use as library
```
# use grass_compiler as grass;
fn main() -> Result<(), Box<grass::Error>> {
    let css = grass::from_string(
        "a { b { color: &; } }".to_owned(),
        &grass::Options::default().style(grass::OutputStyle::Compressed)
    )?;
    assert_eq!(css, "a b{color:a b}");
    Ok(())
}
```

## Use as binary
```bash
cargo install grass
grass input.scss
```
*/

#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![warn(clippy::all, clippy::cargo, clippy::dbg_macro)]
#![deny(missing_debug_implementations)]
#![allow(
    clippy::use_self,
    // filter isn't fallible
    clippy::manual_filter_map,
    renamed_and_removed_lints,
    clippy::unknown_clippy_lints,
    clippy::single_match,
    clippy::new_without_default,
    clippy::single_match_else,
    clippy::multiple_crate_versions,
    clippy::wrong_self_convention,
    clippy::comparison_chain,
    clippy::unwrap_or_default,
    clippy::manual_unwrap_or_default,

    // todo: these should be enabled
    clippy::arc_with_non_send_sync,

    // todo: unignore once we bump MSRV
    clippy::assigning_clones,

    unknown_lints,
)]

use std::path::Path;

use parse::{CssParser, SassParser, StylesheetParser};

std::thread_local! {
    /// Cumulative time spent parsing imported modules during evaluation.
    /// Only used when GRASS_TIMING is set.
    pub(crate) static IMPORT_PARSE_TIME: std::cell::Cell<std::time::Duration> =
        const { std::cell::Cell::new(std::time::Duration::ZERO) };
}
use sass_ast::StyleSheet;
use serializer::Serializer;
#[cfg(feature = "wasm-exports")]
use wasm_bindgen::prelude::*;

use codemap::CodeMap;

pub use crate::error::{
    PublicSassErrorKind as ErrorKind, SassError as Error, SassResult as Result,
};
pub use crate::fs::{Fs, NullFs, StdFs};
pub use crate::logger::{Logger, NullLogger, StdLogger};
pub use crate::options::{InputSyntax, Options, OutputStyle};
pub use crate::{builtin::Builtin, evaluate::Visitor};
pub(crate) use crate::{context_flags::ContextFlags, lexer::Token};
use crate::{lexer::Lexer, parse::ScssParser};

pub mod sass_value {
    pub use crate::{
        ast::ArgumentResult,
        color::Color,
        common::{BinaryOp, Brackets, ListSeparator, QuoteKind},
        unit::{ComplexUnit, Unit},
        value::{
            ArgList, CalculationArg, CalculationName, Number, SassCalculation, SassFunction,
            SassMap, SassNumber, Value,
        },
    };
}

pub mod sass_ast {
    pub use crate::ast::*;
}

pub use codemap;

mod ast;
mod builtin;
mod color;
mod common;
mod context_flags;
mod error;
mod evaluate;
mod fs;
mod interner;
mod lexer;
mod logger;
mod options;
mod parse;
mod selector;
mod serializer;
mod unit;
mod utils;
mod value;

fn raw_to_parse_error(map: &CodeMap, err: Error, unicode: bool) -> Box<Error> {
    let (message, span) = err.raw();
    Box::new(Error::from_loc(message, map.look_up_span(span), unicode))
}

pub fn parse_stylesheet<P: AsRef<Path>>(
    input: String,
    file_name: P,
    options: &Options,
) -> Result<StyleSheet<'static>> {
    // todo: much of this logic is duplicated in `from_string_with_file_name`
    let arena = bumpalo::Bump::new();
    let mut map = CodeMap::new();
    let path = file_name.as_ref();
    let file = map.add_file(path.to_string_lossy().into_owned(), input);
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let input_syntax = options
        .input_syntax
        .unwrap_or_else(|| InputSyntax::for_path(path));

    let path_ref = file_name.as_ref();
    let stylesheet = match input_syntax {
        InputSyntax::Scss => {
            ScssParser::new(lexer, options, empty_span, path_ref, &arena).__parse()
        }
        InputSyntax::Sass => {
            SassParser::new(lexer, options, empty_span, path_ref, &arena).__parse()
        }
        InputSyntax::Css => {
            CssParser::new(lexer, options, empty_span, path_ref, &arena).__parse()
        }
    };

    // Safety: We leak the arena so that the returned StyleSheet's references remain valid.
    // This is necessary because parse_stylesheet returns a StyleSheet that outlives this function.
    // The arena memory will not be freed, which is acceptable for this API.
    let stylesheet = match stylesheet {
        Ok(v) => unsafe { crate::ast::erase_stylesheet_lifetime(v) },
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    };

    // Leak the arena so the StyleSheet's references remain valid
    std::mem::forget(arena);

    Ok(stylesheet)
}

pub fn from_string_with_file_name<P: AsRef<Path>>(
    input: String,
    file_name: P,
    options: &Options,
) -> Result<String> {
    let timing = std::env::var("GRASS_TIMING").is_ok();
    let t_start = std::time::Instant::now();

    let arena = bumpalo::Bump::new();
    let mut map = CodeMap::new();
    let path = file_name.as_ref();
    let file = map.add_file(path.to_string_lossy().into_owned(), input);
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let input_syntax = options
        .input_syntax
        .unwrap_or_else(|| InputSyntax::for_path(path));

    let stylesheet = match input_syntax {
        InputSyntax::Scss => {
            ScssParser::new(lexer, options, empty_span, path, &arena).__parse()
        }
        InputSyntax::Sass => {
            SassParser::new(lexer, options, empty_span, path, &arena).__parse()
        }
        InputSyntax::Css => {
            CssParser::new(lexer, options, empty_span, path, &arena).__parse()
        }
    };

    let t_parse = std::time::Instant::now();

    // Safety: the arena lives on the stack for the entire compilation.
    // The stylesheet references data in the arena, which won't be dropped
    // until after the visitor finishes and this function returns.
    let stylesheet = match stylesheet {
        Ok(v) => unsafe { crate::ast::erase_stylesheet_lifetime(v) },
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    };

    let mut visitor = Visitor::new(path, options, &mut map, &arena, empty_span);

    // Pre-parse all @use/@forward dependencies before evaluation.
    // This front-loads CodeMap mutations so the eval phase doesn't need to parse.
    match visitor.pre_parse_dependencies(&stylesheet) {
        Ok(_) => {}
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    }

    match visitor.visit_stylesheet(&stylesheet) {
        Ok(_) => {}
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    }
    let (css_tree, combined_imports, import_tree_count, has_ooo) =
        match visitor.finish_for_tree_walk() {
            Ok(v) => v,
            Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
        };

    let t_eval = std::time::Instant::now();

    let mut serializer = Serializer::with_capacity(options, &map, false, empty_span, 256 * 1024);

    let prev_requires_semicolon = serializer
        .serialize_tree(&css_tree, &combined_imports, import_tree_count, has_ooo)
        .map_err(|e| raw_to_parse_error(&map, *e, options.unicode_error_messages))?;

    let result = serializer.finish(prev_requires_semicolon);

    if timing {
        let t_serial = std::time::Instant::now();
        let total = t_serial - t_start;
        let entry_parse = t_parse - t_start;
        let eval_phase = t_eval - t_parse;
        let serial = t_serial - t_eval;

        let import_parse = IMPORT_PARSE_TIME.with(|t| {
            let v = t.get();
            t.set(std::time::Duration::ZERO);
            v
        });

        let total_parse = entry_parse + import_parse;
        let pure_eval = eval_phase - import_parse;

        eprintln!(
            "TIMING: total={:.1}ms  parse={:.1}ms ({:.0}%) [entry={:.1}ms + imports={:.1}ms]  eval={:.1}ms ({:.0}%)  serialize={:.1}ms ({:.0}%)",
            total.as_secs_f64() * 1000.0,
            total_parse.as_secs_f64() * 1000.0,
            total_parse.as_secs_f64() / total.as_secs_f64() * 100.0,
            entry_parse.as_secs_f64() * 1000.0,
            import_parse.as_secs_f64() * 1000.0,
            pure_eval.as_secs_f64() * 1000.0,
            pure_eval.as_secs_f64() / total.as_secs_f64() * 100.0,
            serial.as_secs_f64() * 1000.0,
            serial.as_secs_f64() / total.as_secs_f64() * 100.0,
        );
    }

    Ok(result)
}

/// Compile multiple Sass files in parallel.
///
/// Returns results in the same order as the input paths.
/// Each compilation is independent with its own arena and scope.
///
/// Requires the `parallel` feature.
///
/// ```
/// # use grass_compiler as grass;
/// # #[cfg(feature = "parallel")]
/// fn main() -> Result<(), Box<grass::Error>> {
///     let results = grass::from_paths(&["a.scss", "b.scss"], &grass::Options::default());
///     Ok(())
/// }
/// ```
#[cfg(feature = "parallel")]
pub fn from_paths<P: AsRef<Path> + Sync>(
    paths: &[P],
    options: &Options,
) -> Vec<Result<String>> {
    use rayon::prelude::*;
    paths.par_iter().map(|p| from_path(p, options)).collect()
}

/// Compile CSS from a path
///
/// n.b. `grass` does not currently support files or paths that are not valid UTF-8
///
/// ```
/// # use grass_compiler as grass;
/// fn main() -> Result<(), Box<grass::Error>> {
///     let css = grass::from_path("input.scss", &grass::Options::default())?;
///     Ok(())
/// }
/// ```
#[inline]
pub fn from_path<P: AsRef<Path>>(p: P, options: &Options) -> Result<String> {
    from_string_with_file_name(String::from_utf8(options.fs.read(p.as_ref())?)?, p, options)
}

/// Compile CSS from a string
///
/// ```
/// # use grass_compiler as grass;
/// fn main() -> Result<(), Box<grass::Error>> {
///     let css = grass::from_string("a { b { color: &; } }".to_string(), &grass::Options::default())?;
///     assert_eq!(css, "a b {\n  color: a b;\n}\n");
///     Ok(())
/// }
/// ```
#[inline]
pub fn from_string<S: Into<String>>(input: S, options: &Options) -> Result<String> {
    from_string_with_file_name(input.into(), "stdin", options)
}

#[cfg(feature = "wasm-exports")]
#[wasm_bindgen(js_name = from_string)]
pub fn from_string_js(input: String) -> std::result::Result<String, String> {
    from_string(input, &Options::default()).map_err(|e| e.to_string())
}

#[cfg(feature = "wasm-exports")]
mod wasm_fs {
    use std::{
        io::{self, Error, ErrorKind},
        path::{Path, PathBuf},
    };

    use wasm_bindgen::prelude::*;

    use crate::Fs;

    #[wasm_bindgen]
    extern "C" {
        pub type JsFsCallbacks;

        #[wasm_bindgen(method, catch)]
        fn is_file(this: &JsFsCallbacks, path: &str) -> Result<bool, JsValue>;

        #[wasm_bindgen(method, catch)]
        fn is_dir(this: &JsFsCallbacks, path: &str) -> Result<bool, JsValue>;

        #[wasm_bindgen(method, catch)]
        fn read(this: &JsFsCallbacks, path: &str) -> Result<Vec<u8>, JsValue>;

        #[wasm_bindgen(method, catch)]
        fn canonicalize(this: &JsFsCallbacks, path: &str) -> Result<String, JsValue>;

        #[wasm_bindgen(method, catch)]
        fn resolve_first_existing(this: &JsFsCallbacks, candidates: Vec<String>) -> Result<JsValue, JsValue>;
    }

    pub struct JsFs {
        callbacks: JsFsCallbacks,
    }

    // Safety: WASM is single-threaded, so Send+Sync are trivially safe.
    // These are required because the Fs trait has Send+Sync supertraits.
    unsafe impl Send for JsFs {}
    unsafe impl Sync for JsFs {}

    impl std::fmt::Debug for JsFs {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("JsFs").finish()
        }
    }

    impl JsFs {
        pub fn new(callbacks: JsFsCallbacks) -> Self {
            Self { callbacks }
        }
    }

    impl Fs for JsFs {
        fn is_file(&self, path: &Path) -> bool {
            self.callbacks
                .is_file(&path.to_string_lossy())
                .unwrap_or(false)
        }

        fn is_dir(&self, path: &Path) -> bool {
            self.callbacks
                .is_dir(&path.to_string_lossy())
                .unwrap_or(false)
        }

        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.callbacks
                .read(&path.to_string_lossy())
                .map_err(|e| {
                    Error::new(
                        ErrorKind::NotFound,
                        e.as_string().unwrap_or_else(|| "read error".to_string()),
                    )
                })
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.callbacks
                .canonicalize(&path.to_string_lossy())
                .map(PathBuf::from)
                .map_err(|e| {
                    Error::new(
                        ErrorKind::Other,
                        e.as_string()
                            .unwrap_or_else(|| "canonicalize error".to_string()),
                    )
                })
        }

        fn resolve_first_existing(&self, candidates: &[PathBuf]) -> Option<PathBuf> {
            let str_candidates: Vec<String> = candidates
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            match self.callbacks.resolve_first_existing(str_candidates) {
                Ok(val) => {
                    if val.is_null() || val.is_undefined() {
                        None
                    } else {
                        val.as_string().map(PathBuf::from)
                    }
                }
                // Fallback: JS side doesn't implement this method
                Err(_) => candidates.iter().find(|p| self.is_file(p)).cloned(),
            }
        }
    }
}

#[cfg(feature = "wasm-exports")]
#[wasm_bindgen(js_name = compile)]
pub fn compile_js(
    input: String,
    load_paths: Vec<String>,
    style: &str,
    quiet: bool,
    fs_callbacks: wasm_fs::JsFsCallbacks,
) -> std::result::Result<String, String> {
    let js_fs = wasm_fs::JsFs::new(fs_callbacks);

    let mut options = Options::default().fs(&js_fs).quiet(quiet);

    if style == "compressed" {
        options = options.style(OutputStyle::Compressed);
    }

    for lp in &load_paths {
        options = options.load_path(lp);
    }

    from_string(input, &options).map_err(|e| e.to_string())
}

#[cfg(feature = "wasm-exports")]
#[wasm_bindgen(js_name = compile_file)]
pub fn compile_file_js(
    path: String,
    load_paths: Vec<String>,
    style: &str,
    quiet: bool,
    fs_callbacks: wasm_fs::JsFsCallbacks,
) -> std::result::Result<String, String> {
    let js_fs = wasm_fs::JsFs::new(fs_callbacks);

    let mut options = Options::default().fs(&js_fs).quiet(quiet);

    if style == "compressed" {
        options = options.style(OutputStyle::Compressed);
    }

    for lp in &load_paths {
        options = options.load_path(lp);
    }

    from_path(&path, &options).map_err(|e| e.to_string())
}
