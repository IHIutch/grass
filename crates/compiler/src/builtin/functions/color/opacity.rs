use crate::builtin::builtin_imports::*;

/// Check if `s` matches the regex `^[a-zA-Z]+\s*=`
fn is_ms_filter(s: &str) -> bool {
    let mut bytes = s.bytes();

    if !bytes.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    bytes
        .skip_while(u8::is_ascii_alphabetic)
        .find(|c| !matches!(c, b' ' | b'\t' | b'\n'))
        == Some(b'=')
}

#[cfg(test)]
mod test {
    use super::is_ms_filter;
    #[test]
    fn test_is_ms_filter() {
        assert!(is_ms_filter("a=a"));
        assert!(is_ms_filter("a="));
        assert!(is_ms_filter("a  \t\n  =a"));
        assert!(!is_ms_filter("a  \t\n  a=a"));
        assert!(!is_ms_filter("aa"));
        assert!(!is_ms_filter("   aa"));
        assert!(!is_ms_filter("=a"));
        assert!(!is_ms_filter("1=a"));
    }
}

pub(crate) fn alpha(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    if args.len() <= 1 {
        let color = args.get_err(0, "color")?;

        if let Value::String(s, QuoteKind::None) = &color {
            if is_ms_filter(s) {
                return Ok(Value::String(format!("alpha({})", s).into(), QuoteKind::None));
            }
        }

        let color = color.assert_color_with_name("color", args.span())?;

        if !color.color_space().is_legacy() {
            return Err((
                "color.alpha() is only supported for legacy colors. Please use color.channel() instead.",
                args.span(),
            ).into());
        }

        Ok(Value::Dimension(SassNumber::new_unitless(color.alpha())))
    } else {
        let err = args.max_args(1);
        let args = args
            .get_variadic()?
            .into_iter()
            .map(|arg| match arg.node {
                Value::String(s, QuoteKind::None) if is_ms_filter(&s) => Ok(s),
                _ => {
                    err.clone()?;
                    unreachable!()
                }
            })
            .collect::<SassResult<Vec<_>>>()?;

        Ok(Value::String(
            format!("alpha({})", args.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")).into(),
            QuoteKind::None,
        ))
    }
}

pub(crate) fn opacity(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();
    match args.get_err(0, "color")? {
        Value::Color(c) => Ok(Value::Dimension(SassNumber::new_unitless(c.alpha()))),
        Value::Dimension(SassNumber {
            num,
            unit,
            as_slash: _,
        }) => Ok(Value::String(
            format!("opacity({}{})", num.inspect(), unit).into(),
            QuoteKind::None,
        )),
        v if v.is_special_function() => Ok(Value::String(
            format!("opacity({})", v.to_css_string(span, visitor.options.is_compressed())?).into(),
            QuoteKind::None,
        )),
        v => Err((
            format!("$color: {} is not a color.", v.inspect(span)?),
            span,
        )
            .into()),
    }
}

fn opacify(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "opacify() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds_with_unit("amount", 0.0, 1.0, &Unit::None, args.span())?;

    Ok(Value::Color(Arc::new(color.fade_in(amount.num))))
}

fn transparentize(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let color = args
        .get_err(0, "color")?
        .assert_color_with_name("color", args.span())?;

    if !color.color_space().is_legacy() {
        return Err((
            "transparentize() is only supported for legacy colors. Please use color.adjust() instead with an explicit $space argument.",
            args.span(),
        ).into());
    }

    let amount = args
        .get_err(1, "amount")?
        .assert_number_with_name("amount", args.span())?;

    amount.assert_bounds_with_unit("amount", 0.0, 1.0, &Unit::None, args.span())?;

    Ok(Value::Color(Arc::new(color.fade_out(amount.num))))
}

/// Module-level `color.opacity()` — allows color and number passthrough,
/// but NOT special function passthrough (var(), calc(), etc.).
/// `color.opacity(var(--c))` errors, while `color.opacity(0.5)` passes through.
pub(crate) fn module_opacity(mut args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let span = args.span();
    match args.get_err(0, "color")? {
        Value::Color(c) => Ok(Value::Dimension(SassNumber::new_unitless(c.alpha()))),
        Value::Dimension(SassNumber {
            num,
            unit,
            as_slash: _,
        }) => Ok(Value::String(
            format!("opacity({}{})", num.inspect(), unit).into(),
            QuoteKind::None,
        )),
        v => Err((
            format!("$color: {} is not a color.", v.inspect(span)?),
            span,
        )
            .into()),
    }
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("alpha", Builtin::new(alpha));
    f.insert("opacity", Builtin::new(opacity));
    f.insert("opacify", Builtin::new(opacify));
    f.insert("fade-in", Builtin::new(opacify));
    f.insert("transparentize", Builtin::new(transparentize));
    f.insert("fade-out", Builtin::new(transparentize));
}
