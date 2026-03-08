use crate::{
    builtin::{builtin_imports::*, color::angle_value},
    color::{space::ColorSpace, ColorFormat},
    utils::to_sentence,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum UpdateComponents {
    Change,
    Adjust,
    Scale,
}

/// Parse a channel value for a modern color space.
/// Returns the adjustment value in channel units.
fn parse_modern_channel(
    val: Value,
    name: &str,
    is_polar: bool,
    percentage_ref: Option<f64>,
    update: UpdateComponents,
    span: Span,
) -> SassResult<Option<f64>> {
    // Handle `none` keyword for change
    if update == UpdateComponents::Change {
        if let Value::String(s, QuoteKind::None) = &val {
            if s == "none" {
                // Signal to set channel to None (missing)
                return Ok(None);
            }
        }
    }

    let num = val.assert_number_with_name(name, span)?;

    if update == UpdateComponents::Scale {
        // Scale requires percentage
        num.assert_unit(&Unit::Percent, name, span)?;
        num.assert_bounds(name, -100.0, 100.0, span)?;
        return Ok(Some(num.num.0 / 100.0));
    }

    if is_polar {
        // Hue channels accept deg, grad, rad, turn, or no unit
        // But NOT percentages
        if num.unit == Unit::Percent {
            return Err((
                format!(
                    "${}: Expected {} to have an angle unit (deg, grad, rad, turn).",
                    name,
                    num.num.inspect()
                ),
                span,
            )
                .into());
        }
        // Convert angle units to degrees
        let degrees = if num.unit == Unit::None || num.unit == Unit::Deg {
            num.num.0
        } else if num.unit == Unit::Grad {
            num.num.0 * 0.9
        } else if num.unit == Unit::Rad {
            num.num.0 * 180.0 / std::f64::consts::PI
        } else if num.unit == Unit::Turn {
            num.num.0 * 360.0
        } else {
            return Err((
                format!(
                    "${}: Expected {} to have an angle unit (deg, grad, rad, turn).",
                    name,
                    num.num.inspect()
                ),
                span,
            )
                .into());
        };
        Ok(Some(degrees))
    } else {
        // Non-polar channels accept unitless or percentage
        if num.unit == Unit::Percent {
            if let Some(pref) = percentage_ref {
                Ok(Some((num.num.0 / 100.0) * pref))
            } else {
                Err((
                    format!(
                        "${}: Expected {} to have no units or \"%\".",
                        name,
                        num.num.inspect()
                    ),
                    span,
                )
                    .into())
            }
        } else if num.unit == Unit::None {
            Ok(Some(num.num.0))
        } else {
            Err((
                format!(
                    "${}: Expected {} to have unit \"%\" or no units.",
                    name,
                    num.num.inspect()
                ),
                span,
            )
                .into())
        }
    }
}

/// Handle adjust/scale/change for a modern (non-legacy) working space.
fn update_modern(
    color: &Arc<Color>,
    args: &mut ArgumentResult,
    working_space: ColorSpace,
    convert_back: bool,
    update: UpdateComponents,
    span: Span,
) -> SassResult<Value> {
    let channel_defs = working_space.channels();

    // Convert color to working space
    let color_in_space = if color.color_space() != working_space {
        color.to_space(working_space)
    } else {
        color.as_ref().clone()
    };

    // Extract channel arguments for the working space
    // Each entry is Some(Some(value)) for a real value, Some(None) for `none`, or None for not provided
    let mut channel_adjustments: [Option<Option<f64>>; 3] = [None, None, None];

    for i in 0..3 {
        if let Some(v) = args.get(usize::MAX, channel_defs[i].name) {
            // Scale doesn't work on hue channels
            if update == UpdateComponents::Scale && channel_defs[i].is_polar {
                return Err((
                    "$hue: Cannot scale a polar channel (hue).".to_owned(),
                    span,
                )
                    .into());
            }

            let result = parse_modern_channel(
                v.node,
                channel_defs[i].name,
                channel_defs[i].is_polar,
                channel_defs[i].percentage_ref,
                update,
                span,
            )?;

            match result {
                Some(val) => channel_adjustments[i] = Some(Some(val)),
                None => channel_adjustments[i] = Some(None), // `none` keyword
            }
        }
    }

    // Extract alpha
    let alpha_adjustment = if let Some(v) = args.get(usize::MAX, "alpha") {
        let num = v.node.assert_number_with_name("alpha", span)?;
        if update == UpdateComponents::Scale {
            num.assert_unit(&Unit::Percent, "alpha", span)?;
            num.assert_bounds("alpha", -100.0, 100.0, span)?;
            Some(num.num.0 / 100.0)
        } else {
            Some(num.num.0)
        }
    } else {
        None
    };

    // Check for unknown named arguments
    if !args.named.is_empty() {
        let argument_names: Vec<String> = args
            .named
            .keys()
            .map(|key| format!("${key}"))
            .collect();

        let first_name = &argument_names[0];
        return Err((
            format!(
                "{}: Color space {} doesn't have a channel with this name.",
                first_name,
                working_space.name()
            ),
            span,
        )
            .into());
    }

    // Apply modifications to channels
    let mut new_channels = color_in_space.raw_channels();

    for i in 0..3 {
        if let Some(adj) = channel_adjustments[i] {
            match adj {
                None => {
                    // `none` keyword - set channel to missing
                    new_channels[i] = None;
                }
                Some(adj_val) => {
                    let current = new_channels[i].unwrap_or(0.0);
                    let new_val = match update {
                        UpdateComponents::Change => adj_val,
                        UpdateComponents::Adjust => {
                            let val = current + adj_val;
                            // dart-sass clamps lightness in perceptual spaces for adjust()
                            if working_space.is_perceptual()
                                && !channel_defs[i].is_polar
                                && i == 0
                            {
                                val.clamp(channel_defs[i].min, channel_defs[i].max)
                            } else {
                                val
                            }
                        }
                        UpdateComponents::Scale => {
                            let max = channel_defs[i].max;
                            let min = channel_defs[i].min;
                            current
                                + if adj_val > 0.0 {
                                    (max - current) * adj_val
                                } else {
                                    (current - min) * adj_val
                                }
                        }
                    };
                    new_channels[i] = Some(new_val);
                }
            }
        }
    }

    // Normalize negative chroma and hue to [0, 360)
    // This applies to LCH/OKLch spaces after change/adjust
    if let Some(hue_idx) = working_space.hue_channel_index() {
        // Chroma channel: Adjust clamps to 0; Change reflects (negate + rotate hue 180°)
        if working_space.is_perceptual() {
            let chroma_idx = 1; // Chroma is always channel 1 in LCH/OKLch
            if let Some(Some(chroma)) = new_channels.get(chroma_idx) {
                if *chroma < 0.0 && chroma.is_finite() {
                    if update == UpdateComponents::Adjust {
                        new_channels[chroma_idx] = Some(0.0);
                    } else {
                        // Change: chroma reflection
                        new_channels[chroma_idx] = Some(-chroma);
                        if let Some(hue) = new_channels[hue_idx] {
                            new_channels[hue_idx] = Some((hue + 180.0).rem_euclid(360.0));
                        }
                    }
                }
            }
        }

        // Normalize hue to [0, 360)
        if let Some(Some(hue)) = new_channels.get(hue_idx) {
            if hue.is_finite() {
                new_channels[hue_idx] = Some(hue.rem_euclid(360.0));
            }
        }
    }

    // Apply alpha modification
    let new_alpha = if let Some(adj) = alpha_adjustment {
        let current = color_in_space.alpha().0;
        Some(match update {
            UpdateComponents::Change => adj.clamp(0.0, 1.0),
            UpdateComponents::Adjust => (current + adj).clamp(0.0, 1.0),
            UpdateComponents::Scale => {
                let val = current
                    + if adj > 0.0 {
                        (1.0 - current) * adj
                    } else {
                        current * adj
                    };
                val.clamp(0.0, 1.0)
            }
        })
    } else {
        color_in_space.raw_alpha()
    };

    let result = Color::for_space(working_space, new_channels, new_alpha, ColorFormat::Infer);

    // Convert back to original space if $space was explicit
    let final_color = if convert_back && color.color_space() != working_space {
        result.to_space(color.color_space())
    } else {
        result
    };

    Ok(Value::Color(Arc::new(final_color)))
}

fn update_components(
    mut args: ArgumentResult,
    visitor: &mut Visitor,
    update: UpdateComponents,
) -> SassResult<Value> {
    let span = args.span();
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    // todo: what if color is also passed by name
    if args.positional.len() > 1 {
        return Err((
            "Only one positional argument is allowed. All other arguments must be passed by name.",
            span,
        )
            .into());
    }

    // Check for $space parameter
    let space_arg = args.get(usize::MAX, "space");

    // Determine if we should use the modern path
    if let Some(space_val) = space_arg {
        // Explicit $space parameter
        let space_str = match &space_val.node {
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

        let working_space = ColorSpace::from_name(&space_str).ok_or_else(|| {
            (
                format!("$space: Unknown color space \"{}\".", space_str),
                span,
            )
        })?;

        return update_modern(&color, &mut args, working_space, true, update, span);
    }

    // No $space parameter - check if color is in a modern space
    if !color.color_space().is_legacy() {
        return update_modern(&color, &mut args, color.color_space(), false, update, span);
    }

    // Legacy path: existing behavior for RGB/HSL/HWB colors

    // For change, `none` sets a channel to missing. Use Option<Option<Number>>:
    // None = not provided, Some(None) = `none`, Some(Some(val)) = numeric value
    type ChannelArg = Option<Option<Number>>;

    let check_num = |num: Spanned<Value>,
                     name: &str,
                     mut max: f64,
                     _assert_percent: bool,
                     _check_percent: bool|
     -> SassResult<Number> {
        let span = num.span;
        let mut num = num.node.assert_number_with_name(name, span)?;

        if update == UpdateComponents::Scale {
            max = 100.0;
            // Scale always requires percentage and bounds checking
            num.assert_unit(&Unit::Percent, name, span)?;
            num.assert_bounds(name, -max, max, span)?;
        }
        // For Change and Adjust, no bounds checking — out-of-range values are allowed

        // todo: hack to check if rgb channel
        if max == 100.0 {
            num.num /= Number(100.0);
        }

        Ok(num.num)
    };

    let get_arg = |args: &mut ArgumentResult,
                   name: &str,
                   max: f64,
                   assert_percent: bool,
                   check_percent: bool|
     -> SassResult<ChannelArg> {
        Ok(match args.get(usize::MAX, name) {
            Some(v) => {
                // Check for `none` keyword (only valid for Change)
                if update == UpdateComponents::Change {
                    if let Value::String(s, QuoteKind::None) = &v.node {
                        if s == "none" {
                            return Ok(Some(None)); // Set to missing
                        }
                    }
                }
                Some(Some(check_num(v, name, max, assert_percent, check_percent)?))
            }
            None => None,
        })
    };

    let red = get_arg(&mut args, "red", 255.0, false, false)?;
    let green = get_arg(&mut args, "green", 255.0, false, false)?;
    let blue = get_arg(&mut args, "blue", 255.0, false, false)?;

    // Alpha has bounds checking even for change/adjust (unlike channels)
    // Also supports `none` for Change
    let alpha: ChannelArg = if let Some(v) = args.get(usize::MAX, "alpha") {
        // Check for `none` keyword — set alpha to missing
        if update == UpdateComponents::Change {
            if let Value::String(s, QuoteKind::None) = &v.node {
                if s == "none" {
                    Some(None)
                } else {
                    let num = v.node.assert_number_with_name("alpha", span)?;
                    let min = 0.0;
                    if num.unit == Unit::Percent {
                        num.assert_bounds("alpha", min * 100.0, 100.0, span)?;
                        Some(Some(num.num / Number(100.0)))
                    } else {
                        num.assert_bounds_with_unit("alpha", min, 1.0, &Unit::None, span)?;
                        Some(Some(num.num))
                    }
                }
            } else {
                let num = v.node.assert_number_with_name("alpha", span)?;
                let min = 0.0;
                if num.unit == Unit::Percent {
                    num.assert_bounds("alpha", min * 100.0, 100.0, span)?;
                    Some(Some(num.num / Number(100.0)))
                } else {
                    num.assert_bounds_with_unit("alpha", min, 1.0, &Unit::None, span)?;
                    Some(Some(num.num))
                }
            }
        } else {
            let num = v.node.assert_number_with_name("alpha", span)?;
            if update == UpdateComponents::Scale {
                num.assert_unit(&Unit::Percent, "alpha", span)?;
                num.assert_bounds("alpha", -100.0, 100.0, span)?;
                Some(Some(num.num / Number(100.0)))
            } else {
                // Adjust: no bounds check on the argument itself; the result is clamped.
                // Percent unit is stripped (deprecated but accepted).
                Some(Some(num.num))
            }
        }
    } else {
        None
    };

    let hue: ChannelArg = if update == UpdateComponents::Scale {
        None
    } else if let Some(v) = args.get(usize::MAX, "hue") {
        // Check for `none` keyword
        if update == UpdateComponents::Change {
            if let Value::String(s, QuoteKind::None) = &v.node {
                if s == "none" {
                    Some(None)
                } else {
                    Some(Some(angle_value(v.node, "hue", v.span)?))
                }
            } else {
                Some(Some(angle_value(v.node, "hue", v.span)?))
            }
        } else {
            Some(Some(angle_value(v.node, "hue", v.span)?))
        }
    } else {
        None
    };

    let saturation = get_arg(&mut args, "saturation", 100.0, false, true)?;
    let lightness = get_arg(&mut args, "lightness", 100.0, false, true)?;
    let whiteness = get_arg(&mut args, "whiteness", 100.0, true, true)?;
    let blackness = get_arg(&mut args, "blackness", 100.0, true, true)?;

    if !args.named.is_empty() {
        let argument_word = if args.named.len() == 1 {
            "argument"
        } else {
            "arguments"
        };

        let argument_names = to_sentence(
            args.named
                .keys()
                .map(|key| format!("${key}", key = key))
                .collect(),
            "or",
        );

        return Err((
            format!(
                "No {argument_word} named {argument_names}.",
                argument_word = argument_word,
                argument_names = argument_names
            ),
            span,
        )
            .into());
    }

    let has_rgb = red.is_some() || green.is_some() || blue.is_some();
    let has_sl = saturation.is_some() || lightness.is_some();
    let has_wb = whiteness.is_some() || blackness.is_some();

    if has_rgb && (has_sl || has_wb || hue.is_some()) {
        let param_type = if has_wb { "HWB" } else { "HSL" };
        return Err((
            format!(
                "RGB parameters may not be passed along with {} parameters.",
                param_type
            ),
            span,
        )
            .into());
    }

    if has_sl && has_wb {
        return Err((
            "HSL parameters may not be passed along with HWB parameters.",
            span,
        )
            .into());
    }

    // Apply an update operation to a channel value.
    // ChannelArg: None=not provided, Some(None)=set to missing, Some(Some(val))=numeric
    fn apply_update(
        current: f64,
        param: &ChannelArg,
        max: f64,
        update: UpdateComponents,
    ) -> Option<f64> {
        match param {
            None => Some(current),          // Not provided, keep current
            Some(None) => None,             // `none` keyword, set to missing
            Some(Some(p)) => {
                let val = match update {
                    UpdateComponents::Change => p.0,
                    UpdateComponents::Adjust => p.0 + current,
                    UpdateComponents::Scale => {
                        current
                            + if p.0 > 0.0 {
                                (max - current) * p.0
                            } else {
                                current * p.0
                            }
                    }
                };
                Some(val)
            }
        }
    }

    let original_space = color.color_space();
    let original_format = color.format.clone();
    let color = if has_rgb {
        let clamp_rgb = update == UpdateComponents::Adjust;
        let new_r = apply_update(color.red().0, &red, 255.0, update)
            .map(|v| if clamp_rgb { v.clamp(0.0, 255.0) } else { v });
        let new_g = apply_update(color.green().0, &green, 255.0, update)
            .map(|v| if clamp_rgb { v.clamp(0.0, 255.0) } else { v });
        let new_b = apply_update(color.blue().0, &blue, 255.0, update)
            .map(|v| if clamp_rgb { v.clamp(0.0, 255.0) } else { v });
        let new_a = apply_update(color.alpha().0, &alpha, 1.0, update)
            .map(|v| v.clamp(0.0, 1.0));
        Arc::new(Color::for_space(
            ColorSpace::Rgb,
            [new_r, new_g, new_b],
            new_a,
            ColorFormat::Infer,
        ))
    } else if has_wb {
        // When the color is already in HWB space, use raw channel values to avoid
        // precision loss from HWB→RGB→whiteness/blackness roundtrip conversion.
        let (current_hue, current_w, current_b) = if color.color_space() == ColorSpace::Hwb {
            let ch = color.raw_channels();
            (
                Number(ch[0].unwrap_or(0.0)),
                Number(ch[1].unwrap_or(0.0)),
                Number(ch[2].unwrap_or(0.0)),
            )
        } else {
            (color.hue(), color.whiteness(), color.blackness())
        };
        let new_hue = match &hue {
            None => Some(current_hue.0),
            Some(None) => None, // none keyword
            Some(Some(h)) => {
                if update == UpdateComponents::Change {
                    Some(h.0)
                } else {
                    Some(current_hue.0 + h.0)
                }
            }
        };
        let new_w = apply_update(current_w.0, &whiteness, 1.0, update);
        let new_b = apply_update(current_b.0, &blackness, 1.0, update);
        let new_alpha = apply_update(color.alpha().0, &alpha, 1.0, update)
            .map(|v| v.clamp(0.0, 1.0));
        // Use Color::for_space to avoid from_hwb's normalization of out-of-range values
        let mut result = Color::for_space(
            ColorSpace::Hwb,
            [new_hue, new_w, new_b],
            new_alpha,
            ColorFormat::Infer,
        );
        // If original color was in a different space, convert back
        if original_space != ColorSpace::Hwb {
            result = result.to_space(original_space);
        }
        Arc::new(result)
    } else if hue.is_some() || has_sl {
        let (this_hue, this_saturation, this_lightness, this_alpha) = color.as_hsla();
        let new_hue = match &hue {
            None => Some(this_hue.0),
            Some(None) => None,
            Some(Some(h)) => {
                if update == UpdateComponents::Change {
                    Some(h.0)
                } else {
                    Some(this_hue.0 + h.0)
                }
            }
        };
        let mut new_sat = apply_update(this_saturation.0, &saturation, 1.0, update);
        let mut new_hue = new_hue;
        // For Adjust, clamp saturation to ≥0. For Change, reflect (negate + rotate hue 180°).
        if let Some(s) = new_sat {
            if s < 0.0 && s.is_finite() {
                if update == UpdateComponents::Adjust {
                    new_sat = Some(0.0);
                } else {
                    new_sat = Some(-s);
                    if let Some(h) = new_hue {
                        new_hue = Some((h + 180.0).rem_euclid(360.0));
                    }
                }
            }
        }
        let new_light = apply_update(this_lightness.0, &lightness, 1.0, update);
        let new_alpha = apply_update(this_alpha.0, &alpha, 1.0, update)
            .map(|v| v.clamp(0.0, 1.0));
        // Use Color::for_space to avoid from_hsla's clamping of out-of-range values
        let mut result = Color::for_space(
            ColorSpace::Hsl,
            [new_hue, new_sat, new_light],
            new_alpha,
            ColorFormat::Infer,
        );
        if original_space != ColorSpace::Hsl {
            result = result.to_space(original_space);
        } else {
            result.format = original_format.clone();
        }
        Arc::new(result)
    } else if alpha.is_some() {
        let new_alpha = apply_update(color.alpha().0, &alpha, 1.0, update)
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(1.0);
        Arc::new(color.with_alpha(Number(new_alpha)))
    } else {
        color
    };

    Ok(Value::Color(color))
}

pub(crate) fn scale_color(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    update_components(args, visitor, UpdateComponents::Scale)
}

pub(crate) fn change_color(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    update_components(args, visitor, UpdateComponents::Change)
}

pub(crate) fn adjust_color(args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    update_components(args, visitor, UpdateComponents::Adjust)
}

pub(crate) fn ie_hex_str(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;
    Ok(Value::String(color.to_ie_hex_str(), QuoteKind::None))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("change-color", Builtin::new(change_color));
    f.insert("adjust-color", Builtin::new(adjust_color));
    f.insert("scale-color", Builtin::new(scale_color));
    f.insert("ie-hex-str", Builtin::new(ie_hex_str));
}
