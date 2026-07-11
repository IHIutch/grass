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
    /// Source span the serializer maps the value to (dart's
    /// `valueSpanForMap`): the value expression's own span, or — for a bare
    /// `$var` value — the variable's stored declaration span. Always `None`
    /// when source maps are off.
    pub value_span_for_map: Option<Span>,
}
