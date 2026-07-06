mod functions;
pub(crate) mod modules;

pub(crate) use functions::{
    color, global_builtin_message, list, map, math, meta, selector, string,
    DISALLOWED_PLAIN_CSS_FUNCTION_NAMES, GLOBAL_FUNCTIONS,
};

pub use functions::Builtin;

/// Imports common to all builtin fns
mod builtin_imports {
    pub(crate) use super::functions::{
        color_channel_getter_message, function_percent_message, function_unit_other_than_message,
        function_units_message, global_builtin_message, suggest_scale_and_adjust, Builtin,
        GlobalFunctionMap, LegacyChannel, GLOBAL_FUNCTIONS,
    };

    pub(crate) use codemap::{Span, Spanned};

    #[cfg(feature = "random")]
    pub(crate) use rand::{distributions::Alphanumeric, thread_rng, Rng};

    pub(crate) use rustc_hash::FxHashSet;

    pub(crate) use crate::{
        ast::{Argument, ArgumentDeclaration, ArgumentResult, MaybeEvaledArguments},
        color::Color,
        common::{BinaryOp, Brackets, FxIndexMap, Identifier, ListSeparator, QuoteKind},
        deprecation::Deprecation,
        error::SassResult,
        evaluate::Visitor,
        unit::Unit,
        value::{CalculationArg, Number, SassFunction, SassMap, SassNumber, Value},
        Options,
    };

    pub(crate) use std::{cmp::Ordering, rc::Rc};
}
