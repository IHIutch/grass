use crate::{builtin::builtin_imports::*, serializer::serialize_number, value::SassNumber};
use crate::color::space::ColorSpace;

use super::{
    angle_value,
    css_color4::construct_color,
    parse_space_arg,
    rgb::{function_string, parse_channels, percentage_or_unitless},
    ParsedChannels,
};

fn hsl_3_args(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    let span = args.span();

    let hue = args.get_err(0, "hue")?;
    let saturation = args.get_err(1, "saturation")?;
    let lightness = args.get_err(2, "lightness")?;
    let alpha = args.default_arg(3, "alpha", Value::Dimension(SassNumber::new_unitless(1.0)));

    if [&hue, &saturation, &lightness, &alpha]
        .iter()
        .copied()
        .any(Value::is_special_function)
    {
        return Ok(Value::String(
            format!(
                "{}({})",
                name,
                Value::List(
                    Rc::new(if args.len() == 4 {
                        vec![hue, saturation, lightness, alpha]
                    } else {
                        vec![hue, saturation, lightness]
                    }),
                    ListSeparator::Comma,
                    Brackets::None
                )
                .to_css_string(args.span(), false)?
            ).into(),
            QuoteKind::None,
        ));
    }

    let hue = angle_value(hue, "hue", span)?;
    let saturation = saturation.assert_number_with_name("saturation", span)?;
    let lightness = lightness.assert_number_with_name("lightness", span)?;
    let alpha = percentage_or_unitless(
        &alpha.assert_number_with_name("alpha", span)?,
        1.0,
        "alpha",
        span,
        visitor,
    )?;

    Ok(Value::Color(Rc::new(Color::from_hsla_fn(
        Number(hue.rem_euclid(360.0)),
        saturation.num / Number(100.0),
        lightness.num / Number(100.0),
        Number(alpha),
    ))))
}

fn inner_hsl(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(4)?;
    let span = args.span();

    let len = args.len();

    if len == 1 || len == 0 {
        match parse_channels(
            name,
            &["hue", "saturation", "lightness"],
            args.get_err(0, "channels")?,
            visitor,
            args.span(),
        )? {
            ParsedChannels::String(s) => Ok(Value::String(s.into(), QuoteKind::None)),
            ParsedChannels::List(list) | ParsedChannels::SlashList(list) => {
                // Check if any channel or alpha is `none` — if so, use modern Color 4 path
                let has_none = list.iter().any(|v| matches!(v, Value::String(s, QuoteKind::None) if s == "none"));
                if has_none {
                    let has_alpha = list.len() > 3;
                    return construct_color(name, ColorSpace::Hsl, &list, has_alpha, span, visitor);
                }
                let args = ArgumentResult {
                    positional: list,
                    named: BTreeMap::new(),
                    separator: ListSeparator::Comma,
                    span: args.span(),
                    touched: FxHashSet::default(),
                };

                hsl_3_args(name, args, visitor)
            }
        }
    } else if len == 2 {
        let hue = args.get_err(0, "hue")?;
        let saturation = args.get_err(1, "saturation")?;

        if hue.is_var() || saturation.is_var() {
            Ok(Value::String(
                function_string(name, &[hue, saturation], visitor, span)?.into(),
                QuoteKind::None,
            ))
        } else {
            Err(("Missing argument $lightness.", args.span()).into())
        }
    } else {
        hsl_3_args(name, args, visitor)
    }
}

pub(crate) fn hsl(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    inner_hsl("hsl", args, visitor)
}

pub(crate) fn hsla(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    inner_hsl("hsla", args, visitor)
}

pub(crate) fn hue(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.hue() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(false, "hue", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.hue(),
        unit: Unit::Deg,
        as_slash: None,
    }))
}

fn global_hue(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.hue() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(true, "hue", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.hue(),
        unit: Unit::Deg,
        as_slash: None,
    }))
}

pub(crate) fn saturation(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.saturation() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(false, "saturation", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.saturation(),
        unit: Unit::Percent,
        as_slash: None,
    }))
}

fn global_saturation(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.saturation() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(true, "saturation", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.saturation(),
        unit: Unit::Percent,
        as_slash: None,
    }))
}

pub(crate) fn lightness(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.lightness() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(false, "lightness", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.lightness(),
        unit: Unit::Percent,
        as_slash: None,
    }))
}

fn global_lightness(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.lightness() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    visitor.emit_deprecation(Deprecation::ColorFunctions, args.span(), || {
        Ok(color_channel_getter_message(true, "lightness", "hsl"))
    })?;

    Ok(Value::Dimension(SassNumber {
        num: color.lightness(),
        unit: Unit::Percent,
        as_slash: None,
    }))
}

pub(crate) fn adjust_hue(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "adjust-hue() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let degrees = angle_value(args.get_err(1, "degrees")?, "degrees", args.span())?;

    let span = args.span();
    let suggested_value = SassNumber {
        num: degrees,
        unit: Unit::Deg,
        as_slash: None,
    };
    let suggestion_text = serialize_number(&suggested_value, visitor.options, span)?;
    visitor.emit_deprecation(Deprecation::ColorFunctions, span, || {
        Ok(format!(
            "adjust-hue() is deprecated. Suggestion:\n\ncolor.adjust($color, $hue: \
             {suggestion_text})\n\nMore info: https://sass-lang.com/d/color-functions"
        ))
    })?;

    Ok(Value::Color(Rc::new(color.adjust_hue(degrees))))
}

fn lighten(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "lighten() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let mut amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds("amount", 0.0, 100.0, args.span())?;

    amount.num /= Number(100.0);

    Ok(Value::Color(Rc::new(color.lighten(amount.num))))
}

fn darken(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "darken() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let mut amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds("amount", 0.0, 100.0, args.span())?;

    amount.num /= Number(100.0);

    Ok(Value::Color(Rc::new(color.darken(amount.num))))
}

fn saturate(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    if args.len() == 1 {
        let val = args.get_err(0, "amount")?;

        // Pass through special functions like var() and calc()
        if val.is_special_function() {
            return Ok(Value::String(
                format!("saturate({})", val.to_css_string(args.span(), false)?).into(),
                QuoteKind::None,
            ));
        }

        let amount = val.assert_number_with_name("amount", args.span())?;

        return Ok(Value::String(
            format!(
                "saturate({})",
                serialize_number(&amount, &Options::default(), args.span())?,
            ).into(),
            QuoteKind::None,
        ));
    }

    visitor.emit_deprecation(Deprecation::GlobalBuiltin, args.span(), || {
        Ok(global_builtin_message("color", "adjust"))
    })?;

    let mut amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds("amount", 0.0, 100.0, args.span())?;

    amount.num /= Number(100.0);

    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "saturate() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    Ok(Value::Color(Rc::new(color.saturate(amount.num))))
}

fn desaturate(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "desaturate() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let mut amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds("amount", 0.0, 100.0, args.span())?;

    amount.num /= Number(100.0);

    Ok(Value::Color(Rc::new(color.desaturate(amount.num))))
}

pub(crate) fn grayscale(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = match args.get_err(0, "color")? {
        Value::Color(c) => c,
        Value::Dimension(SassNumber {
            num: n,
            unit: u,
            as_slash: _,
        }) => {
            return Ok(Value::String(
                format!("grayscale({}{})", n.inspect(), u).into(),
                QuoteKind::None,
            ))
        }
        v => {
            return Err((
                format!("$color: {} is not a color.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };
    Ok(Value::Color(Rc::new(color.grayscale())))
}

/// Global CSS filter overload: passes through var()/calc() as plain CSS.
fn global_grayscale(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();
    let val = args.get_err(0, "color")?;
    // Pass through special functions like var() and calc()
    if val.is_special_function() {
        return Ok(Value::String(
            format!("grayscale({})", val.to_css_string(span, false)?).into(),
            QuoteKind::None,
        ));
    }
    if !matches!(val, Value::Dimension(..)) {
        visitor.emit_deprecation(Deprecation::GlobalBuiltin, span, || {
            Ok(global_builtin_message("color", "grayscale"))
        })?;
    }
    // Re-wrap and delegate to the main implementation
    let new_args = ArgumentResult {
        positional: vec![val],
        named: args.named,
        separator: args.separator,
        span,
        touched: args.touched,
    };
    grayscale(new_args, visitor)
}

pub(crate) fn complement(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", span)?;

    let target_space = parse_space_arg(&mut args, 1, span)?;

    if let Some(space) = target_space {
        if !space.is_polar() {
            return Err((
                format!(
                    "$space: Color space {} doesn't have a hue channel.",
                    space.name()
                ),
                span,
            )
                .into());
        }
        // Check if hue is missing in the target space (powerless→None for legacy via conversion)
        let in_space = color.to_space_powerless_missing(space);
        let hue_idx = space.hue_channel_index().unwrap();
        if in_space.has_missing_channel(hue_idx) {
            let display_color = in_space.with_powerless_as_missing();
            return Err((
                format!(
                    "$hue: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: {}).",
                    Value::Color(Rc::new(display_color)).inspect(span)?
                ),
                span,
            )
                .into());
        }
        Ok(Value::Color(Rc::new(color.complement_in_space(space))))
    } else if !color.color_space().is_legacy() {
        Err((
            "$color: To use color.complement() with a non-legacy color, you must provide a $space.",
            span,
        )
            .into())
    } else {
        // Legacy complement works in HSL space; check if hue is explicitly missing
        let in_hsl = color.to_space(ColorSpace::Hsl);
        let hue_idx = ColorSpace::Hsl.hue_channel_index().unwrap();
        if in_hsl.has_missing_channel(hue_idx) {
            return Err((
                format!(
                    "$hue: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: {}).",
                    Value::Color(Rc::new(in_hsl)).inspect(span)?
                ),
                span,
            )
                .into());
        }
        Ok(Value::Color(Rc::new(color.complement())))
    }
}

/// Global `invert()`: warns unless the `$color` argument is a plain number
/// or special function (`var()`/`calc()`), matching dart-sass's global-only
/// `invert` wrapper. `color.invert()` (the module form) uses `invert`
/// directly, without this check.
fn global_invert(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    let peek = args
        .named
        .get(&Identifier::from("color"))
        .or_else(|| args.positional.first());

    let skip_warn = match peek {
        Some(Value::Dimension(..)) => true,
        Some(v) => v.is_special_function(),
        None => false,
    };

    if !skip_warn {
        visitor.emit_deprecation(Deprecation::GlobalBuiltin, args.span(), || {
            Ok(global_builtin_message("color", "invert"))
        })?;
    }

    invert(args, visitor)
}

pub(crate) fn invert(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let span = args.span();

    let weight = args
        .get(1, "weight")
        .map::<SassResult<_>, _>(|weight| {
            let mut weight = weight.node.assert_number_with_name("weight", span)?;

            weight.assert_bounds("weight", 0.0, 100.0, span)?;

            weight.num /= Number(100.0);

            Ok(weight.num)
        })
        .transpose()?;

    let target_space = parse_space_arg(&mut args, 2, span)?;

    match args.get_err(0, "color")? {
        Value::Color(c) => {
            if let Some(space) = target_space {
                // Convert with powerless→None for legacy spaces (matches dart-sass legacyMissing: true)
                let in_space = c.to_space_powerless_missing(space);
                let channel_defs = space.channels();
                // Only check channels passed through _invertChannel (not preserved/swapped ones).
                // HWB: only hue (channels 1&2 are swapped, preserving missing).
                // HSL/LCH/OKLch: hue + lightness (saturation/chroma preserved).
                // Others: all channels.
                for (i, ch_def) in channel_defs.iter().enumerate() {
                    let skip = if space == ColorSpace::Hwb {
                        i != 0 // only check hue for HWB
                    } else {
                        ch_def.name == "chroma" || ch_def.name == "saturation"
                    };
                    if !skip && in_space.has_missing_channel(i) {
                        let display_color = in_space.with_powerless_as_missing();
                        return Err((
                            format!(
                                "${}: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: {}).",
                                ch_def.name,
                                Value::Color(Rc::new(display_color)).inspect(span)?
                            ),
                            span,
                        )
                            .into());
                    }
                }
                Ok(Value::Color(Rc::new(
                    c.invert_in_space(space, weight.unwrap_or_else(Number::one)),
                )))
            } else if !c.color_space().is_legacy() {
                // Modern colors require $space
                Err((
                    "$color: To use color.invert() with a non-legacy color, you must provide a $space.",
                    span,
                )
                    .into())
            } else {
                // Legacy invert works in RGB space — check for missing channels
                let in_rgb = c.to_space(ColorSpace::Rgb);
                let rgb_defs = ColorSpace::Rgb.channels();
                for (i, rgb_def) in rgb_defs.iter().enumerate() {
                    if in_rgb.has_missing_channel(i) {
                        return Err((
                            format!(
                                "${}: Because the CSS working group is still deciding on the best behavior, Sass doesn't currently support modifying missing channels (color: {}).",
                                rgb_def.name,
                                Value::Color(Rc::new(in_rgb)).inspect(span)?
                            ),
                            span,
                        )
                            .into());
                    }
                }
                Ok(Value::Color(Rc::new(
                    c.invert(weight.unwrap_or_else(Number::one)),
                )))
            }
        }
        Value::Dimension(SassNumber {
            num: n,
            unit: u,
            as_slash: _,
        }) => {
            if weight.is_some() || target_space.is_some() {
                return Err((
                    "Only one argument may be passed to the plain-CSS invert() function.",
                    args.span(),
                )
                    .into());
            }
            Ok(Value::String(
                format!("invert({}{})", n.inspect(), u).into(),
                QuoteKind::None,
            ))
        }
        v => {
            // Pass through special functions like var() and calc()
            if v.is_special_function() {
                if weight.is_some() || target_space.is_some() {
                    return Err((
                        "Only one argument may be passed to the plain-CSS invert() function.",
                        args.span(),
                    )
                        .into());
                }
                return Ok(Value::String(
                    format!("invert({})", v.to_css_string(span, false)?).into(),
                    QuoteKind::None,
                ));
            }
            Err((
                format!("$color: {} is not a color.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    }
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    // hsl/hsla are plain-CSS-compatible constructors; never warn.
    f.insert("hsl", Builtin::new(hsl));
    f.insert("hsla", Builtin::new(hsla));
    f.insert("hue", Builtin::new(global_hue).with_deprecated_global("color", "hue"));
    f.insert(
        "saturation",
        Builtin::new(global_saturation).with_deprecated_global("color", "saturation"),
    );
    f.insert("adjust-hue", Builtin::new(adjust_hue).with_deprecated_global("color", "adjust"));
    f.insert(
        "lightness",
        Builtin::new(global_lightness).with_deprecated_global("color", "lightness"),
    );
    f.insert("lighten", Builtin::new(lighten).with_deprecated_global("color", "adjust"));
    f.insert("darken", Builtin::new(darken).with_deprecated_global("color", "adjust"));
    // saturate/grayscale/invert warn conditionally on argument shape; the
    // warning is emitted inline in their own function bodies above, not
    // generically here (avoids a double warning).
    f.insert("saturate", Builtin::new(saturate));
    f.insert("desaturate", Builtin::new(desaturate).with_deprecated_global("color", "adjust"));
    f.insert("grayscale", Builtin::new(global_grayscale));
    f.insert("complement", Builtin::new(complement).with_deprecated_global("color", "complement"));
    f.insert("invert", Builtin::new(global_invert));
}
