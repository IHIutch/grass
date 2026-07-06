use crate::{
    builtin::builtin_imports::*, color::space::ColorSpace, serializer::inspect_number,
    value::conversion_factor,
};

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

/// Transcribes dart-sass's `_angleValue` (`lib/src/functions/color.dart`):
/// asserts that `num` is a number and returns its value in degrees. Unitless
/// numbers are used as-is (dart's `UnitlessSassNumber.compatibleWithUnit`
/// always returns true, so they never warn); deg/grad/rad/turn are converted;
/// anything else (including complex units) is deprecated — dart still uses
/// the raw, unconverted value in that case (`return angle.value`).
pub(crate) fn angle_value(
    num: Value,
    name: &str,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Number> {
    let angle = num.assert_number_with_name(name, span)?;

    if angle.unit == Unit::None {
        return Ok(angle.num);
    }

    if let Some(factor) = conversion_factor(&angle.unit, &Unit::Deg) {
        return Ok(angle.num * Number(factor));
    }

    let unit = angle.unit.clone();
    let value_display = inspect_number(&angle, visitor.options, span)?;
    visitor.emit_deprecation(Deprecation::FunctionUnits, span, || {
        Ok(function_unit_other_than_message(
            name,
            "deg",
            &value_display,
            &unit,
        ))
    })?;

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
