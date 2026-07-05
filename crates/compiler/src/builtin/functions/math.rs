use crate::{builtin::builtin_imports::*, evaluate::div};

pub(crate) fn percentage(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let num = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span)?;
    num.assert_no_units("number", args.span)?;

    Ok(Value::Dimension(SassNumber {
        num: Number(num.num.0 * 100.0),
        unit: Unit::Percent,
        as_slash: None,
    }))
}

pub(crate) fn round(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let mut number = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    if !number.num.is_finite() {
        return Err(("Infinity or NaN toInt", args.span()).into());
    }

    number.num = number.num.round();

    Ok(Value::Dimension(number))
}

pub(crate) fn ceil(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let mut number = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    if !number.num.is_finite() {
        return Err(("Infinity or NaN toInt", args.span()).into());
    }

    number.num = number.num.ceil();

    Ok(Value::Dimension(number))
}

pub(crate) fn floor(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let mut number = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    if !number.num.is_finite() {
        return Err(("Infinity or NaN toInt", args.span()).into());
    }

    number.num = number.num.floor();

    Ok(Value::Dimension(number))
}

pub(crate) fn abs(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let mut num = args
        .get_err(0, "number")?
        .assert_number_with_name("number", args.span())?;

    num.num = num.num.abs();

    Ok(Value::Dimension(num))
}

pub(crate) fn comparable(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;
    let unit1 = args
        .get_err(0, "number1")?
        .assert_number_with_name("number1", args.span())?
        .unit;

    let unit2 = args
        .get_err(1, "number2")?
        .assert_number_with_name("number2", args.span())?
        .unit;

    Ok(Value::bool(unit1.comparable(&unit2)))
}

#[cfg(feature = "random")]
pub(crate) fn random(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    let limit = args.default_arg(0, "limit", Value::Null);

    if matches!(limit, Value::Null) {
        let mut rng = rand::thread_rng();
        return Ok(Value::Dimension(SassNumber::new_unitless(
            rng.gen_range(0.0..1.0),
        )));
    }

    let limit = limit.assert_number_with_name("limit", args.span())?;
    let limit_int = limit.assert_int_with_name("limit", args.span())?;
    let limit = limit.num;

    if limit.is_one() {
        return Ok(Value::Dimension(SassNumber::new_unitless(1.0)));
    }

    if limit.is_zero() || limit.is_negative() {
        return Err((
            format!("$limit: Must be greater than 0, was {}.", limit.inspect()),
            args.span(),
        )
            .into());
    }

    let mut rng = rand::thread_rng();
    Ok(Value::Dimension(SassNumber::new_unitless(
        rng.gen_range(0..limit_int) + 1,
    )))
}

/// Compares `lhs` to `rhs` the same way `evaluate::cmp` compares two
/// `Value::Dimension`s, without constructing either as a `Value`. Mirrors
/// the ordering (and the exact "Incompatible units" error text/argument
/// order) of `Value::cmp`'s `Dimension` arm, where `lhs` plays the role of
/// `self` and `rhs` plays the role of `other`.
fn cmp_dimension(
    lhs: (&Number, &Unit),
    rhs: (&Number, &Unit),
    span: Span,
) -> SassResult<Option<Ordering>> {
    let (num, unit) = lhs;
    let (num2, unit2) = rhs;

    if !unit.comparable(unit2) {
        return Err((format!("Incompatible units {} and {}.", unit2, unit), span).into());
    }

    Ok(if unit == unit2 || unit == &Unit::None || unit2 == &Unit::None {
        num.partial_cmp(num2)
    } else {
        num.partial_cmp(&num2.convert(unit2, unit))
    })
}

pub(crate) fn min(args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
    args.min_args(1)?;
    let span = args.span();
    let mut nums = args
        .get_variadic()?
        .into_iter()
        .map(|val| match val.node {
            Value::Dimension(SassNumber {
                num: number,
                unit,
                as_slash: _,
            }) => Ok((number, unit)),
            v => Err((format!("{} is not a number.", v.inspect(span)?), span).into()),
        })
        .collect::<SassResult<Vec<(Number, Unit)>>>()?
        .into_iter();

    let mut min = match nums.next() {
        Some((n, u)) => (n, u),
        None => unreachable!(),
    };

    for (num, unit) in nums {
        if matches!(
            cmp_dimension((&num, &unit), (&min.0, &min.1), span)?,
            Some(Ordering::Less)
        ) {
            min = (num, unit);
        }
    }
    Ok(Value::Dimension(SassNumber {
        num: (min.0),
        unit: min.1,
        as_slash: None,
    }))
}

pub(crate) fn max(args: ArgumentResult, _visitor: &mut Visitor) -> SassResult<Value> {
    args.min_args(1)?;
    let span = args.span();
    let mut nums = args
        .get_variadic()?
        .into_iter()
        .map(|val| match val.node {
            Value::Dimension(SassNumber {
                num: number,
                unit,
                as_slash: _,
            }) => Ok((number, unit)),
            v => Err((format!("{} is not a number.", v.inspect(span)?), span).into()),
        })
        .collect::<SassResult<Vec<(Number, Unit)>>>()?
        .into_iter();

    let mut max = match nums.next() {
        Some((n, u)) => (n, u),
        None => unreachable!(),
    };

    for (num, unit) in nums {
        if matches!(
            cmp_dimension((&num, &unit), (&max.0, &max.1), span)?,
            Some(Ordering::Greater)
        ) {
            max = (num, unit);
        }
    }
    Ok(Value::Dimension(SassNumber {
        num: (max.0),
        unit: max.1,
        as_slash: None,
    }))
}

pub(crate) fn divide(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
    args.max_args(2)?;

    let number1 = args.get_err(0, "number1")?;
    let number2 = args.get_err(1, "number2")?;

    div(number1, number2, visitor.options, args.span())
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("percentage", Builtin::new(percentage).with_deprecated_global("math", "percentage"));
    f.insert("round", Builtin::new(round).with_deprecated_global("math", "round"));
    f.insert("ceil", Builtin::new(ceil).with_deprecated_global("math", "ceil"));
    f.insert("floor", Builtin::new(floor).with_deprecated_global("math", "floor"));
    f.insert("abs", Builtin::new(abs).with_deprecated_global("math", "abs"));
    f.insert("min", Builtin::new(min).with_deprecated_global("math", "min"));
    f.insert("max", Builtin::new(max).with_deprecated_global("math", "max"));
    f.insert(
        "comparable",
        Builtin::new(comparable).with_deprecated_global("math", "compatible"),
    );
    #[cfg(feature = "random")]
    f.insert("random", Builtin::new(random).with_deprecated_global("math", "random"));
}
