use std::fmt;
use std::path::PathBuf;

use crate::{
    ast::ArgumentResult,
    common::Identifier,
    error::SassResult,
    evaluate::{Environment, Visitor},
};

pub(crate) type BuiltinMixin = fn(ArgumentResult, &mut Visitor) -> SassResult<()>;

pub(crate) use crate::ast::AstMixin as UserDefinedMixin;

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Mixin {
    UserDefined(UserDefinedMixin, Environment, PathBuf),
    Builtin(BuiltinMixin),
    /// A builtin mixin that accepts a `@content` block
    BuiltinWithContent(BuiltinMixin),
}

impl fmt::Debug for Mixin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserDefined(u, ..) => f
                .debug_struct("AstMixin")
                .field("name", &u.name)
                .field("args", &u.args)
                .field("body", &u.body)
                .field("has_content", &u.has_content)
                .finish(),
            Self::Builtin(..) | Self::BuiltinWithContent(..) => {
                f.debug_struct("BuiltinMixin").finish()
            }
        }
    }
}

/// A named mixin reference, analogous to `SassFunction`.
/// Returned by `meta.get-mixin()`.
#[derive(Clone, Debug)]
pub(crate) struct SassMixin {
    pub name: Identifier,
    pub mixin: Mixin,
}

impl PartialEq for SassMixin {
    fn eq(&self, other: &Self) -> bool {
        match (&self.mixin, &other.mixin) {
            (Mixin::UserDefined(a, _, _), Mixin::UserDefined(b, _, _)) => a.id == b.id,
            (
                Mixin::Builtin(a) | Mixin::BuiltinWithContent(a),
                Mixin::Builtin(b) | Mixin::BuiltinWithContent(b),
            ) => *a as usize == *b as usize,
            _ => false,
        }
    }
}

impl Eq for SassMixin {}

impl SassMixin {
    pub fn name(&self) -> Identifier {
        self.name
    }
}
