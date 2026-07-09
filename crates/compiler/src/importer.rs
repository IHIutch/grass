use std::path::PathBuf;

use codemap::Span;

use crate::{error::SassResult, options::InputSyntax};

/// The outcome of asking an [`Importer`] to resolve a load-rule URL
/// (`@use`/`@forward`/`@import`).
#[derive(Debug, Clone)]
pub enum ImportResolution {
    /// Fully resolved contents + syntax, keyed by an arbitrary canonical
    /// URL. Bypasses `Fs`/the normal path-candidate machinery entirely —
    /// the given `contents` are parsed directly under `syntax`, and the
    /// result is cached under `canonical_url` so that two different
    /// `@use` sites resolving to the same canonical URL share one parsed
    /// module.
    ///
    /// Backs a JS `Importer`'s `canonicalize`+`load` pair. Nothing in this
    /// slice constructs this variant yet (only a full `Importer`, not a
    /// `FileImporter`, can produce one) — see `find_import_uncached`'s
    /// doc comment for what wiring this up for real still needs.
    Resolved {
        canonical_url: String,
        contents: String,
        syntax: InputSyntax,
    },
    /// A path for the *existing* candidate-resolution machinery
    /// (partials, extensions, index files) to keep handling. Backs a JS
    /// `FileImporter`, which returns a `file:` URL that the compiler then
    /// applies normal partial/extension/index resolution to, exactly like
    /// a load path.
    DelegateToPath(PathBuf),
    /// This importer doesn't recognize the URL; try the next one (or fall
    /// through to the default filesystem/load-path resolution if this was
    /// the last one).
    NotFound,
}

/// A hook allowing custom resolution of `@use`/`@forward`/`@import` URLs,
/// checked in [`Options`](crate::Options) registration order ahead of the
/// default filesystem/load-path resolution.
pub trait Importer: std::fmt::Debug {
    /// Attempt to resolve `url` (the raw load-rule string, e.g.
    /// `db:foo/bar`, or a path resolved against the containing file).
    ///
    /// `from_import` is `true` when resolving an `@import` (as opposed to
    /// `@use`/`@forward`). `containing_url` is the canonical URL/path of
    /// the file the load rule appears in, or `None` at the compilation
    /// entrypoint. `span` is the call-site span of the load rule, provided
    /// so an `Err` return carries the same source-frame attribution as any
    /// other Sass compile error (e.g. a thrown JS exception surfaced
    /// through a napi-backed `Importer`).
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution>;
}
