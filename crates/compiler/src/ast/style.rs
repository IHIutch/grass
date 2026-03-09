use codemap::{Span, Spanned};

use crate::{interner::InternedString, value::Value};

/// A style: `color: red`
#[derive(Clone, Debug)]
pub(crate) struct Style {
    pub property: InternedString,
    pub value: Box<Spanned<Value>>,
    pub declared_as_custom_property: bool,
    /// Span of the property name, used for custom property re-indentation
    pub property_span: Span,
}
