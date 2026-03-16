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
    let file = map.add_file(path.to_string_lossy().into_owned(), input.clone());
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

    // Auto-detect parallelism opportunity: if the entry file is mostly @forward
    // statements with enough independent modules, compile in parallel.
    // Skip if we're already inside a parallel worker (prevent recursive spawning).
    let in_worker = IN_PARALLEL_WORKER.with(|c| c.get());
    if !in_worker {
        if let Some(result) = try_parallel_compile(&stylesheet, &input, path, options) {
            return result;
        }
    }

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

/// Minimum number of independent @forward statements required to trigger
/// automatic parallel compilation.
const PARALLEL_MIN_FRONTIER: usize = 8;

/// Guard against recursive parallelism. When a worker thread is already
/// running a parallel compilation, inner calls must not spawn more threads.
thread_local! {
    static IN_PARALLEL_WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Analyze a parsed stylesheet and, if it consists primarily of @forward
/// statements with enough independent modules, compile in parallel.
///
/// Returns `None` if the stylesheet isn't suitable for parallelism,
/// allowing the caller to fall back to sequential compilation.
fn try_parallel_compile(
    stylesheet: &sass_ast::StyleSheet<'static>,
    input: &str,
    path: &Path,
    options: &Options,
) -> Option<Result<String>> {
    // Collect @forward URLs — bail if the file has non-trivial content
    let mut forward_urls: Vec<String> = Vec::new();
    let mut has_non_forward = false;

    for stmt in stylesheet.body {
        match stmt {
            crate::ast::AstStmt::Forward(rule) => {
                if has_non_forward {
                    return None; // @forward after other content — can't parallelize
                }
                forward_urls.push(rule.url.to_string_lossy().into_owned());
            }
            crate::ast::AstStmt::LoudComment(_) | crate::ast::AstStmt::SilentComment(_) => {}
            _ => {
                has_non_forward = true;
            }
        }
    }

    if forward_urls.len() < PARALLEL_MIN_FRONTIER {
        return None;
    }

    // Split into shared deps (foundation) and independent components
    let shared_count = detect_shared_prefix_count(&forward_urls);
    let component_forwards = &forward_urls[shared_count..];

    if component_forwards.len() < PARALLEL_MIN_FRONTIER {
        return None;
    }

    Some(compile_parallel_inner(
        input,
        path,
        options,
        &forward_urls[..shared_count],
        component_forwards,
    ))
}

/// Execute parallel compilation: compile shared deps once, then distribute
/// component batches across worker threads.
fn compile_parallel_inner(
    input: &str,
    path: &Path,
    options: &Options,
    shared_forwards: &[String],
    component_forwards: &[String],
) -> Result<String> {
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(4);

    // Build shared SCSS from original file text by splitting at the first component
    let first_component_url = &component_forwards[0];
    let shared_scss = if let Some(split_pos) =
        input.find(&format!("@forward \"{}\"", first_component_url))
    {
        input[..split_pos].to_string()
    } else {
        let mut s = String::new();
        for url in shared_forwards {
            s.push_str(&format!("@forward \"{}\";\n", url));
        }
        s
    };

    // Compile shared deps alone to get the shared CSS prefix
    let shared_css = if shared_forwards.is_empty() {
        String::new()
    } else {
        from_string_with_file_name(shared_scss.clone(), path, options)?
    };

    // Partition components across threads
    let chunk_size = (component_forwards.len() + num_threads - 1) / num_threads;
    let chunks: Vec<&[String]> = component_forwards.chunks(chunk_size).collect();

    // Parallel compilation: each thread compiles shared_deps + its component batch
    let results: Vec<Result<String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                let shared_scss = &shared_scss;
                scope.spawn(move || {
                    // Mark this thread as a parallel worker to prevent
                    // recursive parallelism in inner compilations.
                    IN_PARALLEL_WORKER.with(|c| c.set(true));
                    let mut wrapper = shared_scss.clone();
                    for url in *chunk {
                        wrapper.push_str(&format!("@forward \"{}\";\n", url));
                    }
                    from_string_with_file_name(wrapper, path, options)
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Worker thread panicked",
                    )
                    .into())
                })
            })
            .collect()
    });

    // Normalize shared CSS: strip @charset if present (we'll add it back if needed)
    let shared_css_stripped = shared_css
        .strip_prefix("@charset \"UTF-8\";\n")
        .unwrap_or(&shared_css);
    let shared_prefix_len = shared_css_stripped.len();

    // Merge: shared_css + stripped component CSS from each thread
    let mut final_css = shared_css_stripped.to_string();
    let mut has_non_ascii = shared_css.starts_with("@charset");

    for result in results {
        let thread_css = result?;
        let thread_stripped = thread_css
            .strip_prefix("@charset \"UTF-8\";\n")
            .unwrap_or(&thread_css);
        if thread_css.starts_with("@charset") {
            has_non_ascii = true;
        }

        if thread_stripped.len() >= shared_prefix_len
            && thread_stripped[..shared_prefix_len] == final_css[..shared_prefix_len]
        {
            final_css.push_str(&thread_stripped[shared_prefix_len..]);
        } else if shared_forwards.is_empty() {
            final_css.push_str(thread_stripped);
        } else {
            // Byte-prefix stripping failed — fall back to line-based stripping
            let shared_line_count = shared_css_stripped.lines().count();
            let thread_lines: Vec<&str> = thread_stripped.lines().collect();
            if thread_lines.len() > shared_line_count {
                let component_part = thread_lines[shared_line_count..].join("\n");
                if !component_part.is_empty() {
                    final_css.push('\n');
                    final_css.push_str(&component_part);
                    final_css.push('\n');
                }
            }
        }
    }

    if has_non_ascii {
        final_css.insert_str(0, "@charset \"UTF-8\";\n");
    }

    Ok(final_css)
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

/// Compile CSS from a path using intra-file parallel module evaluation.
///
/// Analyzes the entry file's `@forward` structure and, when there are enough
/// independent modules (≥ `min_frontier`), distributes them across worker
/// threads. Each worker independently compiles shared dependencies + its
/// assigned component batch, then the results are merged.
///
/// Falls back to sequential compilation when parallelism isn't beneficial.
///
/// `num_threads`: number of worker threads (0 = auto-detect CPU count).
/// `min_frontier`: minimum independent @forwards to trigger parallel mode (default: 4).
pub fn from_path_parallel<P: AsRef<Path>>(
    p: P,
    options: &Options,
    num_threads: usize,
    min_frontier: usize,
) -> Result<String> {
    let path = p.as_ref();
    let input = String::from_utf8(options.fs.read(path)?)?;

    // Parse entry file to analyze structure
    let arena = bumpalo::Bump::new();
    let mut map = CodeMap::new();
    let file = map.add_file(path.to_string_lossy().into_owned(), input.clone());
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let input_syntax = options
        .input_syntax
        .unwrap_or_else(|| InputSyntax::for_path(path));

    let stylesheet = match input_syntax {
        InputSyntax::Scss => ScssParser::new(lexer, options, empty_span, path, &arena).__parse(),
        InputSyntax::Sass => SassParser::new(lexer, options, empty_span, path, &arena).__parse(),
        InputSyntax::Css => CssParser::new(lexer, options, empty_span, path, &arena).__parse(),
    };
    let stylesheet = match stylesheet {
        Ok(v) => v,
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    };

    // Extract @forward URLs and loud comments from the entry stylesheet.
    let mut preamble_lines: Vec<String> = Vec::new(); // Loud comments before first forward
    let mut forward_urls: Vec<String> = Vec::new();
    let mut has_non_forward = false;
    let mut can_parallelize = true;

    for stmt in stylesheet.body {
        match stmt {
            crate::ast::AstStmt::Forward(rule) => {
                if has_non_forward {
                    can_parallelize = false;
                    break;
                }
                forward_urls.push(rule.url.to_string_lossy().into_owned());
            }
            crate::ast::AstStmt::LoudComment(_) => {
                // Comments don't affect parallelism — they're preserved
                // through the original file text used for shared SCSS
            }
            crate::ast::AstStmt::SilentComment(_) => {}
            _ => {
                has_non_forward = true;
            }
        }
    }

    // If there aren't enough forwards, or there's mixed content, fall back to sequential
    if forward_urls.len() < min_frontier || !can_parallelize {
        return from_string_with_file_name(input, path, options);
    }

    // Determine shared prefix: the first forwards that other components depend on.
    // Heuristic: check which of the first N forwards are @used by any later forward.
    // For simplicity, use a configurable split point. For USWDS, the first 5 are shared.
    //
    // Better heuristic: a forward is "shared" if any later forward's module @uses it.
    // For now, detect this by trying shared_count = 0 first (no shared deps to re-eval).
    // If that doesn't work (CSS mismatch), increase shared_count.
    //
    // For USWDS-like structures: the first few forwards are the foundation that all
    // components depend on. We detect this by checking if removing them would cause
    // compilation errors for later forwards. For simplicity, we try the "all are
    // independent" assumption first.
    //
    // Use a simple heuristic: if forwards[0] contains "core" or "global" or "elements"
    // in the path, include the initial chain of such forwards as shared.
    let shared_count = detect_shared_prefix_count(&forward_urls);
    let shared_forwards = &forward_urls[..shared_count];
    let component_forwards = &forward_urls[shared_count..];

    if component_forwards.len() < min_frontier {
        return from_string_with_file_name(input, path, options);
    }

    let num_threads = if num_threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4)
    } else {
        num_threads
    };

    // Build shared SCSS from original file text.
    // Find the line in the input that corresponds to the first component @forward
    // and split there.
    let first_component_url = &component_forwards[0];
    let shared_scss = if let Some(split_pos) = input.find(&format!("@forward \"{}\"", first_component_url)) {
        input[..split_pos].to_string()
    } else {
        // Can't find the split point — fall back to generated SCSS
        let mut s = String::new();
        for url in shared_forwards {
            s.push_str(&format!("@forward \"{}\";\n", url));
        }
        s
    };

    // Compile shared deps alone to get the shared CSS prefix
    let shared_css = if shared_forwards.is_empty() {
        String::new()
    } else {
        from_string_with_file_name(shared_scss.clone(), path, options)?
    };

    // Partition components across threads
    let chunk_size = (component_forwards.len() + num_threads - 1) / num_threads;
    let chunks: Vec<&[String]> = component_forwards.chunks(chunk_size).collect();

    let t_start = std::time::Instant::now();

    // Parallel compilation: each thread compiles shared_deps + its component batch
    let results: Vec<Result<String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                let shared_scss = &shared_scss;
                let options = options;
                let path = path;
                scope.spawn(move || {
                    // Build wrapper SCSS: shared + all components in this chunk
                    let mut wrapper = shared_scss.clone();
                    for url in *chunk {
                        wrapper.push_str(&format!("@forward \"{}\";\n", url));
                    }
                    from_string_with_file_name(wrapper, path, options)
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Worker thread panicked",
                ).into())
            }))
            .collect()
    });

    let t_parallel = std::time::Instant::now();

    // Normalize shared CSS: strip @charset if present (we'll add it back if needed)
    let shared_css_stripped = shared_css
        .strip_prefix("@charset \"UTF-8\";\n")
        .unwrap_or(&shared_css);
    let shared_prefix_len = shared_css_stripped.len();

    // Merge results: shared_css + stripped component CSS from each thread
    let mut final_css = shared_css_stripped.to_string();
    let mut has_non_ascii = shared_css.starts_with("@charset");

    for result in results {
        let thread_css = result?;
        // Strip @charset from thread output if present
        let thread_stripped = thread_css
            .strip_prefix("@charset \"UTF-8\";\n")
            .unwrap_or(&thread_css);
        if thread_css.starts_with("@charset") {
            has_non_ascii = true;
        }

        // Strip the shared CSS prefix from the thread's output
        if thread_stripped.len() >= shared_prefix_len
            && thread_stripped[..shared_prefix_len] == final_css[..shared_prefix_len]
        {
            final_css.push_str(&thread_stripped[shared_prefix_len..]);
        } else if shared_forwards.is_empty() {
            final_css.push_str(thread_stripped);
        } else {
            // Byte-prefix stripping failed — fall back to line-based stripping
            let shared_line_count = shared_css_stripped.lines().count();
            let thread_lines: Vec<&str> = thread_stripped.lines().collect();
            if thread_lines.len() > shared_line_count {
                let component_part = thread_lines[shared_line_count..].join("\n");
                if !component_part.is_empty() {
                    final_css.push('\n');
                    final_css.push_str(&component_part);
                    final_css.push('\n');
                }
            }
        }
    }

    // Add @charset back if any thread produced non-ASCII output
    if has_non_ascii {
        final_css.insert_str(0, "@charset \"UTF-8\";\n");
    }

    if std::env::var("GRASS_TIMING").is_ok() {
        eprintln!(
            "PARALLEL: {:.1}ms wall ({} threads, {} shared + {} components)",
            (t_parallel - t_start).as_secs_f64() * 1000.0,
            chunks.len(),
            shared_forwards.len(),
            component_forwards.len(),
        );
    }

    Ok(final_css)
}

/// Detect how many of the initial @forward statements form the "shared dependency base"
/// that all later components depend on. Uses path-based heuristics.
fn detect_shared_prefix_count(forward_urls: &[String]) -> usize {
    // Heuristic: shared deps have paths containing keywords like "core", "global",
    // "normalize", "elements", "fonts", "helpers", "utilities" in early positions.
    // Once we see a path that looks like a component (e.g., "usa-*"), stop.
    let shared_keywords = [
        "core", "global", "normalize", "elements", "fonts", "helpers", "reset",
        "variables", "settings", "mixins", "functions",
    ];

    let mut shared_count = 0;
    for url in forward_urls {
        let lower = url.to_lowercase();
        let is_shared = shared_keywords.iter().any(|kw| lower.contains(kw));
        if is_shared {
            shared_count += 1;
        } else {
            break; // First non-shared forward marks the boundary
        }
    }
    shared_count
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
