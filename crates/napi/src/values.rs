//! Value marshalling between grass's internal `Value` (`grass_compiler::sass_value::Value`)
//! and the JS-facing types handed to/returned from `functions` callbacks (todo #221 slice 2).
//!
//! Coverage table (see the module's doc on `functions.rs` for the calling
//! convention this feeds):
//!
//! | grass `Value` variant | JS shape | Notes |
//! |---|---|---|
//! | `True`/`False` | plain JS `true`/`false` | Simplified vs. the real `sass` npm package (which uses `SassBoolean` singletons) — unambiguous and more ergonomic, documented divergence. |
//! | `Null` | plain JS `null`/`undefined` (either accepted on the way in) | Same simplification as booleans. |
//! | `Dimension` | `SassNumber` class instance (`value`, `numeratorUnits`, `denominatorUnits`) | Compound (numerator*denominator) units ARE supported. `as_slash` (division-vs-separator provenance) has no JS representation and is dropped, same lossy corner as the real API. |
//! | `String` | `SassString` class instance (`text`, `hasQuotes`) | Direct. |
//! | `List` | `SassList` class instance (`contents`, `separator`, `brackets`) | Direct. A bare JS `Array` is also accepted as an ergonomic convenience on the way in (becomes a comma-separated, non-bracketed list) — the real API requires an explicit `SassList`, this is a grass-specific relaxation. |
//! | `ArgList` | `SassList`-shaped (positional elements only) | The keyword-argument map is dropped; slice 2 does not expose `.keywords`/`SassArgumentList`. |
//! | `Color`/`Map`/`Calculation`/`FunctionRef`/`MixinRef` | unsupported | Marshalling errors out with a clear message naming the type. Deferred: see todo #221's design doc §7.1 for `SassFunction`/`SassMixin` (needs the same callback-capturing plumbing bidirectionally) and later slices for `SassColor`/`SassMap`/`SassCalculation`. |

use std::rc::Rc;

use napi::bindgen_prelude::{Either, FromNapiValue, ToNapiValue, ValidateNapiValue};
use napi::{Env, Error, JsUnknown, NapiValue, Result, ValueType};
use napi_derive::napi;

use grass_compiler::sass_value as sass;

/// `SassNumber(value, unit?)`'s second argument, matching the real Sass JS
/// API's `{numeratorUnits, denominatorUnits}` shape for compound units.
#[napi(object)]
pub struct SassNumberUnits {
    pub numerator_units: Option<Vec<String>>,
    pub denominator_units: Option<Vec<String>>,
}

/// A Sass number, mirroring the real Sass JS API's `SassNumber` class
/// (`value`, `numeratorUnits`, `denominatorUnits`). Constructed either with
/// a single unit string (`new SassNumber(1, "px")`) or explicit
/// numerator/denominator arrays for compound units
/// (`new SassNumber(1, {numeratorUnits: ["px"], denominatorUnits: ["s"]})`).
#[napi]
pub struct SassNumber {
    pub value: f64,
    pub numerator_units: Vec<String>,
    pub denominator_units: Vec<String>,
}

#[napi]
impl SassNumber {
    #[napi(constructor)]
    pub fn new(value: f64, unit: Option<Either<String, SassNumberUnits>>) -> Self {
        let (numerator_units, denominator_units) = match unit {
            None => (Vec::new(), Vec::new()),
            Some(Either::A(single)) => (vec![single], Vec::new()),
            Some(Either::B(units)) => (
                units.numerator_units.unwrap_or_default(),
                units.denominator_units.unwrap_or_default(),
            ),
        };

        Self {
            value,
            numerator_units,
            denominator_units,
        }
    }
}

/// A Sass string, mirroring the real Sass JS API's `SassString` class
/// (`text`, `hasQuotes`). `hasQuotes` defaults to `true`, matching the real
/// API's default (an unquoted string must opt in explicitly).
#[napi]
pub struct SassString {
    pub text: String,
    pub has_quotes: bool,
}

#[napi]
impl SassString {
    #[napi(constructor)]
    pub fn new(text: String, has_quotes: Option<bool>) -> Self {
        Self {
            text,
            has_quotes: has_quotes.unwrap_or(true),
        }
    }
}

/// A Sass list, mirroring the real Sass JS API's `SassList` class
/// (`contents`, `separator`, `brackets`). Unlike the real API, `separator`
/// is a readable word (`"comma"`/`"space"`/`"slash"`/`""` for undecided)
/// rather than a single-character token, to avoid ambiguity with an actual
/// comma/space/slash appearing as data — a grass-specific divergence.
#[napi]
pub struct SassList {
    pub separator: String,
    pub brackets: bool,
    elems: Vec<sass::Value>,
}

#[napi]
impl SassList {
    #[napi(constructor)]
    pub fn new(
        env: Env,
        contents: Vec<JsUnknown>,
        separator: Option<String>,
        brackets: Option<bool>,
    ) -> Result<Self> {
        let elems = contents
            .into_iter()
            .map(|item| js_value_to_sass(env, item))
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            separator: separator.unwrap_or_else(|| "comma".to_owned()),
            brackets: brackets.unwrap_or(false),
            elems,
        })
    }

    #[napi(getter)]
    pub fn contents(&self, env: Env) -> Result<Vec<JsUnknown>> {
        self.elems
            .iter()
            .map(|v| sass_value_to_js(env, v))
            .collect()
    }
}

pub fn to_unknown<T: ToNapiValue>(env: Env, val: T) -> Result<JsUnknown> {
    unsafe {
        let raw = T::to_napi_value(env.raw(), val)?;
        JsUnknown::from_raw(env.raw(), raw)
    }
}

pub(crate) fn list_separator_to_str(sep: sass::ListSeparator) -> &'static str {
    match sep {
        sass::ListSeparator::Space => "space",
        sass::ListSeparator::Comma => "comma",
        sass::ListSeparator::Slash => "slash",
        sass::ListSeparator::Undecided => "",
    }
}

pub(crate) fn list_separator_from_str(s: &str) -> sass::ListSeparator {
    match s {
        "space" => sass::ListSeparator::Space,
        "slash" => sass::ListSeparator::Slash,
        "" => sass::ListSeparator::Undecided,
        _ => sass::ListSeparator::Comma,
    }
}

/// Converts a grass `Unit` into the JS-facing `(numeratorUnits,
/// denominatorUnits)` string arrays. Uses only `grass_compiler`'s public
/// API (`Unit::Complex`/`ComplexUnit`'s fields are public, `Display` gives
/// each single unit's canonical string) — no compiler-crate changes needed.
pub(crate) fn unit_to_js_units(unit: &sass::Unit) -> (Vec<String>, Vec<String>) {
    match unit {
        sass::Unit::None => (Vec::new(), Vec::new()),
        sass::Unit::Complex(complex) => (
            complex.numer.iter().map(ToString::to_string).collect(),
            complex.denom.iter().map(ToString::to_string).collect(),
        ),
        other => (vec![other.to_string()], Vec::new()),
    }
}

/// The inverse of [`unit_to_js_units`], built the same way: `Unit::from`
/// (public) parses each unit string, `Unit::Complex`/`ComplexUnit` (public
/// variant/fields) assembles compound units.
pub(crate) fn js_units_to_unit(
    numerator_units: Vec<String>,
    denominator_units: Vec<String>,
) -> sass::Unit {
    let numer: Vec<sass::Unit> = numerator_units.into_iter().map(sass::Unit::from).collect();
    let denom: Vec<sass::Unit> = denominator_units
        .into_iter()
        .map(sass::Unit::from)
        .collect();

    if denom.is_empty() && numer.len() <= 1 {
        numer.into_iter().next().unwrap_or(sass::Unit::None)
    } else {
        sass::Unit::Complex(std::sync::Arc::new(sass::ComplexUnit { numer, denom }))
    }
}

fn unsupported_sass_value(kind: &str) -> Error {
    Error::from_reason(format!(
        "grass's `functions` bridge does not yet support Sass {kind} values (todo #221 slice 2). \
         Supported types: booleans, null, numbers, strings, lists."
    ))
}

/// Converts a grass-internal `Value` into the JS value handed to a
/// `functions` callback as one of its `args`.
pub fn sass_value_to_js(env: Env, value: &sass::Value) -> Result<JsUnknown> {
    match value {
        sass::Value::True => Ok(env.get_boolean(true)?.into_unknown()),
        sass::Value::False => Ok(env.get_boolean(false)?.into_unknown()),
        sass::Value::Null => Ok(env.get_null()?.into_unknown()),
        sass::Value::Dimension(n) => {
            let (numerator_units, denominator_units) = unit_to_js_units(&n.unit);
            to_unknown(
                env,
                SassNumber {
                    value: n.num.0,
                    numerator_units,
                    denominator_units,
                },
            )
        }
        sass::Value::String(s, quote_kind) => to_unknown(
            env,
            SassString {
                text: s.to_string(),
                has_quotes: matches!(quote_kind, sass::QuoteKind::Quoted),
            },
        ),
        sass::Value::List(items, sep, brackets) => to_unknown(
            env,
            SassList {
                separator: list_separator_to_str(*sep).to_owned(),
                brackets: matches!(brackets, sass::Brackets::Bracketed),
                elems: items.as_ref().clone(),
            },
        ),
        sass::Value::ArgList(arglist) => to_unknown(
            env,
            SassList {
                separator: list_separator_to_str(arglist.separator).to_owned(),
                brackets: false,
                elems: arglist.elems.clone(),
            },
        ),
        sass::Value::Color(_) => Err(unsupported_sass_value("SassColor")),
        sass::Value::Map(_) => Err(unsupported_sass_value("SassMap")),
        sass::Value::Calculation(_) => Err(unsupported_sass_value("SassCalculation")),
        sass::Value::FunctionRef(_) => Err(unsupported_sass_value("SassFunction")),
        // `MixinRef`'s field type (`ast::SassMixin`) isn't part of grass_compiler's
        // public API surface, so it can't be named in a pattern here — a bare
        // wildcard arm covers it (and stays correct if new variants are added).
        _ => Err(unsupported_sass_value("SassMixin")),
    }
}

/// Converts a JS value (a `functions` callback's return value, or a nested
/// `SassList` constructor argument) into a grass-internal `Value`.
pub fn js_value_to_sass(env: Env, value: JsUnknown) -> Result<sass::Value> {
    match value.get_type()? {
        ValueType::Boolean => {
            let b = value.coerce_to_bool()?.get_value()?;
            Ok(if b {
                sass::Value::True
            } else {
                sass::Value::False
            })
        }
        ValueType::Null | ValueType::Undefined => Ok(sass::Value::Null),
        ValueType::Object => {
            let raw = unsafe { <JsUnknown as napi::NapiRaw>::raw(&value) };

            if unsafe { <&SassNumber as ValidateNapiValue>::validate(env.raw(), raw) }.is_ok() {
                let n: &SassNumber = unsafe { FromNapiValue::from_napi_value(env.raw(), raw)? };
                return Ok(sass::Value::Dimension(sass::SassNumber {
                    num: sass::Number(n.value),
                    unit: js_units_to_unit(n.numerator_units.clone(), n.denominator_units.clone()),
                    as_slash: None,
                }));
            }

            if unsafe { <&SassString as ValidateNapiValue>::validate(env.raw(), raw) }.is_ok() {
                let s: &SassString = unsafe { FromNapiValue::from_napi_value(env.raw(), raw)? };
                return Ok(sass::Value::String(
                    s.text.clone().into(),
                    if s.has_quotes {
                        sass::QuoteKind::Quoted
                    } else {
                        sass::QuoteKind::None
                    },
                ));
            }

            if unsafe { <&SassList as ValidateNapiValue>::validate(env.raw(), raw) }.is_ok() {
                let l: &SassList = unsafe { FromNapiValue::from_napi_value(env.raw(), raw)? };
                return Ok(sass::Value::List(
                    Rc::new(l.elems.clone()),
                    list_separator_from_str(&l.separator),
                    if l.brackets {
                        sass::Brackets::Bracketed
                    } else {
                        sass::Brackets::None
                    },
                ));
            }

            if value.is_array()? {
                let items: Vec<JsUnknown> =
                    unsafe { FromNapiValue::from_napi_value(env.raw(), raw)? };
                let elems = items
                    .into_iter()
                    .map(|item| js_value_to_sass(env, item))
                    .collect::<Result<Vec<_>>>()?;
                return Ok(sass::Value::List(
                    Rc::new(elems),
                    sass::ListSeparator::Comma,
                    sass::Brackets::None,
                ));
            }

            Err(Error::from_reason(
                "Custom function values must be a boolean, null, SassNumber, SassString, \
                 SassList, or Array (grass's `functions` bridge, todo #221 slice 2 — \
                 SassColor/SassMap/SassCalculation/SassFunction/SassMixin are not yet supported)."
                    .to_owned(),
            ))
        }
        other => Err(Error::from_reason(format!(
            "Custom function values of JS type {other:?} are not supported by grass's \
             `functions` bridge."
        ))),
    }
}
