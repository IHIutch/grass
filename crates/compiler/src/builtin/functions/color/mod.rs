use crate::{builtin::builtin_imports::*, color::space::ColorSpace, value::conversion_factor};

use super::GlobalFunctionMap;

pub mod css_color4;
pub mod hsl;
pub mod hwb;
pub mod opacity;
pub mod other;
pub mod rgb;
pub mod space_fns;

#[derive(Debug, Clone)]
pub(crate) enum ParsedChannels {
    String(String),
    List(Vec<Value>),
    /// Like List, but alpha came from a slash-separated list input.
    SlashList(Vec<Value>),
}

pub(crate) fn angle_value(num: Value, name: &str, span: Span) -> SassResult<Number> {
    let angle = num.assert_number_with_name(name, span)?;

    if angle.has_compatible_units(&Unit::Deg) {
        let factor = conversion_factor(&angle.unit, &Unit::Deg).unwrap();

        return Ok(angle.num * Number(factor));
    }

    Ok(angle.num)
}

/// Parse an optional $space argument from the argument list.
pub(super) fn parse_space_arg(
    args: &mut ArgumentResult,
    pos: usize,
    span: Span,
) -> SassResult<Option<ColorSpace>> {
    match args.get(pos, "space") {
        Some(space_val) => match &space_val.node {
            Value::String(s, QuoteKind::Quoted) => Err((
                format!("$space: Expected {} to be an unquoted string.", s),
                span,
            )
                .into()),
            Value::String(s, QuoteKind::None) => {
                let space = ColorSpace::from_name(s)
                    .ok_or_else(|| (format!("$space: Unknown color space \"{}\".", s), span))?;
                Ok(Some(space))
            }
            Value::Null => Ok(None),
            v => Err((
                format!("$space: {} is not a string.", v.inspect(span)?),
                span,
            )
                .into()),
        },
        None => Ok(None),
    }
}

pub(crate) fn function_string(
    name: &'static str,
    args: &[Value],
    visitor: &mut Visitor,
    span: Span,
) -> SassResult<String> {
    let args = args
        .iter()
        .map(|arg| arg.to_css_string(span, visitor.options.is_compressed()))
        .collect::<SassResult<Vec<_>>>()?
        .join(", ");

    Ok(format!("{}({})", name, args))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    css_color4::declare(f);
    hsl::declare(f);
    hwb::declare(f);
    opacity::declare(f);
    other::declare(f);
    rgb::declare(f);
}
