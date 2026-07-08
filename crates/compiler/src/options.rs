use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{builtin::Builtin, Deprecation, Fs, Logger, StdFs, StdLogger};

#[cfg(any(feature = "custom-builtin-fns", doc))]
use std::sync::Arc;

#[cfg(any(feature = "custom-builtin-fns", doc))]
use crate::{
    ast::ArgumentResult, builtin::split_signature_name, builtin::DynamicBuiltinFn,
    error::SassResult, evaluate::Visitor, value::Value,
};

/// Configuration for Sass compilation
///
/// The simplest usage is `grass::Options::default()`; however, a builder pattern
/// is also exposed to offer more control.
#[derive(Debug)]
pub struct Options<'a> {
    pub(crate) fs: &'a dyn Fs,
    pub(crate) logger: &'a dyn Logger,
    pub(crate) style: OutputStyle,
    pub(crate) load_paths: Vec<PathBuf>,
    pub(crate) allows_charset: bool,
    pub(crate) unicode_error_messages: bool,
    pub(crate) quiet: bool,
    pub(crate) input_syntax: Option<InputSyntax>,
    pub(crate) custom_fns: FxHashMap<String, Builtin>,
    pub(crate) silence_deprecations: FxHashSet<Deprecation>,
    pub(crate) fatal_deprecations: FxHashSet<Deprecation>,
    pub(crate) future_deprecations: FxHashSet<Deprecation>,
    pub(crate) source_map: bool,
}

impl Default for Options<'_> {
    #[inline]
    fn default() -> Self {
        Self {
            fs: &StdFs,
            logger: &StdLogger,
            style: OutputStyle::Expanded,
            load_paths: Vec::new(),
            allows_charset: true,
            unicode_error_messages: true,
            quiet: false,
            input_syntax: None,
            custom_fns: FxHashMap::default(),
            silence_deprecations: FxHashSet::default(),
            fatal_deprecations: FxHashSet::default(),
            future_deprecations: FxHashSet::default(),
            source_map: false,
        }
    }
}

impl<'a> Options<'a> {
    /// This option allows you to control the file system that Sass will see.
    ///
    /// By default, it uses [`StdFs`], which is backed by [`std::fs`],
    /// allowing direct, unfettered access to the local file system.
    #[must_use]
    #[inline]
    pub fn fs(mut self, fs: &'a dyn Fs) -> Self {
        self.fs = fs;
        self
    }

    /// This option allows you to define how log events should be handled
    ///
    /// Be default, [`StdLogger`] is used, which writes all events to standard output.
    #[must_use]
    #[inline]
    pub fn logger(mut self, logger: &'a dyn Logger) -> Self {
        self.logger = logger;
        self
    }

    /// `grass` currently offers 2 different output styles
    ///
    ///  - [`OutputStyle::Expanded`] writes each selector and declaration on its own line.
    ///  - [`OutputStyle::Compressed`] removes as many extra characters as possible
    ///    and writes the entire stylesheet on a single line.
    ///
    /// By default, output is expanded.
    #[must_use]
    #[inline]
    pub const fn style(mut self, style: OutputStyle) -> Self {
        self.style = style;
        self
    }

    /// This flag tells Sass not to emit any warnings when compiling. By default,
    /// Sass emits warnings when deprecated features are used or when the `@warn`
    /// rule is encountered. It also silences the `@debug` rule.
    ///
    /// Setting this option to `true` will stop all logs from reaching the [`crate::Logger`].
    ///
    /// By default, this value is `false` and warnings are emitted.
    #[must_use]
    #[inline]
    pub const fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// All Sass implementations allow users to provide load paths: paths on the
    /// filesystem that Sass will look in when locating modules. For example, if
    /// you pass `node_modules/susy/sass` as a load path, you can use
    /// `@import "susy"` to load `node_modules/susy/sass/susy.scss`.
    ///
    /// Imports will always be resolved relative to the current file first, though.
    /// Load paths will only be used if no relative file exists that matches the
    /// module's URL. This ensures that you can't accidentally mess up your relative
    /// imports when you add a new library.
    ///
    /// This method will append a single path to the list.
    #[must_use]
    #[inline]
    pub fn load_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.load_paths.push(path.as_ref().to_owned());
        self
    }

    /// Append multiple loads paths
    ///
    /// Note that this method does *not* remove existing load paths
    ///
    /// See [`Options::load_path`](Options::load_path) for more information about
    /// load paths
    #[must_use]
    #[inline]
    pub fn load_paths<P: AsRef<Path>>(mut self, paths: &[P]) -> Self {
        for path in paths {
            self.load_paths.push(path.as_ref().to_owned());
        }

        self
    }

    /// This flag tells Sass whether to emit a `@charset`
    /// declaration or a UTF-8 byte-order mark.
    ///
    /// By default, Sass will insert either a `@charset`
    /// declaration (in expanded output mode) or a byte-order
    /// mark (in compressed output mode) if the stylesheet
    /// contains any non-ASCII characters.
    #[must_use]
    #[inline]
    pub const fn allows_charset(mut self, allows_charset: bool) -> Self {
        self.allows_charset = allows_charset;
        self
    }

    /// This flag tells Sass only to emit ASCII characters as
    /// part of error messages.
    ///
    /// By default Sass will emit non-ASCII characters for
    /// these messages.
    ///
    /// This flag does not affect the CSS output.
    #[must_use]
    #[inline]
    pub const fn unicode_error_messages(mut self, unicode_error_messages: bool) -> Self {
        self.unicode_error_messages = unicode_error_messages;
        self
    }

    /// This option forces Sass to parse input using the given syntax.
    ///
    /// By default, Sass will attempt to read the file extension to determine
    /// the syntax. If this is not possible, it will default to [`InputSyntax::Scss`].
    ///
    /// This flag only affects the first file loaded. Files that are loaded using
    /// `@import`, `@use`, or `@forward` will always have their syntax inferred.
    #[must_use]
    #[inline]
    pub const fn input_syntax(mut self, syntax: InputSyntax) -> Self {
        self.input_syntax = Some(syntax);
        self
    }

    /// Add a custom function accessible from within Sass
    ///
    /// See the [`Builtin`] documentation for additional information
    #[must_use]
    #[inline]
    #[cfg(any(feature = "custom-builtin-fns", doc))]
    pub fn add_custom_fn<S: Into<String>>(mut self, name: S, func: Builtin) -> Self {
        self.custom_fns.insert(name.into(), func);
        self
    }

    /// Add a custom function accessible from within Sass, whose call
    /// arguments are pre-bound to the parameters declared in `signature`
    /// (positional/named/defaults/`$rest...`) before `f` is invoked — the
    /// same calling convention as an `@function`, rather than the raw
    /// unbound [`ArgumentResult`](crate::sass_value::ArgumentResult) that
    /// [`Builtin::new`]/[`Options::add_custom_fn`] receive.
    ///
    /// `signature` is a full function signature string, e.g.
    /// `"sum($a, $b: 1)"` or `"scale($a, $b: $a)"` (defaults may reference
    /// earlier parameters). The name portion is normalized the same way as
    /// `@function` names (`_` becomes `-`). Returns an error if `signature`
    /// isn't of the form `"name(...)"`; the `(...)` portion itself is
    /// parsed lazily, the first time the function is called in a given
    /// compilation.
    #[inline]
    #[cfg(any(feature = "custom-builtin-fns", doc))]
    pub fn add_custom_fn_with_signature<S, F>(mut self, signature: S, f: F) -> SassResult<Self>
    where
        S: AsRef<str>,
        F: Fn(ArgumentResult, &mut Visitor) -> SassResult<Value> + Send + Sync + 'static,
    {
        let (name, arg_text) = split_signature_name(signature.as_ref())?;
        let f: Arc<DynamicBuiltinFn> = Arc::new(f);
        self.custom_fns
            .insert(name, Builtin::new_dynamic(f, Some(Arc::from(arg_text))));
        Ok(self)
    }

    /// Silence warnings for the given deprecation.
    ///
    /// By default, all active (non-future) deprecations emit a warning when
    /// triggered. This method adds a single deprecation to the set that is
    /// silenced instead.
    #[must_use]
    #[inline]
    pub fn silence_deprecation(mut self, deprecation: Deprecation) -> Self {
        self.silence_deprecations.insert(deprecation);
        self
    }

    /// Treat the given deprecation as a fatal error instead of a warning.
    ///
    /// This method adds a single deprecation to the set that, when
    /// triggered, causes compilation to fail with an error instead of
    /// emitting a warning.
    #[must_use]
    #[inline]
    pub fn fatal_deprecation(mut self, deprecation: Deprecation) -> Self {
        self.fatal_deprecations.insert(deprecation);
        self
    }

    /// Opt in to a deprecation that is not yet active by default.
    ///
    /// Some deprecations (dart-sass's "future" deprecations) are disabled by
    /// default until a later release. This method adds a single deprecation
    /// to the set that is opted into early.
    #[must_use]
    #[inline]
    pub fn future_deprecation(mut self, deprecation: Deprecation) -> Self {
        self.future_deprecations.insert(deprecation);
        self
    }

    /// Enable collection of a Source Map v3 mapping alongside the compiled CSS.
    ///
    /// This is a design-spike prototype: only top-level style declarations and
    /// selectors are mapped, and only [`crate::from_string_with_source_map`]
    /// consumes this flag. It has no effect on [`crate::from_string`] or
    /// [`crate::from_path`], and it never changes CSS output.
    ///
    /// By default, this value is `false`.
    #[must_use]
    #[inline]
    pub const fn source_map(mut self, source_map: bool) -> Self {
        self.source_map = source_map;
        self
    }

    pub(crate) fn is_compressed(&self) -> bool {
        matches!(self.style, OutputStyle::Compressed)
    }
}

/// Useful when parsing Sass from sources other than the file system
///
/// See [`Options::input_syntax`] for additional information
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputSyntax {
    /// The CSS-superset SCSS syntax.
    Scss,

    /// The whitespace-sensitive indented syntax.
    Sass,

    /// The plain CSS syntax, which disallows special Sass features.
    Css,
}

impl InputSyntax {
    pub(crate) fn for_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("css") => Self::Css,
            Some("sass") => Self::Sass,
            _ => Self::Scss,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputStyle {
    /// This mode writes each selector and declaration on its own line.
    ///
    /// This is the default output.
    Expanded,

    /// Ideal for release builds, this mode removes as many extra characters as
    /// possible and writes the entire stylesheet on a single line.
    Compressed,
}
