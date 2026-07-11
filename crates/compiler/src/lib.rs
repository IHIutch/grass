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

use std::{path::Path, sync::Arc};

use parse::{CssParser, SassParser, StylesheetParser};
use sass_ast::StyleSheet;
use serializer::Serializer;
#[cfg(feature = "wasm-exports")]
use wasm_bindgen::prelude::*;

use codemap::CodeMap;

pub use crate::deprecation::Deprecation;
pub use crate::error::{
    PublicSassErrorKind as ErrorKind, SassError as Error, SassResult as Result,
};
pub use crate::fs::{DirListing, EntryKind, Fs, NullFs, StdFs};
pub use crate::importer::{ImportResolution, ImportSource, Importer};
pub use crate::logger::{Logger, NullLogger, StdLogger};
pub use crate::options::{InputSyntax, Options, OutputStyle};
pub use crate::source_map::{encode_uri, SourceMapData};
use crate::{ast::CssStmt, lexer::Lexer, parse::ScssParser};
pub use crate::{builtin::Builtin, evaluate::Visitor};
pub(crate) use crate::{context_flags::ContextFlags, lexer::Token};

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
mod deprecation;
mod error;
mod evaluate;
mod fs;
mod importer;
mod interner;
mod lexer;
mod logger;
mod options;
mod parse;
mod selector;
mod serializer;
mod source_map;
mod stack;
mod unit;
mod utils;
mod value;

fn raw_to_parse_error(map: &CodeMap, err: Error, unicode: bool) -> Box<Error> {
    let (message, span) = err.raw();
    Box::new(Error::from_loc(message, map.look_up_span(span), unicode))
}

/// ⚠ Memory note: each call permanently leaks its parse arena (the returned
/// `StyleSheet<'static>` borrows from it). Do not call this per-request in a
/// long-running process; for one-shot compilation use [`from_string`] /
/// [`from_path`], which free everything.
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
            ScssParser::new(lexer, options, empty_span, path_ref, &arena).__parse(None)
        }
        InputSyntax::Sass => {
            SassParser::new(lexer, options, empty_span, path_ref, &arena).__parse(None)
        }
        InputSyntax::Css => {
            CssParser::new(lexer, options, empty_span, path_ref, &arena).__parse(None)
        }
    };

    // Safety: We leak the arena so that the returned StyleSheet's references remain valid.
    // This is necessary because parse_stylesheet returns a StyleSheet that outlives this function.
    // The arena memory will not be freed, which is acceptable for this API.
    // INVARIANT: the erased-'static StyleSheet must not outlive the arena it was allocated in.
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
    let (css, _mappings, _sources, _sources_content, _loaded_files) =
        compile_impl(input, file_name, options)?;
    Ok(css)
}

/// Compile CSS from a string, additionally returning a [`SourceMapData`]
/// when [`Options::source_map`] is enabled.
///
/// Only top-level style declarations and selectors produce mappings (see
/// `docs/design/source-maps.md`). The second tuple element is `None`
/// whenever `options.source_map()` was not set to `true`, in which case
/// this function's CSS output is byte-identical to [`from_string`].
///
/// This has no real input path, so — matching the JS API's
/// `compileString` without a `url` option — the sole `sources` entry is a
/// `data:` URL of `input` itself, rather than a literal `"stdin"` string.
/// Any additional files pulled in via `@use`/`@import` are recorded under
/// their real (resolved) paths.
pub fn from_string_with_source_map<S: Into<String>>(
    input: S,
    options: &Options,
) -> Result<(String, Option<SourceMapData>)> {
    let input = input.into();
    // Only clone the input when a map was actually requested — this is the
    // one extra cost `from_string` callers must never pay, so it's gated on
    // the same flag that already gates all other source-map bookkeeping.
    let input_for_data_url = if options.source_map {
        Some(input.clone())
    } else {
        None
    };

    let (css, mappings, mut sources, sources_content, loaded_files) =
        compile_impl(input, "stdin", options)?;

    let map = if options.source_map {
        if let Some(idx) = sources.iter().position(|name| name == "stdin") {
            sources[idx] = crate::source_map::stdin_data_url(
                input_for_data_url.as_deref().unwrap_or_default(),
            );
        }
        Some(SourceMapData::new(
            &mappings,
            sources,
            sources_content,
            loaded_files,
        ))
    } else {
        None
    };

    Ok((css, map))
}

/// Like [`from_string_with_source_map`], but seeds the entrypoint's canonical
/// URL / relative-import base with `url` (matching the JS API's
/// `compileString({ url })`). `@use`/`@import` written relative to the entry
/// resolve against `url` — it becomes the entry's `current_import_path`, and
/// is the `containing_url` handed to custom importers for the entry's OWN
/// loads — and the source map's sole entrypoint `sources` entry is `url`
/// itself rather than the synthetic `data:` URL that
/// [`from_string_with_source_map`] uses when no URL is known.
pub fn from_string_with_url_and_source_map<S: Into<String>>(
    input: S,
    url: &str,
    options: &Options,
) -> Result<(String, Option<SourceMapData>)> {
    let (css, mappings, sources, sources_content, loaded_files) =
        compile_impl(input.into(), url, options)?;

    let map = if options.source_map {
        Some(SourceMapData::new(
            &mappings,
            sources,
            sources_content,
            loaded_files,
        ))
    } else {
        None
    };

    Ok((css, map))
}

/// Compile CSS from a path, additionally returning a [`SourceMapData`] when
/// [`Options::source_map`] is enabled. See [`from_string_with_source_map`]
/// for the general contract; unlike that function, `sources` entries here
/// are real file paths (as given, and as resolved by any `@use`/`@import`),
/// never `data:` URLs.
#[inline]
pub fn from_path_with_source_map<P: AsRef<Path>>(
    p: P,
    options: &Options,
) -> Result<(String, Option<SourceMapData>)> {
    let input = String::from_utf8(options.fs.read(p.as_ref())?)?;
    let (css, mappings, sources, sources_content, loaded_files) = compile_impl(input, p, options)?;

    let map = if options.source_map {
        Some(SourceMapData::new(
            &mappings,
            sources,
            sources_content,
            loaded_files,
        ))
    } else {
        None
    };

    Ok((css, map))
}

/// Compile CSS from a path and return the files loaded during compilation
/// without collecting source-map mappings.
///
/// This is the dependency-only path used by `--watch`. Set
/// [`Options::dependency_tracking`] to `true` before calling this function;
/// the returned list is empty otherwise. Unlike
/// [`from_path_with_source_map`], this path never enables serializer mapping
/// state unless [`Options::source_map`] is also set.
#[inline]
pub fn from_path_with_loaded_files<P: AsRef<Path>>(
    p: P,
    options: &Options,
) -> Result<(String, Vec<std::path::PathBuf>)> {
    let input = String::from_utf8(options.fs.read(p.as_ref())?)?;
    let (css, _mappings, _sources, _sources_content, loaded_files) =
        compile_impl(input, p, options)?;
    Ok((css, loaded_files))
}

#[allow(clippy::type_complexity)]
fn compile_impl<P: AsRef<Path>>(
    input: String,
    file_name: P,
    options: &Options,
) -> Result<(
    String,
    Vec<crate::source_map::RawMapping>,
    Vec<String>,
    Vec<Arc<codemap::File>>,
    Vec<std::path::PathBuf>,
)> {
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
            ScssParser::new(lexer, options, empty_span, path, &arena).__parse(None)
        }
        InputSyntax::Sass => {
            SassParser::new(lexer, options, empty_span, path, &arena).__parse(None)
        }
        InputSyntax::Css => CssParser::new(lexer, options, empty_span, path, &arena).__parse(None),
    };

    // Safety: the arena lives on the stack for the entire compilation.
    // The stylesheet references data in the arena, which won't be dropped
    // until after the visitor finishes and this function returns.
    // INVARIANT: the erased-'static StyleSheet must not outlive the arena it was allocated in.
    let stylesheet = match stylesheet {
        Ok(v) => unsafe { crate::ast::erase_stylesheet_lifetime(v) },
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    };

    let mut visitor = Visitor::new(path, options, &mut map, &arena, empty_span);
    match visitor.visit_stylesheet(&stylesheet) {
        Ok(_) => {}
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    }
    // Gathered before `finish()` (which consumes `visitor`) and gated on
    // Keep dependency tracking independent from source-map mapping state:
    // watch mode needs the complete visitor load graph, while the serializer
    // must remain on its normal no-mapping path. Independent of whether any
    // of these files contributed an emitted CSS mapping — see
    // `Visitor::loaded_files`.
    let loaded_files = if options.source_map || options.dependency_tracking {
        let mut files = visitor.loaded_files();
        let entry_path = options
            .fs
            .canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf());
        if files.binary_search(&entry_path).is_err() {
            files.push(entry_path);
            files.sort_unstable();
        }
        files
    } else {
        Vec::new()
    };
    let stmts = match visitor.finish() {
        Ok(s) => s,
        Err(e) => return Err(raw_to_parse_error(&map, *e, options.unicode_error_messages)),
    };

    let mut serializer = Serializer::with_capacity(options, &map, false, empty_span, 256 * 1024);

    let mut prev_was_group_end = false;
    let mut prev_requires_semicolon = false;
    let mut had_previous_visible = false;
    let mut stmts: std::collections::VecDeque<CssStmt> = stmts.into();

    while let Some(stmt) = stmts.pop_front() {
        if stmt.is_invisible() {
            continue;
        }

        let is_group_end = stmt.is_group_end();
        let requires_semicolon = Serializer::requires_semicolon(&stmt);
        let closing_brace_line = serializer.stmt_closing_brace_line(&stmt);

        let buf_len_before = serializer.buffer_len();

        serializer
            .visit_group(
                stmt,
                prev_was_group_end,
                prev_requires_semicolon,
                had_previous_visible,
            )
            .map_err(|e| raw_to_parse_error(&map, *e, options.unicode_error_messages))?;

        // Track whether any visible statement has been processed,
        // even if it wrote nothing (e.g. stripped sourcemap comment)
        had_previous_visible = true;

        // If the statement wrote nothing (e.g. stripped sourcemap comment),
        // don't update prev state — the next real statement should get
        // a normal separator, not group_end or semicolon from the phantom.
        if serializer.buffer_len() == buf_len_before {
            continue;
        }

        // Sub-problem C at top level: comment after closing `}` on same source line
        let mut spliced_trailing_comment = false;
        if let Some(brace_line) = closing_brace_line {
            let next_visible = stmts.iter().position(|s| !s.is_invisible());
            if let Some(idx) = next_visible {
                if let Some(comment_line) = serializer.comment_start_line(&stmts[idx]) {
                    if comment_line == brace_line {
                        if let CssStmt::Comment(ref comment, span) = stmts[idx] {
                            let comment = comment.clone();
                            serializer
                                .write_inline_comment(&comment, span, true)
                                .map_err(|e| {
                                    raw_to_parse_error(&map, *e, options.unicode_error_messages)
                                })?;
                            stmts.remove(idx);
                            spliced_trailing_comment = true;
                        }
                    }
                }
            }
        }

        // dart-sass does not insert its usual blank-line separator before the
        // next top-level statement when this one's closing `}` absorbed a
        // trailing same-line comment (verified via npx: `} /* c */\n.r {...}`
        // has no blank line, unlike a bare `}\n.r {...}`) — so don't count
        // this as a "group end" for the next iteration's separator decision.
        prev_was_group_end = is_group_end && !spliced_trailing_comment;
        prev_requires_semicolon = requires_semicolon;
    }

    let (mut mappings, sources, sources_content) = serializer.take_mappings();
    let css = serializer.finish(prev_requires_semicolon);

    // `finish` may have prepended a `@charset "UTF-8";` line (or, in
    // compressed mode, a BOM) for non-ASCII output — but that happens after
    // `take_mappings` already captured line/column positions relative to the
    // pre-prepend buffer. Shift them to match the final string dart-sass
    // itself emits a leading empty `mappings` group in this case, confirming
    // its dst positions are absolute over the final output (verified via
    // `npx sass@1.97.3` on a non-ASCII fixture; see
    // `crates/lib/tests/cli_source_map.rs`).
    if css.starts_with("@charset \"UTF-8\";\n") {
        for m in &mut mappings {
            m.dst_line += 1;
        }
    } else if css.starts_with('\u{FEFF}') {
        for m in &mut mappings {
            if m.dst_line == 0 {
                m.dst_col += 1;
            }
        }
    }

    Ok((css, mappings, sources, sources_content, loaded_files))
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
        ffi::OsString,
        io::{self, Error, ErrorKind},
        path::{Path, PathBuf},
    };

    use wasm_bindgen::prelude::*;

    use crate::{
        fs::{DirListing, EntryKind},
        Fs,
    };

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
        fn resolve_first_existing(
            this: &JsFsCallbacks,
            candidates: Vec<String>,
        ) -> Result<JsValue, JsValue>;

        /// Batches many per-candidate `is_file`/`is_dir` boundary crossings
        /// into a single directory read. Each returned entry is a string
        /// whose first byte is a kind tag (`f` file / `d` dir / anything
        /// else = unknown/symlink) followed immediately by the entry's file
        /// name. Optional: if the JS side doesn't implement this method (or
        /// it throws), the call errors and `dir_listing` falls back to
        /// per-candidate checks, exactly as before this method existed.
        #[wasm_bindgen(method, catch, js_name = readdirSync)]
        fn readdir_sync(this: &JsFsCallbacks, dir: &str) -> Result<Vec<String>, JsValue>;
    }

    pub struct JsFs {
        callbacks: JsFsCallbacks,
    }

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
            self.callbacks.read(&path.to_string_lossy()).map_err(|e| {
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
                    Error::other(
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

        fn dir_listing(&self, dir: &Path) -> Option<DirListing> {
            let entries = self.callbacks.readdir_sync(&dir.to_string_lossy()).ok()?;
            let mut listing = DirListing::default();
            for entry in entries {
                if entry.is_empty() {
                    continue;
                }
                let (kind_tag, name) = entry.split_at(1);
                let kind = match kind_tag {
                    "f" => EntryKind::File,
                    "d" => EntryKind::Dir,
                    _ => EntryKind::Other,
                };
                listing.insert(OsString::from(name), kind);
            }
            Some(listing)
        }
    }
}

/// Builds the `{css, sourceMap}` object returned to JS by `compile_js`/
/// `compile_file_js` when source maps are wired up. `sourceMap` is a real
/// parsed JS object (via `JSON.parse`, over the same JSON text the CLI and
/// napi surfaces build), not a string — matching the shape those surfaces
/// use, and never has a `file` key (that field is CLI-only).
#[cfg(feature = "wasm-exports")]
fn wasm_compile_result(css: String, map: Option<SourceMapData>, include_sources: bool) -> JsValue {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &JsValue::from_str("css"), &JsValue::from_str(&css)).unwrap();

    let source_map = match map {
        Some(map) => {
            js_sys::JSON::parse(&map.to_json(None, include_sources)).unwrap_or(JsValue::UNDEFINED)
        }
        None => JsValue::UNDEFINED,
    };
    js_sys::Reflect::set(&obj, &JsValue::from_str("sourceMap"), &source_map).unwrap();

    obj.into()
}

#[cfg(feature = "wasm-exports")]
#[wasm_bindgen(js_name = compile)]
#[allow(clippy::too_many_arguments)]
pub fn compile_js(
    input: String,
    load_paths: Vec<String>,
    style: &str,
    quiet: bool,
    source_map: bool,
    source_map_include_sources: bool,
    fs_callbacks: wasm_fs::JsFsCallbacks,
) -> std::result::Result<JsValue, String> {
    let js_fs = wasm_fs::JsFs::new(fs_callbacks);

    let mut options = Options::default()
        .fs(&js_fs)
        .quiet(quiet)
        .source_map(source_map);

    if style == "compressed" {
        options = options.style(OutputStyle::Compressed);
    }

    for lp in &load_paths {
        options = options.load_path(lp);
    }

    let (css, map) = from_string_with_source_map(input, &options).map_err(|e| e.to_string())?;
    Ok(wasm_compile_result(css, map, source_map_include_sources))
}

#[cfg(feature = "wasm-exports")]
#[wasm_bindgen(js_name = compile_file)]
#[allow(clippy::too_many_arguments)]
pub fn compile_file_js(
    path: String,
    load_paths: Vec<String>,
    style: &str,
    quiet: bool,
    source_map: bool,
    source_map_include_sources: bool,
    fs_callbacks: wasm_fs::JsFsCallbacks,
) -> std::result::Result<JsValue, String> {
    let js_fs = wasm_fs::JsFs::new(fs_callbacks);

    let mut options = Options::default()
        .fs(&js_fs)
        .quiet(quiet)
        .source_map(source_map);

    if style == "compressed" {
        options = options.style(OutputStyle::Compressed);
    }

    for lp in &load_paths {
        options = options.load_path(lp);
    }

    let (css, map) = from_path_with_source_map(&path, &options).map_err(|e| e.to_string())?;
    Ok(wasm_compile_result(css, map, source_map_include_sources))
}
