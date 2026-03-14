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
            } else if num.has_compatible_units(&Unit::Deg) {
                // Accept other angle units (turn, rad, grad) and convert to degrees
                let factor = crate::value::conversion_factor(&num.unit, &Unit::Deg).unwrap();
                num.num.0 * factor
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
/// `name` is the CSS function name (e.g. "lab", "oklch") used for passthrough
/// when channels contain special functions like var(), calc(), env(), attr().
pub(crate) fn construct_color(
    name: &'static str,
    space: ColorSpace,
    channels: &[Value],
    has_alpha: bool,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    construct_color_inner(name, space, channels, has_alpha, false, span, visitor)
}

/// Like `construct_color`, but `alpha_from_slash_list` indicates the alpha came
/// from a Sass slash-list (e.g. `list.slash(channels, alpha)`), which requires
/// spaces around the `/` in passthrough output for modern color functions.
pub(crate) fn construct_color_slash_list(
    name: &'static str,
    space: ColorSpace,
    channels: &[Value],
    has_alpha: bool,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    construct_color_inner(name, space, channels, has_alpha, true, span, visitor)
}

fn construct_color_inner(
    name: &'static str,
    space: ColorSpace,
    channels: &[Value],
    has_alpha: bool,
    alpha_from_slash_list: bool,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    // If any channel is a special function (var(), calc(), env(), attr(), etc.),
    // pass through as a plain CSS string instead of trying to evaluate.
    let any_special = channels.iter().any(|v| v.is_special_function());
    if any_special {
        let is_compressed = visitor.options.is_compressed();
        // RGB and HSL use comma-separated syntax; HWB and all modern spaces use space-separated
        let comma_sep = matches!(space, ColorSpace::Rgb | ColorSpace::Hsl);
        let sep = if comma_sep { ", " } else { " " };
        let mut result = String::new();
        result.push_str(name);
        result.push('(');
        for (i, ch) in channels.iter().enumerate() {
            if has_alpha && i == 3 {
                if comma_sep {
                    result.push_str(sep);
                } else if alpha_from_slash_list {
                    // Slash-list input → spaces around slash: `lab(1% 2 3 / var(--c))`
                    result.push_str(" / ");
                } else {
                    // Parsed slash → no spaces: `lab(1% 2 3/0.4)`
                    result.push('/');
                }
            } else if i > 0 {
                result.push_str(sep);
            }
            result.push_str(&ch.to_css_string(span, is_compressed)?);
        }
        result.push(')');
        return Ok(Value::String(result.into(), QuoteKind::None));
    }

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

    // Apply dart-sass clamping rules for color construction:
    // - Lightness: clamp to [min, max], NaN → min
    // - Chroma (LCH/OKLCh): clamp negative to 0
    // - Hue: normalize to [0, 360) via modulo
    // - Alpha: clamp to [0, 1]
    let mut channels_arr = [c0, c1, c2];
    for (i, ch) in channels_arr.iter_mut().enumerate() {
        if let Some(val) = ch {
            if channel_defs[i].name == "lightness" {
                // NaN → min, then clamp to [min, max]
                *val = if val.is_nan() {
                    channel_defs[i].min
                } else {
                    val.clamp(channel_defs[i].min, channel_defs[i].max)
                };
            } else if channel_defs[i].is_polar {
                // Hue: normalize to [0, 360) via modulo. NaN stays NaN.
                // Infinity becomes NaN (infinity % 360 = NaN per IEEE 754).
                if !val.is_nan() {
                    *val = val.rem_euclid(360.0);
                }
            } else if channel_defs[i].name == "chroma" {
                // Chroma: clamp negative to 0 (including -infinity), NaN → 0
                if val.is_nan() || *val < 0.0 {
                    *val = 0.0;
                }
            }
        }
    }

    // Clamp alpha to [0, 1]
    let alpha = alpha.map(|a| {
        if a.is_nan() { 0.0 } else { a.clamp(0.0, 1.0) }
    });

    use crate::color::{Color, ColorFormat};

    Ok(Value::Color(Rc::new(Color::for_space(
        space, channels_arr, alpha,
        ColorFormat::Infer,
    ))))
}

/// `lab($channels)` - construct a CIE Lab color
pub(crate) fn lab(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    let parsed = parse_channels(
        "lab",
        &["lightness", "a", "b"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )?;
    match &parsed {
        ParsedChannels::String(s) => Ok(Value::String(s.clone().into(), QuoteKind::None)),
        ParsedChannels::List(list) | ParsedChannels::SlashList(list) => {
            let is_slash_list = matches!(parsed, ParsedChannels::SlashList(_));
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $a.", span).into());
            }
            if is_slash_list {
                construct_color_slash_list("lab", ColorSpace::Lab, list, has_alpha, span, visitor)
            } else {
                construct_color("lab", ColorSpace::Lab, list, has_alpha, span, visitor)
            }
        }
    }
}

/// `lch($channels)` - construct a CIE LCH color
pub(crate) fn lch(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    let parsed = parse_channels(
        "lch",
        &["lightness", "chroma", "hue"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )?;
    match &parsed {
        ParsedChannels::String(s) => Ok(Value::String(s.clone().into(), QuoteKind::None)),
        ParsedChannels::List(list) | ParsedChannels::SlashList(list) => {
            let is_slash_list = matches!(parsed, ParsedChannels::SlashList(_));
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $chroma.", span).into());
            }
            if is_slash_list {
                construct_color_slash_list("lch", ColorSpace::Lch, list, has_alpha, span, visitor)
            } else {
                construct_color("lch", ColorSpace::Lch, list, has_alpha, span, visitor)
            }
        }
    }
}

/// `oklab($channels)` - construct an OKLab color
pub(crate) fn oklab(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    let parsed = parse_channels(
        "oklab",
        &["lightness", "a", "b"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )?;
    match &parsed {
        ParsedChannels::String(s) => Ok(Value::String(s.clone().into(), QuoteKind::None)),
        ParsedChannels::List(list) | ParsedChannels::SlashList(list) => {
            let is_slash_list = matches!(parsed, ParsedChannels::SlashList(_));
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $a.", span).into());
            }
            if is_slash_list {
                construct_color_slash_list("oklab", ColorSpace::Oklab, list, has_alpha, span, visitor)
            } else {
                construct_color("oklab", ColorSpace::Oklab, list, has_alpha, span, visitor)
            }
        }
    }
}

/// `oklch($channels)` - construct an OKLch color
pub(crate) fn oklch(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();

    let parsed = parse_channels(
        "oklch",
        &["lightness", "chroma", "hue"],
        args.get_err(0, "channels")?,
        visitor,
        span,
    )?;
    match &parsed {
        ParsedChannels::String(s) => Ok(Value::String(s.clone().into(), QuoteKind::None)),
        ParsedChannels::List(list) | ParsedChannels::SlashList(list) => {
            let is_slash_list = matches!(parsed, ParsedChannels::SlashList(_));
            let has_alpha = list.len() > 3;
            if list.len() < 3 {
                return Err(("Missing element $chroma.", span).into());
            }
            if is_slash_list {
                construct_color_slash_list("oklch", ColorSpace::Oklch, list, has_alpha, span, visitor)
            } else {
                construct_color("oklch", ColorSpace::Oklch, list, has_alpha, span, visitor)
            }
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
        return Ok(Value::String(fn_string.into(), QuoteKind::None));
    }

    // Validate list format
    if matches!(description, Value::List(_, _, Brackets::Bracketed)) {
        return Err((
            format!(
                "$description: Expected an unbracketed list, was {}",
                description.inspect(span)?
            ),
            span,
        )
            .into());
    }

    if description.separator() == ListSeparator::Comma {
        return Err((
            format!(
                "$description: Expected a space- or slash-separated list, was {}",
                description.inspect(span)?
            ),
            span,
        )
            .into());
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

    // Relative color syntax: `color(from <color> <space> <channels...>)`
    // Detect unquoted `from` keyword and pass through as CSS string.
    if let Some(Value::String(s, QuoteKind::None)) = items.first() {
        if s.eq_ignore_ascii_case("from") {
            let fn_string = function_string("color", &[description], visitor, span)?;
            return Ok(Value::String(fn_string.into(), QuoteKind::None));
        }
    }

    if items.is_empty() {
        return Err(("Missing color space name.", span).into());
    }

    // First item is the color space name
    let space_name = match &items[0] {
        Value::String(s, QuoteKind::None) => s.clone(),
        v if v.is_special_function() || v.is_var() => {
            let fn_string = function_string("color", &[description], visitor, span)?;
            return Ok(Value::String(fn_string.into(), QuoteKind::None));
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
    let mut channel_items: Vec<Value> = items[1..].to_vec();

    // Check if the last channel has as_slash (i.e. `0.3 / 0.4` parsed as division).
    // If so, split it back into channel value and alpha.
    if alpha_val.is_none() {
        if let Some(Value::Dimension(SassNumber { as_slash: Some(slash), .. })) = channel_items.last() {
            let alpha = Value::Dimension(slash.1.clone());
            let chan = Value::Dimension(slash.0.clone());
            let last_idx = channel_items.len() - 1;
            channel_items[last_idx] = chan;
            alpha_val = Some(alpha);
        } else if let Some(Value::String(text, QuoteKind::None)) = channel_items.last() {
            // Handle strings like "none/0.4" or "0.3/none" from unresolved slash expressions
            if let Some(slash_pos) = text.find('/') {
                let ch_str = text[..slash_pos].trim();
                let al_str = text[slash_pos + 1..].trim();
                if let (Some(ch), Some(al)) = (
                    super::rgb::parse_slash_part(ch_str),
                    super::rgb::parse_slash_part(al_str),
                ) {
                    let last_idx = channel_items.len() - 1;
                    channel_items[last_idx] = ch;
                    alpha_val = Some(al);
                }
            }
        }
    }

    // If there are fewer than 3 channels but any is a special function (var(), calc(), etc.),
    // the var() could expand to multiple values at runtime — pass through as CSS string.
    if channel_items.len() < 3 {
        if channel_items.iter().any(|v| v.is_special_function() || v.is_var()) {
            let is_compressed = visitor.options.is_compressed();
            let mut result = format!("color({}", space_name);
            for ch in &channel_items {
                result.push(' ');
                result.push_str(&ch.to_css_string(span, is_compressed)?);
            }
            if let Some(alpha) = &alpha_val {
                result.push('/');
                result.push_str(&alpha.to_css_string(span, is_compressed)?);
            }
            result.push(')');
            return Ok(Value::String(result.into(), QuoteKind::None));
        }
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

    // Check if any channel or alpha is a special function — pass through as CSS string
    let alpha_is_special = alpha_val.as_ref().is_some_and(|a| a.is_special_function());
    if channel_items.iter().any(|v| v.is_special_function()) || alpha_is_special {
        let is_compressed = visitor.options.is_compressed();
        let mut result = format!("color({}", space_name);
        for ch in &channel_items {
            result.push(' ');
            result.push_str(&ch.to_css_string(span, is_compressed)?);
        }
        if let Some(alpha) = &alpha_val {
            // dart-sass uses no spaces around / for color() passthrough
            result.push('/');
            result.push_str(&alpha.to_css_string(span, is_compressed)?);
        }
        result.push(')');
        return Ok(Value::String(result.into(), QuoteKind::None));
    }

    // Build channels list with optional alpha
    let mut channels_with_alpha = channel_items;
    if let Some(alpha) = alpha_val {
        channels_with_alpha.push(alpha);
    }

    construct_color("color", space, &channels_with_alpha, channels_with_alpha.len() > 3, span, visitor)
}
