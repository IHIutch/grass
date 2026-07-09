//! Send-safe intermediate representation for todo #221 slice 3's async
//! `functions` bridge.
//!
//! `Task::compute()` runs off the JS thread with no `Env` at all, but grass's
//! internal `Value` (`grass_compiler::sass_value::Value`) is `Rc`-based and
//! therefore `!Send` — it cannot cross the worker-thread -> JS-thread ->
//! worker-thread round trip a `ThreadsafeFunction` call requires. `WireValue`
//! mirrors the exact same supported-type subset as `values.rs`'s marshalling
//! table (booleans, null, numbers, strings, lists — the same set
//! `sass_value_to_js`/`js_value_to_sass` accept), but as owned, `Send`,
//! `Env`-free data, so it can live inside the channel payload passed to
//! [`napi::threadsafe_function::ThreadsafeFunction::call`].
//!
//! The actual JS-value boundary conversion still happens on the JS thread,
//! via `values::sass_value_to_js`/`js_value_to_sass` — `WireValue` only
//! exists for the two extra hops on either side of that
//! (`Value -> WireValue` on the worker thread before crossing over,
//! `WireValue -> Value` on the worker thread after crossing back; on the JS
//! thread the code goes `WireValue -> Value -> JsUnknown` and
//! `JsUnknown -> Value -> WireValue`, reusing `values.rs` unchanged for the
//! middle step).

use grass_compiler::sass_value as sass;

use crate::values::{js_units_to_unit, list_separator_from_str, list_separator_to_str, unit_to_js_units};

#[derive(Debug, Clone)]
pub enum WireValue {
    True,
    False,
    Null,
    Number {
        value: f64,
        numerator_units: Vec<String>,
        denominator_units: Vec<String>,
    },
    Str {
        text: String,
        has_quotes: bool,
    },
    List {
        items: Vec<WireValue>,
        separator: String,
        brackets: bool,
    },
}

/// Converts a grass-internal `Value` into `WireValue`. Errors with the
/// unsupported type's bare name for the same set `values::sass_value_to_js`
/// rejects (`SassColor`/`SassMap`/`SassCalculation`/`SassFunction`/
/// `SassMixin`) — callers are responsible for formatting that name into a
/// full error message (kept as a plain `&'static str` here since this module
/// has no `Env`/`napi::Error` to build one with).
pub fn value_to_wire(value: &sass::Value) -> Result<WireValue, &'static str> {
    match value {
        sass::Value::True => Ok(WireValue::True),
        sass::Value::False => Ok(WireValue::False),
        sass::Value::Null => Ok(WireValue::Null),
        sass::Value::Dimension(n) => {
            let (numerator_units, denominator_units) = unit_to_js_units(&n.unit);
            Ok(WireValue::Number {
                value: n.num.0,
                numerator_units,
                denominator_units,
            })
        }
        sass::Value::String(s, quote_kind) => Ok(WireValue::Str {
            text: s.to_string(),
            has_quotes: matches!(quote_kind, sass::QuoteKind::Quoted),
        }),
        sass::Value::List(items, sep, brackets) => Ok(WireValue::List {
            items: items.iter().map(value_to_wire).collect::<Result<_, _>>()?,
            separator: list_separator_to_str(*sep).to_owned(),
            brackets: matches!(brackets, sass::Brackets::Bracketed),
        }),
        sass::Value::ArgList(arglist) => Ok(WireValue::List {
            items: arglist
                .elems
                .iter()
                .map(value_to_wire)
                .collect::<Result<_, _>>()?,
            separator: list_separator_to_str(arglist.separator).to_owned(),
            brackets: false,
        }),
        sass::Value::Color(_) => Err("SassColor"),
        sass::Value::Map(_) => Err("SassMap"),
        sass::Value::Calculation(_) => Err("SassCalculation"),
        sass::Value::FunctionRef(_) => Err("SassFunction"),
        // `MixinRef`'s field type isn't part of grass_compiler's public API
        // surface, so a wildcard arm covers it (matches values.rs's pattern).
        _ => Err("SassMixin"),
    }
}

/// The inverse of [`value_to_wire`] — always succeeds, since `WireValue`
/// only ever holds shapes `value_to_wire` accepted in the first place.
pub fn wire_to_sass(wire: WireValue) -> sass::Value {
    match wire {
        WireValue::True => sass::Value::True,
        WireValue::False => sass::Value::False,
        WireValue::Null => sass::Value::Null,
        WireValue::Number {
            value,
            numerator_units,
            denominator_units,
        } => sass::Value::Dimension(sass::SassNumber {
            num: sass::Number(value),
            unit: js_units_to_unit(numerator_units, denominator_units),
            as_slash: None,
        }),
        WireValue::Str { text, has_quotes } => sass::Value::String(
            text.into(),
            if has_quotes {
                sass::QuoteKind::Quoted
            } else {
                sass::QuoteKind::None
            },
        ),
        WireValue::List {
            items,
            separator,
            brackets,
        } => sass::Value::List(
            std::rc::Rc::new(items.into_iter().map(wire_to_sass).collect()),
            list_separator_from_str(&separator),
            if brackets {
                sass::Brackets::Bracketed
            } else {
                sass::Brackets::None
            },
        ),
    }
}
