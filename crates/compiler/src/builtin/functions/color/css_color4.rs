use crate::builtin::builtin_imports::*;
use crate::color::space::ColorSpace;

use super::rgb::{function_string, parse_channels, percentage_or_unitless};
use super::{GlobalFunctionMap, ParsedChannels};

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("lab", Builtin::new(lab));
    f.insert("lch", Builtin::new(lch));
    f.insert("oklab", Builtin::new(oklab));
    f.insert("oklch", Builtin::new(oklch));
    f.insert("color", Builtin::new(color_fn));
}

/// Parse a channel value that might be `none`, a number, or a percentage.
/// Returns Some(f64) for a normal value, None for `none`.
fn parse_channel_value(
    val: &Value,
    name: &str,
    max: f64,
    percentage_ref: Option<f64>,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Option<f64>> {
    match val {
        Value::String(s, QuoteKind::None) if s == "none" => Ok(None),
        _ => {
            let num = val.clone().assert_number_with_name(name, span)?;

            let value = if num.unit == Unit::Percent {
                if let Some(pref) = percentage_ref {
                    (num.num.0 / 100.0) * pref
                } else {
                    // For hue channels, percentage doesn't apply
                    return Err((
                        format!(
                            "${}: Expected {} to have unit \"deg\" or no units.",
                            name, num.num.inspect()
                        ),
                        span,
                    )
                        .into());
                }
            } else if num.unit == Unit::None || num.unit == Unit::Deg {
                num.num.0
            } else {
                return Err((
                    format!(
                        "${}: Expected {} to have no units or \"%\".",
                        name, num.num.inspect()
                    ),
                    span,
                )
                    .into());
            };

            Ok(Some(value))
        }
    }
}

/// Parse alpha that might be `none`, a number, or a percentage.
fn parse_alpha_value(
    val: &Value,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Option<f64>> {
    match val {
        Value::String(s, QuoteKind::None) if s == "none" => Ok(None),
        _ => {
            let alpha = percentage_or_unitless(
                &val.clone().assert_number_with_name("alpha", span)?,
                1.0,
                "alpha",
                span,
                visitor,
            )?;
            Ok(Some(alpha))
        }
    }
}

/// Construct a Color from parsed channels for a known color space.
fn construct_color(
    space: ColorSpace,
    channels: &[Value],
    has_alpha: bool,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    let channel_defs = space.channels();

    let c0 = parse_channel_value(
        &channels[0],
        channel_defs[0].name,
        channel_defs[0].max,
        channel_defs[0].percentage_ref,
        span,
        visitor,
    )?;
    let c1 = parse_channel_value(
        &channels[1],
        channel_defs[1].name,
        channel_defs[1].max,
        channel_defs[1].percentage_ref,
        span,
        visitor,
    )?;
    let c2 = parse_channel_value(
        &channels[2],
        channel_defs[2].name,
        channel_defs[2].max,
        channel_defs[2].percentage_ref,
        span,
        visitor,
    )?;

    let alpha = if has_alpha && channels.len() > 3 {
        parse_alpha_value(&channels[3], span, visitor)?
    } else {
        Some(1.0)
    };

    use crate::color::{Color, ColorFormat};

    Ok(Value::Color(Arc::new(Color::for_space(
        space, [c0, c1, c2], alpha,
        ColorFormat::Infer,
    ))))
}

/// `lab($channels)` - construct a CIE Lab color
pub(crate) fn lab(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    match parse_channels(
        "lab",
        &["lightness", "a", "b"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )? {
        ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
        ParsedChannels::List(list) => {
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $a.", span).into());
            }
            construct_color(ColorSpace::Lab, &list, has_alpha, span, visitor)
        }
    }
}

/// `lch($channels)` - construct a CIE LCH color
pub(crate) fn lch(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    match parse_channels(
        "lch",
        &["lightness", "chroma", "hue"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )? {
        ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
        ParsedChannels::List(list) => {
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $chroma.", span).into());
            }
            construct_color(ColorSpace::Lch, &list, has_alpha, span, visitor)
        }
    }
}

/// `oklab($channels)` - construct an OKLab color
pub(crate) fn oklab(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    match parse_channels(
        "oklab",
        &["lightness", "a", "b"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )? {
        ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
        ParsedChannels::List(list) => {
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $a.", span).into());
            }
            construct_color(ColorSpace::Oklab, &list, has_alpha, span, visitor)
        }
    }
}

/// `oklch($channels)` - construct an OKLch color
pub(crate) fn oklch(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    match parse_channels(
        "oklch",
        &["lightness", "chroma", "hue"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )? {
        ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
        ParsedChannels::List(list) => {
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $chroma.", span).into());
            }
            construct_color(ColorSpace::Oklch, &list, has_alpha, span, visitor)
        }
    }
}

/// `color($description)` - construct a color in a named space.
/// Syntax: `color(display-p3 0.5 0.3 0.1)` or `color(display-p3 0.5 0.3 0.1 / 0.8)`
pub(crate) fn color_fn(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    let description = args.get_err(0, "description")?;

    if description.is_var() {
        let fn_string = function_string("color", &[description], visitor, span)?;
        return Ok(Value::String(fn_string, QuoteKind::None));
    }

    // The description is a space-separated list potentially with slash for alpha
    let mut items = description.clone().as_list();

    // Handle slash-separated alpha
    let desc_clone = description.clone();
    let mut alpha_val = None;
    if desc_clone.separator() == ListSeparator::Slash {
        let slash_parts = desc_clone.as_list();
        if slash_parts.len() != 2 {
            return Err((
                format!(
                    "Only 2 slash-separated elements allowed, but {} were passed.",
                    slash_parts.len()
                ),
                span,
            )
                .into());
        }
        items = slash_parts[0].clone().as_list();
        alpha_val = Some(slash_parts[1].clone());
    }

    if items.is_empty() {
        return Err(("Missing color space name.", span).into());
    }

    // First item is the color space name
    let space_name = match &items[0] {
        Value::String(s, QuoteKind::None) => s.clone(),
        v if v.is_special_function() || v.is_var() => {
            let fn_string = function_string("color", &[description], visitor, span)?;
            return Ok(Value::String(fn_string, QuoteKind::None));
        }
        v => {
            return Err((
                format!(
                    "$description: Expected color space name, got {}.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let space = match ColorSpace::from_name(&space_name) {
        Some(s) => s,
        None => {
            return Err((
                format!("Unknown color space \"{}\".", space_name),
                span,
            )
                .into())
        }
    };

    // Remaining items are channels
    let channel_items: Vec<Value> = items[1..].to_vec();

    if channel_items.len() < 3 {
        let channel_defs = space.channels();
        let missing = channel_defs.get(channel_items.len()).map_or("channel", |c| c.name);
        return Err((
            format!("Missing element ${}.", missing),
            span,
        )
            .into());
    }
    if channel_items.len() > 3 {
        return Err((
            format!(
                "Only 3 elements allowed, but {} were passed.",
                channel_items.len()
            ),
            span,
        )
            .into());
    }

    // Build channels list with optional alpha
    let mut channels_with_alpha = channel_items;
    if let Some(alpha) = alpha_val {
        channels_with_alpha.push(alpha);
    }

    construct_color(space, &channels_with_alpha, channels_with_alpha.len() > 3, span, visitor)
}
