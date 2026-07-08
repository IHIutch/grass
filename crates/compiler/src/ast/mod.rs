pub use args::*;
pub(crate) use css::*;
pub use expr::*;
pub use interpolation::*;
pub(crate) use media::*;
pub(crate) use mixin::*;
pub use stmt::*;
pub(crate) use style::*;
pub(crate) use unknown::*;

pub use args::ArgumentResult;

mod args;
mod css;
mod expr;
mod interpolation;
mod media;
mod mixin;
mod stmt;
mod style;
mod unknown;

/// Safety: This is safe because the arena outlives the entire compilation.
/// All AST references point into the arena, which is not deallocated until
/// after the visitor finishes. The `'static` lifetime is used as an erasure
/// mechanism so that runtime types (Value, SassFunction, Mixin, Scopes,
/// Environment, Module) don't need lifetime parameters.
///
/// The arena is created at the entry point (lib.rs) and lives until the
/// compilation result is returned to the caller.
/// See the module-level safety doc above for justification.
///
/// INVARIANT: the returned `'static`-erased value must not outlive the
/// arena backing the compilation that produced it (see call sites in
/// `lib.rs` and `evaluate/visitor.rs`).
pub(crate) unsafe fn erase_stylesheet_lifetime<'a>(
    sheet: StyleSheet<'a>,
) -> StyleSheet<'static> {
    std::mem::transmute(sheet)
}

/// Safety: mirrors [`erase_stylesheet_lifetime`] — this is only used to
/// erase the lifetime of an `ArgumentDeclaration` parsed against a
/// `Visitor`'s own arena (see `Visitor::parse_dynamic_signature`), which
/// lives for the entire compilation. The erased value is cached on the
/// `Visitor` itself (never on `Builtin`/`Options`, which can outlive any
/// single compilation), so it cannot outlive the arena it borrows from.
pub(crate) unsafe fn erase_argument_declaration_lifetime<'a>(
    decl: ArgumentDeclaration<'a>,
) -> ArgumentDeclaration<'static> {
    std::mem::transmute(decl)
}
