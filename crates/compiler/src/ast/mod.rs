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
#[allow(dead_code)]
pub(crate) unsafe fn erase_fn_decl_lifetime<'a>(
    decl: AstFunctionDecl<'a>,
) -> AstFunctionDecl<'static> {
    std::mem::transmute(decl)
}

/// See `erase_fn_decl_lifetime` for safety justification.
#[allow(dead_code)]
pub(crate) unsafe fn erase_mixin_lifetime<'a>(mixin: AstMixin<'a>) -> AstMixin<'static> {
    std::mem::transmute(mixin)
}

/// See `erase_fn_decl_lifetime` for safety justification.
#[allow(dead_code)]
pub(crate) unsafe fn erase_content_block_lifetime<'a>(
    block: AstContentBlock<'a>,
) -> AstContentBlock<'static> {
    std::mem::transmute(block)
}

/// See `erase_fn_decl_lifetime` for safety justification.
#[allow(dead_code)]
pub(crate) unsafe fn erase_forward_rule_lifetime<'a>(
    rule: AstForwardRule<'a>,
) -> AstForwardRule<'static> {
    std::mem::transmute(rule)
}

/// See `erase_fn_decl_lifetime` for safety justification.
pub(crate) unsafe fn erase_stylesheet_lifetime<'a>(sheet: StyleSheet<'a>) -> StyleSheet<'static> {
    std::mem::transmute(sheet)
}

/// See `erase_fn_decl_lifetime` for safety justification.
#[allow(dead_code)]
pub(crate) unsafe fn erase_stmt_lifetime<'a>(stmt: AstStmt<'a>) -> AstStmt<'static> {
    std::mem::transmute(stmt)
}

/// See `erase_fn_decl_lifetime` for safety justification.
#[allow(dead_code)]
pub(crate) unsafe fn erase_configured_variable_lifetime<'a>(
    cv: ConfiguredVariable<'a>,
) -> ConfiguredVariable<'static> {
    std::mem::transmute(cv)
}
