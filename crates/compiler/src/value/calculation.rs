use core::fmt;
use std::iter::Iterator;

use codemap::Span;

use crate::{
    common::BinaryOp,
    error::SassResult,
    serializer::inspect_number,
    unit::{Unit, UNIT_CONVERSION_TABLE},
    value::{conversion_factor, Number, SassNumber, Value},
    Options,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalculationArg {
    Number(SassNumber),
    Calculation(SassCalculation),
    String(String),
    Operation {
        lhs: Box<Self>,
        op: BinaryOp,
        rhs: Box<Self>,
    },
    Interpolation(String),
}

impl CalculationArg {
    pub fn parenthesize_calculation_rhs(outer: BinaryOp, right: BinaryOp) -> bool {
        if outer == BinaryOp::Div {
            true
        } else if outer == BinaryOp::Plus {
            false
        } else {
            right == BinaryOp::Plus || right == BinaryOp::Minus
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalculationName {
    Calc,
    Min,
    Max,
    Clamp,
    Abs,
    Acos,
    Asin,
    Atan,
    Atan2,
    Cos,
    Exp,
    Hypot,
    Log,
    Mod,
    Pow,
    Rem,
    Round,
    Sign,
    Sin,
    Sqrt,
    Tan,
}

impl fmt::Display for CalculationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalculationName::Calc => f.write_str("calc"),
            CalculationName::Min => f.write_str("min"),
            CalculationName::Max => f.write_str("max"),
            CalculationName::Clamp => f.write_str("clamp"),
            CalculationName::Abs => f.write_str("abs"),
            CalculationName::Acos => f.write_str("acos"),
            CalculationName::Asin => f.write_str("asin"),
            CalculationName::Atan => f.write_str("atan"),
            CalculationName::Atan2 => f.write_str("atan2"),
            CalculationName::Cos => f.write_str("cos"),
            CalculationName::Exp => f.write_str("exp"),
            CalculationName::Hypot => f.write_str("hypot"),
            CalculationName::Log => f.write_str("log"),
            CalculationName::Mod => f.write_str("mod"),
            CalculationName::Pow => f.write_str("pow"),
            CalculationName::Rem => f.write_str("rem"),
            CalculationName::Round => f.write_str("round"),
            CalculationName::Sign => f.write_str("sign"),
            CalculationName::Sin => f.write_str("sin"),
            CalculationName::Sqrt => f.write_str("sqrt"),
            CalculationName::Tan => f.write_str("tan"),
        }
    }
}

impl CalculationName {
    pub(crate) fn in_min_or_max(self) -> bool {
        self == CalculationName::Min || self == CalculationName::Max
    }

    /// Whether this calculation function can be overridden by a user-defined function
    pub(crate) fn is_overridable(self) -> bool {
        !matches!(self, CalculationName::Calc | CalculationName::Clamp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SassCalculation {
    pub name: CalculationName,
    pub args: Vec<CalculationArg>,
}

impl SassCalculation {
    pub fn unsimplified(name: CalculationName, args: Vec<CalculationArg>) -> Self {
        Self { name, args }
    }

    pub fn calc(arg: CalculationArg) -> Value {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(n) => Value::Dimension(n),
            CalculationArg::Calculation(c) => Value::Calculation(c),
            _ => Value::Calculation(SassCalculation {
                name: CalculationName::Calc,
                args: vec![arg],
            }),
        }
    }

    pub fn min(args: Vec<CalculationArg>, options: &Options, span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(!args.is_empty(), "min() must have at least one argument.");

        let mut minimum: Option<SassNumber> = None;

        for arg in &args {
            match arg {
                CalculationArg::Number(n)
                    if minimum.is_some() && !minimum.as_ref().unwrap().is_comparable_to(n) =>
                {
                    minimum = None;
                    break;
                }
                CalculationArg::Number(n)
                    if minimum.is_none()
                        || minimum.as_ref().unwrap().num
                            > n.num.convert(&n.unit, &minimum.as_ref().unwrap().unit) =>
                {
                    minimum = Some(n.clone());
                }
                CalculationArg::Number(..) => continue,
                _ => {
                    minimum = None;
                    break;
                }
            }
        }

        Ok(match minimum {
            Some(min) => Value::Dimension(min),
            None => {
                Self::verify_compatible_numbers(&args, options, span)?;

                Value::Calculation(SassCalculation {
                    name: CalculationName::Min,
                    args,
                })
            }
        })
    }

    pub fn max(args: Vec<CalculationArg>, options: &Options, span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        if args.is_empty() {
            return Err(("max() must have at least one argument.", span).into());
        }

        let mut maximum: Option<SassNumber> = None;

        for arg in &args {
            match arg {
                CalculationArg::Number(n)
                    if maximum.is_some() && !maximum.as_ref().unwrap().is_comparable_to(n) =>
                {
                    maximum = None;
                    break;
                }
                CalculationArg::Number(n)
                    if maximum.is_none()
                        || maximum.as_ref().unwrap().num
                            < n.num.convert(&n.unit, &maximum.as_ref().unwrap().unit) =>
                {
                    maximum = Some(n.clone());
                }
                CalculationArg::Number(..) => continue,
                _ => {
                    maximum = None;
                    break;
                }
            }
        }

        Ok(match maximum {
            Some(max) => Value::Dimension(max),
            None => {
                Self::verify_compatible_numbers(&args, options, span)?;

                Value::Calculation(SassCalculation {
                    name: CalculationName::Max,
                    args,
                })
            }
        })
    }

    pub fn clamp(
        min: CalculationArg,
        value: Option<CalculationArg>,
        max: Option<CalculationArg>,
        options: &Options,
        span: Span,
    ) -> SassResult<Value> {
        if value.is_none() && max.is_some() {
            return Err(("If value is null, max must also be null.", span).into());
        }

        let min = Self::simplify(min);
        let value = value.map(Self::simplify);
        let max = max.map(Self::simplify);

        match (min.clone(), value.clone(), max.clone()) {
            (
                CalculationArg::Number(min),
                Some(CalculationArg::Number(value)),
                Some(CalculationArg::Number(max)),
            ) => {
                if min.is_comparable_to(&value) && min.is_comparable_to(&max) {
                    if value.num <= min.num.convert(min.unit(), value.unit()) {
                        return Ok(Value::Dimension(min));
                    }

                    if value.num >= max.num.convert(max.unit(), value.unit()) {
                        return Ok(Value::Dimension(max));
                    }

                    return Ok(Value::Dimension(value));
                }
            }
            _ => {}
        }

        let mut args = vec![min];

        if let Some(value) = value {
            args.push(value);
        }

        if let Some(max) = max {
            args.push(max);
        }

        Self::verify_length(&args, 3, span)?;
        Self::verify_compatible_numbers(&args, options, span)?;

        Ok(Value::Calculation(SassCalculation {
            name: CalculationName::Clamp,
            args,
        }))
    }

    /// Whether a unit can be converted at compile time (is in the conversion table)
    /// Whether two units can be simplified in a multi-arg calculation
    /// Units that are in a known compatibility group (like %, vw, em, px) but
    /// can't actually be converted at compile time should NOT be simplified.
    /// Completely unknown/fake units that are equal CAN be simplified.
    fn can_simplify_units(a: &Unit, b: &Unit) -> bool {
        if *a == Unit::None && *b == Unit::None {
            return true;
        }
        // If both units have a known conversion factor, simplify
        if UNIT_CONVERSION_TABLE.contains_key(a) && conversion_factor(a, b).is_some() {
            return true;
        }
        // If both are the same unit and not %, simplify.
        // % is "possibly compatible" with other units at runtime, so it can't be simplified.
        if a == b && *a != Unit::Percent {
            return true;
        }
        false
    }

    fn coerce_to_rad(num: f64, unit: &Unit) -> f64 {
        if *unit == Unit::None {
            return num;
        }
        let factor = conversion_factor(unit, &Unit::Rad).unwrap();
        num * factor
    }

    fn is_angle_unit(unit: &Unit) -> bool {
        matches!(unit, Unit::Rad | Unit::Deg | Unit::Grad | Unit::Turn)
    }

    fn unsimplified_calc(name: CalculationName, args: Vec<CalculationArg>) -> Value {
        Value::Calculation(SassCalculation { name, args })
    }

    pub fn abs(arg: CalculationArg, options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) => {
                Ok(Value::Dimension(SassNumber {
                    num: n.num.abs(),
                    unit: n.unit.clone(),
                    as_slash: None,
                }))
            }
            _ => {
                Self::verify_compatible_numbers(std::slice::from_ref(&arg), options, span)?;
                Ok(Self::unsimplified_calc(CalculationName::Abs, vec![arg]))
            }
        }
    }

    pub fn exp(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if n.unit == Unit::None => {
                Ok(Value::Dimension(SassNumber::new_unitless(Number(n.num.0.exp()))))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have no units.",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Exp, vec![arg])),
        }
    }

    pub fn sign(arg: CalculationArg, options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if !n.unit.is_complex() => {
                let val = if n.num.0.is_nan() {
                    f64::NAN
                } else if n.num.0 == 0.0 {
                    // preserve sign of zero
                    n.num.0
                } else if n.num.0 > 0.0 {
                    1.0
                } else {
                    -1.0
                };
                Ok(Value::Dimension(SassNumber::new_unitless(Number(val))))
            }
            _ => {
                Self::verify_compatible_numbers(std::slice::from_ref(&arg), options, span)?;
                Ok(Self::unsimplified_calc(CalculationName::Sign, vec![arg]))
            }
        }
    }

    pub fn sin(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n)
                if n.unit == Unit::None || Self::is_angle_unit(&n.unit) =>
            {
                let rad = Self::coerce_to_rad(n.num.0, &n.unit);
                Ok(Value::Dimension(SassNumber::new_unitless(Number(rad.sin()))))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have an angle unit (deg, grad, rad, turn).",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Sin, vec![arg])),
        }
    }

    pub fn cos(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n)
                if n.unit == Unit::None || Self::is_angle_unit(&n.unit) =>
            {
                let rad = Self::coerce_to_rad(n.num.0, &n.unit);
                Ok(Value::Dimension(SassNumber::new_unitless(Number(rad.cos()))))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have an angle unit (deg, grad, rad, turn).",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Cos, vec![arg])),
        }
    }

    pub fn tan(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n)
                if n.unit == Unit::None || Self::is_angle_unit(&n.unit) =>
            {
                let rad = Self::coerce_to_rad(n.num.0, &n.unit);
                Ok(Value::Dimension(SassNumber::new_unitless(Number(rad.tan()))))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have an angle unit (deg, grad, rad, turn).",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Tan, vec![arg])),
        }
    }

    pub fn asin(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if n.unit == Unit::None => {
                let val = if n.num > Number(1.0) || n.num < Number(-1.0) {
                    f64::NAN
                } else if n.num.0 == 0.0 {
                    0.0
                } else {
                    n.num.0.asin().to_degrees()
                };
                Ok(Value::Dimension(SassNumber {
                    num: Number(val),
                    unit: Unit::Deg,
                    as_slash: None,
                }))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have no units.",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Asin, vec![arg])),
        }
    }

    pub fn acos(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if n.unit == Unit::None => {
                let val = if n.num > Number(1.0) || n.num < Number(-1.0) {
                    f64::NAN
                } else if n.num.0 == 1.0 {
                    0.0
                } else {
                    n.num.0.acos().to_degrees()
                };
                Ok(Value::Dimension(SassNumber {
                    num: Number(val),
                    unit: Unit::Deg,
                    as_slash: None,
                }))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have no units.",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Acos, vec![arg])),
        }
    }

    pub fn atan(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if n.unit == Unit::None => {
                let val = if n.num.0 == 0.0 {
                    0.0
                } else {
                    n.num.0.atan().to_degrees()
                };
                Ok(Value::Dimension(SassNumber {
                    num: Number(val),
                    unit: Unit::Deg,
                    as_slash: None,
                }))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have no units.",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Atan, vec![arg])),
        }
    }

    pub fn sqrt(arg: CalculationArg, _options: &Options, span: Span) -> SassResult<Value> {
        let arg = Self::simplify(arg);
        match arg {
            CalculationArg::Number(ref n) if n.unit == Unit::None => {
                Ok(Value::Dimension(SassNumber::new_unitless(n.num.sqrt())))
            }
            CalculationArg::Number(ref n) => {
                Err((
                    format!(
                        "Expected {} to have no units.",
                        Value::Dimension(n.clone()).inspect(span)?
                    ),
                    span,
                ).into())
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Sqrt, vec![arg])),
        }
    }

    // --- Multi-arg functions ---

    pub fn atan2(args: Vec<CalculationArg>, _options: &Options, _span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(args.len() == 2);

        match (&args[0], &args[1]) {
            (CalculationArg::Number(y), CalculationArg::Number(x)) => {
                let can_simplify = Self::can_simplify_units(&y.unit, &x.unit);

                if can_simplify {
                    let x_val = if y.unit != Unit::None && x.unit != Unit::None {
                        x.num.convert(&x.unit, &y.unit).0
                    } else {
                        x.num.0
                    };
                    return Ok(Value::Dimension(SassNumber {
                        num: Number(y.num.0.atan2(x_val).to_degrees()),
                        unit: Unit::Deg,
                        as_slash: None,
                    }));
                }
                Ok(Self::unsimplified_calc(CalculationName::Atan2, args))
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Atan2, args)),
        }
    }

    pub fn pow(args: Vec<CalculationArg>, _options: &Options, span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(args.len() == 2);

        match (&args[0], &args[1]) {
            (CalculationArg::Number(base), CalculationArg::Number(exp))
                if base.unit == Unit::None && exp.unit == Unit::None =>
            {
                Ok(Value::Dimension(SassNumber::new_unitless(
                    base.num.pow(exp.num),
                )))
            }
            (CalculationArg::Number(base), CalculationArg::Number(exp)) => {
                if base.unit != Unit::None {
                    Err((
                        format!(
                            "Expected {} to have no units.",
                            Value::Dimension(base.clone()).inspect(span)?
                        ),
                        span,
                    ).into())
                } else {
                    Err((
                        format!(
                            "Expected {} to have no units.",
                            Value::Dimension(exp.clone()).inspect(span)?
                        ),
                        span,
                    ).into())
                }
            }
            _ => Ok(Self::unsimplified_calc(CalculationName::Pow, args)),
        }
    }

    pub fn log(args: Vec<CalculationArg>, _options: &Options, span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(args.len() == 1 || args.len() == 2);

        if args.len() == 1 {
            match &args[0] {
                CalculationArg::Number(n) if n.unit == Unit::None => {
                    let val = if n.num.0 < 0.0 && !n.num.0.is_nan() {
                        f64::NAN
                    } else if n.num.0 == 0.0 {
                        f64::NEG_INFINITY
                    } else {
                        n.num.0.ln()
                    };
                    Ok(Value::Dimension(SassNumber::new_unitless(Number(val))))
                }
                CalculationArg::Number(n) => {
                    Err((
                        format!(
                            "Expected {} to have no units.",
                            Value::Dimension(n.clone()).inspect(span)?
                        ),
                        span,
                    ).into())
                }
                _ => Ok(Self::unsimplified_calc(CalculationName::Log, args)),
            }
        } else {
            match (&args[0], &args[1]) {
                (CalculationArg::Number(val), CalculationArg::Number(base))
                    if val.unit == Unit::None && base.unit == Unit::None =>
                {
                    let result = if base.num.0 == 0.0 {
                        Number::zero()
                    } else {
                        val.num.log(base.num)
                    };
                    Ok(Value::Dimension(SassNumber::new_unitless(result)))
                }
                (CalculationArg::Number(val), CalculationArg::Number(base)) => {
                    if val.unit != Unit::None {
                        Err((
                            format!(
                                "Expected {} to have no units.",
                                Value::Dimension(val.clone()).inspect(span)?
                            ),
                            span,
                        ).into())
                    } else {
                        Err((
                            format!(
                                "Expected {} to have no units.",
                                Value::Dimension(base.clone()).inspect(span)?
                            ),
                            span,
                        ).into())
                    }
                }
                _ => Ok(Self::unsimplified_calc(CalculationName::Log, args)),
            }
        }
    }

    pub fn hypot(args: Vec<CalculationArg>, options: &Options, span: Span) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);

        let first = match &args[0] {
            CalculationArg::Number(n) => n,
            _ => {
                Self::verify_compatible_numbers(&args, options, span)?;
                return Ok(Self::unsimplified_calc(CalculationName::Hypot, args));
            }
        };

        let first_unit = first.unit.clone();
        let mut sum = first.num.0 * first.num.0;
        let mut all_numbers = true;

        for arg in &args[1..] {
            match arg {
                CalculationArg::Number(n)
                    if Self::can_simplify_units(&n.unit, &first_unit) =>
                {
                    let converted = n.num.convert(&n.unit, &first_unit).0;
                    sum += converted * converted;
                }
                _ => {
                    all_numbers = false;
                    break;
                }
            }
        }

        if all_numbers {
            Ok(Value::Dimension(SassNumber {
                num: Number(sum.sqrt()),
                unit: first_unit,
                as_slash: None,
            }))
        } else {
            Self::verify_compatible_numbers(&args, options, span)?;
            Ok(Self::unsimplified_calc(CalculationName::Hypot, args))
        }
    }

    pub fn calc_mod(
        args: Vec<CalculationArg>,
        options: &Options,
        span: Span,
    ) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(args.len() == 2);

        match (&args[0], &args[1]) {
            (CalculationArg::Number(a), CalculationArg::Number(b))
                if a.unit.comparable(&b.unit) =>
            {
                let b_converted = b.num.convert(&b.unit, &a.unit).0;
                // CSS mod: result sign matches divisor
                // a mod b = a - b * floor(a / b)
                let result = if b_converted == 0.0 {
                    f64::NAN
                } else {
                    a.num.0 - b_converted * (a.num.0 / b_converted).floor()
                };
                Ok(Value::Dimension(SassNumber {
                    num: Number(result),
                    unit: a.unit.clone(),
                    as_slash: None,
                }))
            }
            _ => {
                Self::verify_compatible_numbers(&args, options, span)?;
                Ok(Self::unsimplified_calc(CalculationName::Mod, args))
            }
        }
    }

    pub fn calc_rem(
        args: Vec<CalculationArg>,
        options: &Options,
        span: Span,
    ) -> SassResult<Value> {
        let args = Self::simplify_arguments(args);
        debug_assert!(args.len() == 2);

        match (&args[0], &args[1]) {
            (CalculationArg::Number(a), CalculationArg::Number(b))
                if a.unit.comparable(&b.unit) =>
            {
                let b_converted = b.num.convert(&b.unit, &a.unit).0;
                // CSS rem: result sign matches dividend (IEEE remainder)
                let result = if b_converted == 0.0 {
                    f64::NAN
                } else if b_converted.is_infinite() {
                    if a.num.0.is_finite() {
                        a.num.0
                    } else {
                        f64::NAN
                    }
                } else {
                    a.num.0 % b_converted
                };
                Ok(Value::Dimension(SassNumber {
                    num: Number(result),
                    unit: a.unit.clone(),
                    as_slash: None,
                }))
            }
            _ => {
                Self::verify_compatible_numbers(&args, options, span)?;
                Ok(Self::unsimplified_calc(CalculationName::Rem, args))
            }
        }
    }

    pub fn round(
        args: Vec<CalculationArg>,
        strategy: Option<String>,
        options: &Options,
        span: Span,
    ) -> SassResult<Value> {
        let strategy_str = strategy.as_deref().unwrap_or("nearest");

        if args.len() == 1 && strategy.is_none() {
            // round(number) — single arg, nearest integer
            let arg = Self::simplify(args.into_iter().next().unwrap());
            match arg {
                CalculationArg::Number(ref n) => {
                    return Ok(Value::Dimension(SassNumber {
                        num: n.num.round(),
                        unit: n.unit.clone(),
                        as_slash: None,
                    }));
                }
                _ => {
                    return Ok(Self::unsimplified_calc(
                        CalculationName::Round,
                        vec![arg],
                    ));
                }
            }
        }

        let args = Self::simplify_arguments(args);

        if args.len() == 1 {
            // round(strategy, number) — single number with strategy
            match &args[0] {
                CalculationArg::Number(n) => {
                    let result = Self::round_with_step(n.num.0, 1.0, strategy_str);
                    return Ok(Value::Dimension(SassNumber {
                        num: Number(result),
                        unit: n.unit.clone(),
                        as_slash: None,
                    }));
                }
                _ => {}
            }
        }

        if args.len() == 2 {
            match (&args[0], &args[1]) {
                (CalculationArg::Number(number), CalculationArg::Number(step))
                    if number.unit.comparable(&step.unit) =>
                {
                    let step_converted = step.num.convert(&step.unit, &number.unit).0;
                    let result =
                        Self::round_with_step(number.num.0, step_converted, strategy_str);
                    return Ok(Value::Dimension(SassNumber {
                        num: Number(result),
                        unit: number.unit.clone(),
                        as_slash: None,
                    }));
                }
                _ => {}
            }
        }

        Self::verify_compatible_numbers(&args, options, span)?;

        let mut full_args = Vec::with_capacity(args.len() + 1);
        if let Some(s) = strategy {
            full_args.push(CalculationArg::String(s));
        }
        full_args.extend(args);
        Ok(Self::unsimplified_calc(CalculationName::Round, full_args))
    }

    fn round_with_step(number: f64, step: f64, strategy: &str) -> f64 {
        if number.is_nan() || step.is_nan() || step == 0.0 {
            return f64::NAN;
        }
        if number.is_infinite() {
            // round(strategy, ±infinity, finite) = ±infinity
            // round(strategy, ±infinity, ±infinity) = NaN
            return if step.is_infinite() {
                f64::NAN
            } else {
                number
            };
        }
        if step.is_infinite() {
            // Rounding a finite number to a multiple of infinity.
            // The multiples of infinity are: ..., -infinity, 0, infinity, ...
            // So the "nearest" and "to-zero" result is 0 with appropriate sign.
            return match strategy {
                "nearest" | "to-zero" => 0.0_f64.copysign(number),
                "up" => {
                    if number > 0.0 {
                        f64::INFINITY
                    } else {
                        0.0_f64.copysign(number)
                    }
                }
                "down" => {
                    if number < 0.0 {
                        f64::NEG_INFINITY
                    } else {
                        0.0_f64.copysign(number)
                    }
                }
                _ => f64::NAN,
            };
        }

        // CSS spec: negative step generally acts as abs(step), but for
        // to-zero it reverses direction (toward zero becomes away from zero)
        let negative_step = step < 0.0;
        let step = step.abs();
        let div = number / step;
        match strategy {
            "nearest" => div.round() * step,
            "up" => div.ceil() * step,
            "down" => div.floor() * step,
            "to-zero" => {
                if negative_step {
                    // Away from zero: ceil for positive, floor for negative
                    if div > 0.0 {
                        div.ceil() * step
                    } else {
                        div.floor() * step
                    }
                } else {
                    div.trunc() * step
                }
            }
            _ => f64::NAN,
        }
    }

    fn verify_length(args: &[CalculationArg], len: usize, span: Span) -> SassResult<()> {
        if args.len() == len {
            return Ok(());
        }

        if args.iter().any(|arg| {
            matches!(
                arg,
                CalculationArg::String(..) | CalculationArg::Interpolation(..)
            )
        }) {
            return Ok(());
        }

        let was_or_were = if args.len() == 1 { "was" } else { "were" };

        Err((
            format!(
                "{len} arguments required, but only {} {was_or_were} passed.",
                args.len(),
                len = len,
                was_or_were = was_or_were,
            ),
            span,
        )
            .into())
    }

    #[allow(clippy::needless_range_loop)]
    fn verify_compatible_numbers(
        args: &[CalculationArg],
        options: &Options,
        span: Span,
    ) -> SassResult<()> {
        for arg in args {
            match arg {
                CalculationArg::Number(num) => match &num.unit {
                    Unit::Complex(complex) => {
                        if complex.numer.len() > 1 || !complex.denom.is_empty() {
                            let num = num.clone();
                            let value = Value::Dimension(num);
                            return Err((
                                format!(
                                    "Number {} isn't compatible with CSS calculations.",
                                    value.inspect(span)?
                                ),
                                span,
                            )
                                .into());
                        }
                    }
                    _ => continue,
                },
                _ => continue,
            }
        }

        for i in 0..args.len() {
            let number1 = match &args[i] {
                CalculationArg::Number(num) => num,
                _ => continue,
            };

            for j in (i + 1)..args.len() {
                let number2 = match &args[j] {
                    CalculationArg::Number(num) => num,
                    _ => continue,
                };

                if number1.has_possibly_compatible_units(number2) {
                    continue;
                }

                return Err((
                    format!(
                        "{} and {} are incompatible.",
                        inspect_number(number1, options, span)?,
                        inspect_number(number2, options, span)?
                    ),
                    span,
                )
                    .into());
            }
        }

        Ok(())
    }

    pub fn operate_internal(
        mut op: BinaryOp,
        left: CalculationArg,
        right: CalculationArg,
        in_min_or_max: bool,
        simplify: bool,
        options: &Options,
        span: Span,
    ) -> SassResult<CalculationArg> {
        if !simplify {
            return Ok(CalculationArg::Operation {
                lhs: Box::new(left),
                op,
                rhs: Box::new(right),
            });
        }

        let left = Self::simplify(left);
        let mut right = Self::simplify(right);

        if op == BinaryOp::Plus || op == BinaryOp::Minus {
            match (&left, &right) {
                (CalculationArg::Number(left), CalculationArg::Number(right))
                    if if in_min_or_max {
                        left.is_comparable_to(right)
                    } else {
                        left.has_compatible_units(&right.unit)
                    } =>
                {
                    if op == BinaryOp::Plus {
                        return Ok(CalculationArg::Number(left.clone() + right.clone()));
                    } else {
                        return Ok(CalculationArg::Number(left.clone() - right.clone()));
                    }
                }
                _ => {}
            }

            Self::verify_compatible_numbers(&[left.clone(), right.clone()], options, span)?;

            if let CalculationArg::Number(mut n) = right {
                if n.num.is_negative() {
                    n.num.0 *= -1.0;
                    op = if op == BinaryOp::Plus {
                        BinaryOp::Minus
                    } else {
                        BinaryOp::Plus
                    }
                } else {
                    // todo: do we need this branch?
                }
                right = CalculationArg::Number(n);
            }

            return Ok(CalculationArg::Operation {
                lhs: Box::new(left),
                op,
                rhs: Box::new(right),
            });
        }

        match (left, right) {
            (CalculationArg::Number(num1), CalculationArg::Number(num2)) => {
                if op == BinaryOp::Mul {
                    Ok(CalculationArg::Number(num1 * num2))
                } else {
                    Ok(CalculationArg::Number(num1 / num2))
                }
            }
            (left, right) => Ok(CalculationArg::Operation {
                lhs: Box::new(left),
                op,
                rhs: Box::new(right),
            }),
        }

        //   _verifyCompatibleNumbers([left, right]);

        // Ok(CalculationArg::Operation {
        //     lhs: Box::new(left),
        //     op,
        //     rhs: Box::new(right),
        // })
    }

    fn simplify(arg: CalculationArg) -> CalculationArg {
        match arg {
            CalculationArg::Number(..)
            | CalculationArg::Operation { .. }
            | CalculationArg::Interpolation(..)
            | CalculationArg::String(..) => arg,
            CalculationArg::Calculation(mut calc) => {
                if calc.name == CalculationName::Calc {
                    calc.args.remove(0)
                } else {
                    CalculationArg::Calculation(calc)
                }
            }
        }
    }

    fn simplify_arguments(args: Vec<CalculationArg>) -> Vec<CalculationArg> {
        args.into_iter().map(Self::simplify).collect()
    }
}
