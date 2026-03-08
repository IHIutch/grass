use crate::builtin::builtin_imports::*;
use crate::color::space::ColorSpace;

use super::{
    angle_value,
    rgb::{parse_channels, percentage_or_unitless},
    GlobalFunctionMap, ParsedChannels,
};

pub(crate) fn blackness(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.blackness() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    Ok(Value::Dimension(SassNumber {
        num: color.blackness() * 100,
        unit: Unit::Percent,
        as_slash: None,
    }))
}

pub(crate) fn whiteness(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;

    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "color.whiteness() is only supported for legacy colors. Please use color.channel() instead.",
            args.span(),
        ).into());
    }

    Ok(Value::Dimension(SassNumber {
        num: color.whiteness() * 100,
        unit: Unit::Percent,
        as_slash: None,
    }))
}

fn hwb_inner(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    let span = args.span();

    let hue = angle_value(args.get_err(0, "hue")?, "hue", args.span())?;

    let whiteness = args
        .get_err(1, "whiteness")?
        .assert_number_with_name("whiteness", span)?;
    whiteness.assert_unit(&Unit::Percent, "whiteness", span)?;

    let blackness = args
        .get_err(2, "blackness")?
        .assert_number_with_name("blackness", span)?;
    blackness.assert_unit(&Unit::Percent, "blackness", span)?;

    let alpha = args
        .default_arg(3, "alpha", Value::Dimension(SassNumber::new_unitless(1.0)))
        .assert_number_with_name("alpha", args.span())?;

    let alpha = percentage_or_unitless(&alpha, 1.0, "alpha", args.span(), visitor)?;

    Ok(Value::Color(Arc::new(Color::from_hwb(
        hue,
        whiteness.num,
        blackness.num,
        Number(alpha),
    ))))
}

pub(crate) fn hwb(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(4)?;

    if args.len() == 0 || args.len() == 1 {
        let span = args.span();
        match parse_channels(
            "hwb",
            &["hue", "whiteness", "blackness"],
            args.get_err(0, "channels")?,
            visitor,
            span,
        )? {
            ParsedChannels::String(s) => Err((
                format!("Expected numeric channels, got \"{}\".", s),
                span,
            )
                .into()),
            ParsedChannels::List(list) => {
                // Check if any channel is `none` — if so, use modern Color 4 path
                let has_none = list.iter().any(|v| matches!(v, Value::String(s, QuoteKind::None) if s == "none"));
                if has_none {
                    let has_alpha = list.len() > 3;
                    return super::css_color4::construct_color(ColorSpace::Hwb, &list, has_alpha, span, visitor);
                }
                let args = ArgumentResult {
                    positional: list,
                    named: BTreeMap::new(),
                    separator: ListSeparator::Comma,
                    span,
                    touched: BTreeSet::new(),
                };

                hwb_inner(args, visitor)
            }
        }
    } else if args.len() == 3 || args.len() == 4 {
        hwb_inner(args, visitor)
    } else {
        args.max_args(1)?;
        unreachable!()
    }
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("hwb", Builtin::new(hwb));
    f.insert("whiteness", Builtin::new(whiteness));
    f.insert("blackness", Builtin::new(blackness));
}
