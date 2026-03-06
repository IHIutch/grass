use crate::builtin::builtin_imports::*;
use crate::color::space::ColorSpace;

fn bool_to_value(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// `color.space($color)` - returns the color space name as a string
pub(crate) fn space(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    Ok(Value::String(
        color.color_space().name().to_owned(),
        QuoteKind::None,
    ))
}

/// `color.to-space($color, $space)` - convert a color to a different space
pub(crate) fn to_space(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let space_name = args
        .get_err(1, "space")?;

    let space_str = match &space_name {
        Value::String(s, _) => s.clone(),
        v => {
            return Err((
                format!(
                    "$space: {} is not a string.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let target_space = ColorSpace::from_name(&space_str).ok_or_else(|| {
        (
            format!("$space: Unknown color space \"{}\".", space_str),
            span,
        )
    })?;

    Ok(Value::Color(Arc::new(color.to_space(target_space))))
}

/// `color.is-legacy($color)` - check if color is in a legacy space
pub(crate) fn is_legacy(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    Ok(bool_to_value(color.color_space().is_legacy()))
}

/// `color.is-missing($color, $channel)` - check if a channel is missing (none)
pub(crate) fn is_missing(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let channel_name = args
        .get_err(1, "channel")?;

    let channel_str = match &channel_name {
        Value::String(s, _) => s.clone(),
        v => {
            return Err((
                format!(
                    "$channel: {} is not a string.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let result = if channel_str == "alpha" {
        color.has_missing_alpha()
    } else {
        let channels = color.color_space().channels();
        match channels.iter().position(|c| c.name == channel_str.as_str()) {
            Some(idx) => color.has_missing_channel(idx),
            None => {
                return Err((
                    format!(
                        "$channel: Color {} doesn't have a channel named \"{}\".",
                        color.color_space().name(),
                        channel_str
                    ),
                    span,
                )
                    .into())
            }
        }
    };

    Ok(bool_to_value(result))
}

/// `color.channel($color, $channel, $space: null)` - get a channel value
pub(crate) fn channel(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let channel_name = args
        .get_err(1, "channel")?;

    let channel_str = match &channel_name {
        Value::String(s, _) => s.clone(),
        v => {
            return Err((
                format!(
                    "$channel: {} is not a string.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let target_space = match args.get(2, "space") {
        Some(space_val) => {
            let space_str = match &space_val.node {
                Value::String(s, _) => s.clone(),
                Value::Null => {
                    // null means use the color's own space
                    color.color_space().name().to_owned()
                }
                v => {
                    return Err((
                        format!(
                            "$space: {} is not a string.",
                            v.inspect(span)?
                        ),
                        span,
                    )
                        .into())
                }
            };
            ColorSpace::from_name(&space_str).ok_or_else(|| {
                (
                    format!("$space: Unknown color space \"{}\".", space_str),
                    span,
                )
            })?
        }
        None => color.color_space(),
    };

    let color_in_space = if target_space == color.color_space() {
        color.as_ref().clone()
    } else {
        color.to_space(target_space)
    };

    if channel_str == "alpha" {
        return Ok(Value::Dimension(SassNumber::new_unitless(color_in_space.alpha())));
    }

    let channels = target_space.channels();
    let idx = channels.iter().position(|c| c.name == channel_str.as_str());

    match idx {
        Some(i) => {
            let val = color_in_space.channel_value(i);
            let unit = if channels[i].is_polar {
                Unit::Deg
            } else if target_space.is_legacy() && channel_str == "saturation"
                || channel_str == "lightness"
                || channel_str == "whiteness"
                || channel_str == "blackness"
            {
                Unit::Percent
            } else {
                Unit::None
            };

            Ok(Value::Dimension(SassNumber {
                num: val,
                unit,
                as_slash: None,
            }))
        }
        None => Err((
            format!(
                "$channel: Color {} doesn't have a channel named \"{}\".",
                target_space.name(),
                channel_str
            ),
            span,
        )
            .into()),
    }
}

/// `color.is-in-gamut($color, $space: null)` - check if all channels are within bounds
pub(crate) fn is_in_gamut(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let target_space = match args.get(1, "space") {
        Some(space_val) => {
            let space_str = match &space_val.node {
                Value::String(s, _) => s.clone(),
                Value::Null => color.color_space().name().to_owned(),
                v => {
                    return Err((
                        format!(
                            "$space: {} is not a string.",
                            v.inspect(span)?
                        ),
                        span,
                    )
                        .into())
                }
            };
            ColorSpace::from_name(&space_str).ok_or_else(|| {
                (
                    format!("$space: Unknown color space \"{}\".", space_str),
                    span,
                )
            })?
        }
        None => color.color_space(),
    };

    let color_in_space = if target_space == color.color_space() {
        color.as_ref().clone()
    } else {
        color.to_space(target_space)
    };

    Ok(bool_to_value(color_in_space.is_in_gamut()))
}

/// `color.to-gamut($color, $space: null, $method)` - map a color to be within its gamut
pub(crate) fn to_gamut(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let target_space = match args.get(1, "space") {
        Some(space_val) => {
            let space_str = match &space_val.node {
                Value::String(s, _) => s.clone(),
                Value::Null => color.color_space().name().to_owned(),
                v => {
                    return Err((
                        format!(
                            "$space: {} is not a string.",
                            v.inspect(span)?
                        ),
                        span,
                    )
                        .into())
                }
            };
            ColorSpace::from_name(&space_str).ok_or_else(|| {
                (
                    format!("$space: Unknown color space \"{}\".", space_str),
                    span,
                )
            })?
        }
        None => color.color_space(),
    };

    let method = args.get_err(2, "method")?;
    let method_str = match &method {
        Value::String(s, _) => s.clone(),
        v => {
            return Err((
                format!(
                    "$method: {} is not a string.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let color_in_space = if target_space == color.color_space() {
        color.as_ref().clone()
    } else {
        color.to_space(target_space)
    };

    let gamut_mapped = match method_str.as_str() {
        "clip" => color_in_space.to_gamut_clip(),
        "local-minde" => color_in_space.to_gamut_local_minde(),
        _ => {
            return Err((
                format!(
                    "$method: Unknown gamut mapping method \"{}\". Must be \"clip\" or \"local-minde\".",
                    method_str
                ),
                span,
            )
                .into())
        }
    };

    // Convert back to original space if we converted
    let result = if target_space != color.color_space() {
        gamut_mapped.to_space(color.color_space())
    } else {
        gamut_mapped
    };

    Ok(Value::Color(Arc::new(result)))
}

/// `color.is-powerless($color, $channel, $space: null)` - check for powerless channels
pub(crate) fn is_powerless(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let channel_name = args
        .get_err(1, "channel")?;

    let channel_str = match &channel_name {
        Value::String(s, _) => s.clone(),
        v => {
            return Err((
                format!(
                    "$channel: {} is not a string.",
                    v.inspect(span)?
                ),
                span,
            )
                .into())
        }
    };

    let target_space = match args.get(2, "space") {
        Some(space_val) => {
            let space_str = match &space_val.node {
                Value::String(s, _) => s.clone(),
                Value::Null => color.color_space().name().to_owned(),
                v => {
                    return Err((
                        format!(
                            "$space: {} is not a string.",
                            v.inspect(span)?
                        ),
                        span,
                    )
                        .into())
                }
            };
            ColorSpace::from_name(&space_str).ok_or_else(|| {
                (
                    format!("$space: Unknown color space \"{}\".", space_str),
                    span,
                )
            })?
        }
        None => color.color_space(),
    };

    let color_in_space = if target_space == color.color_space() {
        color.as_ref().clone()
    } else {
        color.to_space(target_space)
    };

    let channels = target_space.channels();
    let idx = channels.iter().position(|c| c.name == channel_str.as_str());

    match idx {
        Some(i) => Ok(bool_to_value(color_in_space.is_channel_powerless(i))),
        None => Err((
            format!(
                "$channel: Color {} doesn't have a channel named \"{}\".",
                target_space.name(),
                channel_str
            ),
            span,
        )
            .into()),
    }
}
