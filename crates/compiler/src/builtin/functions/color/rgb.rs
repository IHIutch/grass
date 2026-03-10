use crate::{builtin::builtin_imports::*, serializer::inspect_number};
use crate::color::space::ColorSpace;

use super::ParsedChannels;

/// Try to parse a string part from a "channel/alpha" split as a value.
/// Handles "none", plain numbers (0.4), and percentages (40%).
pub(crate) fn parse_slash_part(s: &str) -> Option<Value> {
    if s == "none" {
        Some(Value::String("none".to_owned(), QuoteKind::None))
    } else if let Some(num_str) = s.strip_suffix('%') {
        num_str
            .parse::<f64>()
            .ok()
            .map(|n| Value::Dimension(SassNumber {
                num: Number(n),
                unit: Unit::Percent,
                as_slash: None,
            }))
    } else if let Some((num_str, unit)) = parse_number_with_unit(s) {
        num_str.parse::<f64>().ok().map(|n| Value::Dimension(SassNumber {
            num: Number(n),
            unit,
            as_slash: None,
        }))
    } else {
        s.parse::<f64>()
            .ok()
            .map(|n| Value::Dimension(SassNumber::new_unitless(n)))
    }
}

/// Try to parse a string as a number with a known CSS unit suffix (e.g., "3deg", "0.5turn").
fn parse_number_with_unit(s: &str) -> Option<(&str, Unit)> {
    let units = [
        ("deg", Unit::Deg),
        ("grad", Unit::Grad),
        ("rad", Unit::Rad),
        ("turn", Unit::Turn),
    ];
    for (suffix, unit) in &units {
        if let Some(num_str) = s.strip_suffix(suffix) {
            if !num_str.is_empty() && num_str.bytes().last().map_or(false, |b| b.is_ascii_digit() || b == b'.') {
                return Some((num_str, unit.clone()));
            }
        }
    }
    None
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

fn inner_rgb_2_arg(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    // rgba(var(--foo), 0.5) is valid CSS because --foo might be `123, 456, 789`
    // and functions are parsed after variable substitution.
    let color = args.get_err(0, "color")?;
    let alpha = args.get_err(1, "alpha")?;

    let is_compressed = visitor.options.is_compressed();

    if color.is_var() {
        return Ok(Value::String(
            function_string(name, &[color, alpha], visitor, args.span())?,
            QuoteKind::None,
        ));
    } else if alpha.is_var() {
        match &color {
            Value::Color(color) => {
                return Ok(Value::String(
                    format!(
                        "{}({}, {}, {}, {})",
                        name,
                        color.red().to_string(is_compressed),
                        color.green().to_string(is_compressed),
                        color.blue().to_string(is_compressed),
                        alpha.to_css_string(args.span(), is_compressed)?
                    ),
                    QuoteKind::None,
                ));
            }
            _ => {
                return Ok(Value::String(
                    function_string(name, &[color, alpha], visitor, args.span())?,
                    QuoteKind::None,
                ))
            }
        }
    } else if alpha.is_special_function() {
        let color = color.assert_color_with_name("color", args.span())?;

        return Ok(Value::String(
            format!(
                "{}({}, {}, {}, {})",
                name,
                color.red().to_string(is_compressed),
                color.green().to_string(is_compressed),
                color.blue().to_string(is_compressed),
                alpha.to_css_string(args.span(), is_compressed)?
            ),
            QuoteKind::None,
        ));
    }

    let color = color.assert_color_with_name("color", args.span())?;
    let alpha = alpha.assert_number_with_name("alpha", args.span())?;
    Ok(Value::Color(Arc::new(color.with_alpha(Number(
        percentage_or_unitless(&alpha, 1.0, "alpha", args.span(), visitor)?,
    )))))
}

fn inner_rgb_3_arg(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    let alpha = if args.len() > 3 {
        args.get(3, "alpha")
    } else {
        None
    };

    let red = args.get_err(0, "red")?;
    let green = args.get_err(1, "green")?;
    let blue = args.get_err(2, "blue")?;

    if red.is_special_function()
        || green.is_special_function()
        || blue.is_special_function()
        || alpha
            .as_ref()
            .map(|alpha| alpha.node.is_special_function())
            .unwrap_or(false)
    {
        let fn_string = if let Some(alpha) = alpha {
            function_string(
                name,
                &[red, green, blue, alpha.node],
                visitor,
                args.span(),
            )?
        } else {
            function_string(name, &[red, green, blue], visitor, args.span())?
        };

        return Ok(Value::String(fn_string, QuoteKind::None));
    }

    let span = args.span();

    let red = red.assert_number_with_name("red", span)?;
    let green = green.assert_number_with_name("green", span)?;
    let blue = blue.assert_number_with_name("blue", span)?;

    Ok(Value::Color(Arc::new(Color::from_rgba_fn(
        Number(percentage_or_unitless(
            &red, 255.0, "red", span, visitor,
        )?),
        Number(percentage_or_unitless(
            &green, 255.0, "green", span, visitor,
        )?),
        Number(percentage_or_unitless(
            &blue, 255.0, "blue", span, visitor,
        )?),
        Number(
            alpha
                .map(|alpha| {
                    percentage_or_unitless(
                        &alpha.node.assert_number_with_name("alpha", span)?,
                        1.0,
                        "alpha",
                        span,
                        visitor,
                    )
                })
                .transpose()?
                .unwrap_or(1.0),
        ),
    ))))
}

pub(crate) fn percentage_or_unitless(
    number: &SassNumber,
    max: f64,
    name: &str,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<f64> {
    let value = if number.unit == Unit::None {
        number.num
    } else if number.unit == Unit::Percent {
        (number.num * Number(max)) / Number(100.0)
    } else {
        return Err((
            format!(
                "${name}: Expected {} to have no units or \"%\".",
                inspect_number(number, visitor.options, span)?,
                name = name,
            ),
            span,
        )
            .into());
    };

    Ok(value.0)
}

fn is_var_slash(value: &Value) -> bool {
    match value {
        Value::String(text, QuoteKind::Quoted) => {
            text.to_ascii_lowercase().starts_with("var(") && text.contains('/')
        }
        _ => false,
    }
}

pub(crate) fn parse_channels(
    name: &'static str,
    arg_names: &[&'static str],
    mut channels: Value,
    visitor: &mut Visitor,
    span: Span,
) -> SassResult<ParsedChannels> {
    if channels.is_var() {
        let fn_string = function_string(name, &[channels], visitor, span)?;
        return Ok(ParsedChannels::String(fn_string));
    }

    let original_channels = channels.clone();

    let mut alpha_from_slash_list = None;

    if channels.separator() == ListSeparator::Slash {
        let list = channels.clone().as_list();
        if list.len() != 2 {
            return Err((
                format!(
                    "Only 2 slash-separated elements allowed, but {} {} passed.",
                    list.len(),
                    if list.len() == 1 { "was" } else { "were" }
                ),
                span,
            )
                .into());
        }

        channels = list[0].clone();
        let inner_alpha_from_slash_list = list[1].clone();

        let is_none = matches!(&inner_alpha_from_slash_list, Value::String(s, QuoteKind::None) if s == "none");
        if !inner_alpha_from_slash_list.is_special_function() && !is_none {
            inner_alpha_from_slash_list
                .clone()
                .assert_number_with_name("alpha", span)?;
        }

        alpha_from_slash_list = Some(inner_alpha_from_slash_list);

        if list[0].is_var() {
            let fn_string = function_string(name, &[original_channels], visitor, span)?;
            return Ok(ParsedChannels::String(fn_string));
        }
    }

    let is_comma_separated = channels.separator() == ListSeparator::Comma;
    let is_bracketed = matches!(channels, Value::List(_, _, Brackets::Bracketed));

    if is_comma_separated || is_bracketed {
        let mut err_buffer = "$channels must be".to_owned();

        if is_bracketed {
            err_buffer.push_str(" an unbracketed");
        }

        if is_comma_separated {
            if is_bracketed {
                err_buffer.push(',');
            } else {
                err_buffer.push_str(" a");
            }

            err_buffer.push_str(" space-separated");
        }

        err_buffer.push_str(" list.");

        return Err((err_buffer, span).into());
    }

    let mut list = channels.clone().as_list();

    if list.len() > 3 {
        return Err((
            format!("Only 3 elements allowed, but {} were passed.", list.len()),
            span,
        )
            .into());
    } else if list.len() < 3 {
        if list.iter().any(Value::is_var)
            || (!list.is_empty() && is_var_slash(list.last().unwrap()))
        {
            let fn_string = function_string(name, &[original_channels], visitor, span)?;
            return Ok(ParsedChannels::String(fn_string));
        } else {
            let argument = arg_names[list.len()];
            return Err((
                format!("Missing element ${argument}.", argument = argument),
                span,
            )
                .into());
        }
    }

    if let Some(alpha_from_slash_list) = alpha_from_slash_list {
        list.push(alpha_from_slash_list);
        return Ok(ParsedChannels::List(list));
    }

    #[allow(clippy::collapsible_match)]
    match &list[2] {
        Value::Dimension(SassNumber { as_slash, .. }) => match as_slash {
            Some(slash) => Ok(ParsedChannels::List(vec![
                list[0].clone(),
                list[1].clone(),
                // todo: superfluous clones
                Value::Dimension(slash.0.clone()),
                Value::Dimension(slash.1.clone()),
            ])),
            None => Ok(ParsedChannels::List(list)),
        },
        Value::String(text, QuoteKind::None) if text.contains('/') => {
            // Try to split "channel/alpha" strings like "none/0.4" or "40%/none"
            let parts: Vec<&str> = text.splitn(2, '/').collect();
            if parts.len() == 2 {
                let channel_val = parse_slash_part(parts[0].trim());
                let alpha_val = parse_slash_part(parts[1].trim());

                if let (Some(ch), Some(al)) = (channel_val, alpha_val) {
                    return Ok(ParsedChannels::List(vec![
                        list[0].clone(),
                        list[1].clone(),
                        ch,
                        al,
                    ]));
                }
            }

            let fn_string = function_string(name, &[channels], visitor, span)?;
            Ok(ParsedChannels::String(fn_string))
        }
        _ => Ok(ParsedChannels::List(list)),
    }
}

fn inner_rgb(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(4)?;

    match args.len() {
        0 | 1 => {
            let span = args.span();
            match parse_channels(
                name,
                &["red", "green", "blue"],
                args.get_err(0, "channels")?,
                visitor,
                span,
            )? {
                ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
                ParsedChannels::List(list) => {
                    // Check if any channel is `none` — if so, use modern Color 4 path
                    let has_none = list.iter().any(|v| matches!(v, Value::String(s, QuoteKind::None) if s == "none"));
                    if has_none {
                        let has_alpha = list.len() > 3;
                        return super::css_color4::construct_color(name, ColorSpace::Rgb, &list, has_alpha, span, visitor);
                    }
                    let args = ArgumentResult {
                        positional: list,
                        named: BTreeMap::new(),
                        separator: ListSeparator::Comma,
                        span,
                        touched: BTreeSet::new(),
                    };

                    inner_rgb_3_arg(name, args, visitor)
                }
            }
        }
        2 => inner_rgb_2_arg(name, args, visitor),
        _ => inner_rgb_3_arg(name, args, visitor),
    }
}

pub(crate) fn rgb(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    inner_rgb("rgb", args, visitor)
}

pub(crate) fn rgba(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    inner_rgb("rgba", args, visitor)
}

pub(crate) fn red(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.red() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    Ok(Value::Dimension(SassNumber::new_unitless(color.red())))
}

pub(crate) fn green(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.green() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    Ok(Value::Dimension(SassNumber::new_unitless(color.green())))
}

pub(crate) fn blue(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.blue() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    Ok(Value::Dimension(SassNumber::new_unitless(color.blue())))
}

pub(crate) fn mix(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(4)?;
    let span = args.span();

    let color1 = args
        .get_err(0, "color1")?
        .assert_color_with_name("color1", span)?;

    let color2 = args
        .get_err(1, "color2")?
        .assert_color_with_name("color2", span)?;

    let weight = match args.default_arg(
        2,
        "weight",
        Value::Dimension(SassNumber::new_unitless(50.0)),
    ) {
        Value::Dimension(mut num) => {
            num.assert_bounds("weight", 0.0, 100.0, span)?;
            num.num /= Number(100.0);
            num.num
        }
        v => {
            return Err((
                format!(
                    "$weight: {} is not a number.",
                    v.to_css_string(span, visitor.options.is_compressed())?
                ),
                span,
            )
                .into())
        }
    };

    // Parse $method parameter
    let method = args.get(3, "method");

    let method_value = match method {
        None => None,
        Some(v) => match v.node {
            Value::Null => None,
            other => Some(other),
        },
    };

    match method_value {
        None => {
            // No method: legacy behavior, but both colors must be legacy
            if !color1.color_space().is_legacy() {
                return Err((
                    format!(
                        "$color1: To use color.mix() with non-legacy color {}, you must provide a $method.",
                        Value::Color(color1.clone()).inspect(span)?
                    ),
                    span,
                ).into());
            }
            if !color2.color_space().is_legacy() {
                return Err((
                    format!(
                        "$color2: To use color.mix() with non-legacy color {}, you must provide a $method.",
                        Value::Color(color2.clone()).inspect(span)?
                    ),
                    span,
                ).into());
            }
            Ok(Value::Color(Arc::new(color1.mix(&color2, weight))))
        }
        Some(method_val) => {
            let (space, hue_method) = parse_interpolation_method(method_val, span)?;
            Ok(Value::Color(Arc::new(
                color1.mix_with_method(&color2, weight.0, space, hue_method),
            )))
        }
    }
}

fn parse_interpolation_method(
    value: Value,
    span: Span,
) -> SassResult<(ColorSpace, crate::color::HueInterpolationMethod)> {
    use crate::color::HueInterpolationMethod;

    let parts: Vec<String> = match &value {
        Value::String(s, QuoteKind::None) => vec![s.clone()],
        Value::String(_, QuoteKind::Quoted) => {
            return Err((
                format!(
                    "$method: Expected {} to be an unquoted string.",
                    value.inspect(span)?
                ),
                span,
            ).into());
        }
        Value::List(items, ListSeparator::Space, _) => {
            let mut parts = Vec::new();
            for item in items {
                match item {
                    Value::String(s, QuoteKind::None) => parts.push(s.clone()),
                    _ => {
                        return Err((
                            format!(
                                "$method: Expected {} to be an unquoted string.",
                                value.inspect(span)?
                            ),
                            span,
                        ).into());
                    }
                }
            }
            parts
        }
        _ => {
            return Err((
                format!(
                    "$method: {} is not a string.",
                    value.inspect(span)?
                ),
                span,
            ).into());
        }
    };

    if parts.is_empty() {
        return Err(("$method: Must not be empty.", span).into());
    }

    let space = match ColorSpace::from_name(&parts[0]) {
        Some(s) => s,
        None => {
            return Err((
                format!("$method: Unknown color space \"{}\".", parts[0]),
                span,
            ).into());
        }
    };

    let hue_method = if parts.len() == 1 {
        HueInterpolationMethod::Shorter
    } else if parts.len() == 3 && parts[2].eq_ignore_ascii_case("hue") {
        let method = match parts[1].to_ascii_lowercase().as_str() {
            "shorter" => HueInterpolationMethod::Shorter,
            "longer" => HueInterpolationMethod::Longer,
            "increasing" => HueInterpolationMethod::Increasing,
            "decreasing" => HueInterpolationMethod::Decreasing,
            _ => {
                return Err((
                    format!(
                        "$method: Unknown hue interpolation method \"{}\".",
                        parts[1]
                    ),
                    span,
                ).into());
            }
        };

        if !space.is_polar() {
            let method_name = format!("HueInterpolationMethod.{} hue", parts[1].to_ascii_lowercase());
            return Err((
                format!(
                    "$method: Hue interpolation method \"{}\" may not be set for rectangular color space {}.",
                    method_name,
                    space.name()
                ),
                span,
            ).into());
        }

        method
    } else if parts.len() == 2 {
        // Could be "oklch shorter" without "hue" or some other invalid format
        return Err((
            format!(
                "$method: Expected \"{}\" to end with \"hue\".",
                parts.join(" ")
            ),
            span,
        ).into());
    } else {
        return Err((
            format!(
                "$method: Expected \"{}\" to be a valid interpolation method.",
                parts.join(" ")
            ),
            span,
        ).into());
    };

    Ok((space, hue_method))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("rgb", Builtin::new(rgb));
    f.insert("rgba", Builtin::new(rgba));
    f.insert("red", Builtin::new(red));
    f.insert("green", Builtin::new(green));
    f.insert("blue", Builtin::new(blue));
    f.insert("mix", Builtin::new(mix));
}
