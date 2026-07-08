// A reference to the parser is only necessary for some functions
#![allow(unused_variables)]

use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use rustc_hash::{FxHashMap, FxHashSet};

use std::sync::{Arc, LazyLock};

use codemap::Span;

use crate::{
    ast::ArgumentResult,
    error::SassResult,
    evaluate::Visitor,
    serializer::serialize_number,
    unit::Unit,
    value::{Number, SassNumber, Value},
    Options,
};

#[cfg(any(feature = "custom-builtin-fns", doc))]
use codemap::CodeMap;

#[cfg(any(feature = "custom-builtin-fns", doc))]
use crate::error::SassError;

pub mod color;
pub mod list;
pub mod map;
pub mod math;
pub mod meta;
pub mod selector;
pub mod string;

// todo: maybe Identifier instead of str?
pub(crate) type GlobalFunctionMap = FxHashMap<&'static str, Builtin>;

/// Builds the `global-builtin` deprecation message dart-sass shows for a
/// global function call, given the `sass:*` module and function name that
/// replaces it (e.g. `global_builtin_message("color", "adjust")`).
pub(crate) fn global_builtin_message(module: &str, name: &str) -> String {
    format!(
        "Global built-in functions are deprecated and will be removed in Dart Sass 3.0.0.\nUse \
         {module}.{name} instead.\n\nMore info and automated migrator: \
         https://sass-lang.com/d/import"
    )
}

/// Builds the `color-functions` deprecation message dart-sass shows for a
/// deprecated single-channel getter (`red()`/`color.red()`,
/// `hue()`/`color.hue()`, etc.), given whether it was called through its
/// global name (`is_global`) or the `color.*` module, the function's own
/// `name`, and the `sass:color` `$space` argument `color.channel()` needs
/// (`"rgb"` for red/green/blue, `"hsl"` for hue/saturation/lightness).
pub(crate) fn color_channel_getter_message(is_global: bool, name: &str, space: &str) -> String {
    let prefix = if is_global { "" } else { "color." };
    format!(
        "{prefix}{name}() is deprecated. Suggestion:\n\ncolor.channel($color, \"{name}\", \
         $space: {space})\n\nMore info: https://sass-lang.com/d/color-functions"
    )
}

/// Transcribes dart-sass's `SassNumber.unitSuggestion` (`lib/src/value/number.dart`):
/// a suggested Sass snippet for converting a variable named `$name` containing
/// `unit`'s numerator/denominator units into a number with the given
/// `expected_unit` (or unitless, if `expected_unit` is `None`). Wraps the
/// result in `calc(...)` whenever the source has at least one numerator unit
/// (matching dart's `numeratorUnits.isEmpty` check) — this is why a bare
/// `$name * 1%` (unitless source, percent-expected) is NOT calc-wrapped but
/// `calc($name / 1px * 1%)` (a numerator-bearing source) is.
fn unit_suggestion(name: &str, unit: &Unit, expected_unit: Option<&str>) -> String {
    let (numer, denom) = unit.clone().numer_and_denom();

    let mut result = format!("${name}");
    for d in &denom {
        result.push_str(&format!(" * 1{d}"));
    }
    for n in &numer {
        result.push_str(&format!(" / 1{n}"));
    }
    if let Some(u) = expected_unit {
        result.push_str(&format!(" * 1{u}"));
    }

    if numer.is_empty() {
        result
    } else {
        format!("calc({result})")
    }
}

/// Builds the `function-units` deprecation message for a built-in function
/// argument that was passed a number with an invalid (or missing) unit,
/// e.g. `list.nth($list, 1px)`. `name` is the parameter name (without `$`),
/// `unit` is the unit actually passed. Also covers dart's `_adjustChannel`
/// alpha-unit check (`color.adjust($c, $alpha: ...)` for any color space),
/// which uses this exact same message template. Verified against npx
/// sass@1.97.3 (`list.nth($l, 1px)` → `calc($n / 1px)`).
pub(crate) fn function_units_message(name: &str, unit: &Unit) -> String {
    format!(
        "${name}: Passing a number with unit {unit} is deprecated.\n\nTo preserve current \
         behavior: {}\n\nMore info: https://sass-lang.com/d/function-units",
        unit_suggestion(name, unit, None)
    )
}

/// Builds dart-sass's `_angleValue`/legacy-`color.change()`-alpha deprecation
/// message family: "Passing a unit other than `expected_unit` (`value_display`)
/// is deprecated", ending in "See" (not "More info:") — distinct wording from
/// [`function_units_message`]/[`function_percent_message`], verified against
/// npx sass@1.97.3 for both call sites (`adjust-hue`'s hue arg,
/// `color.change()`'s legacy `$alpha`).
pub(crate) fn function_unit_other_than_message(
    name: &str,
    expected_unit: &str,
    value_display: &str,
    unit: &Unit,
) -> String {
    format!(
        "${name}: Passing a unit other than {expected_unit} ({value_display}) is \
         deprecated.\n\nTo preserve current behavior: {}\n\nSee \
         https://sass-lang.com/d/function-units",
        unit_suggestion(name, unit, None)
    )
}

/// Builds dart-sass's `_checkPercent` deprecation message: "Passing a number
/// without unit % (`value_display`) is deprecated", ending in "More info:".
/// Used by `mix()`/`invert()`'s legacy `$weight` and the `hsl()` constructor's
/// `$saturation`/`$lightness`. Verified against npx sass@1.97.3.
pub(crate) fn function_percent_message(name: &str, value_display: &str, unit: &Unit) -> String {
    format!(
        "${name}: Passing a number without unit % ({value_display}) is deprecated.\n\nTo \
         preserve current behavior: {}\n\nMore info: https://sass-lang.com/d/function-units",
        unit_suggestion(name, unit, Some("%"))
    )
}

/// The legacy HSL/alpha channels `_suggestScaleAndAdjust` operates over —
/// bundles the channel's dart-source name, its bounds, and the unit its
/// `color.adjust(...)` suggestion is serialized with (`%` for lightness/
/// saturation, unitless for alpha), matching `ColorChannel`/`LinearChannel`
/// in dart's `lib/src/value/color/channel.dart`.
pub(crate) enum LegacyChannel {
    Lightness,
    Saturation,
    Alpha,
}

impl LegacyChannel {
    fn name(&self) -> &'static str {
        match self {
            Self::Lightness => "lightness",
            Self::Saturation => "saturation",
            Self::Alpha => "alpha",
        }
    }

    fn bounds(&self) -> (f64, f64) {
        match self {
            Self::Lightness | Self::Saturation => (0.0, 100.0),
            Self::Alpha => (0.0, 1.0),
        }
    }

    fn difference_unit(&self) -> Unit {
        match self {
            Self::Lightness | Self::Saturation => Unit::Percent,
            Self::Alpha => Unit::None,
        }
    }
}

/// Transcribes dart-sass's `_suggestScaleAndAdjust` (lib/src/functions/color.dart):
/// builds the `color.scale(...)` + `color.adjust(...)` suggestion pair shown for
/// the deprecated lighten/darken/saturate/desaturate/opacify/fade-in/
/// transparentize/fade-out functions.
///
/// `old_value` is the color's CURRENT value of `channel`, and `adjustment` is
/// the signed delta requested by the user — both already on the channel's own
/// scale (0-100 for lightness/saturation, 0-1 for alpha).
pub(crate) fn suggest_scale_and_adjust(
    old_value: Number,
    adjustment: Number,
    channel: LegacyChannel,
    span: Span,
    options: &Options,
) -> SassResult<String> {
    let channel_name = channel.name();
    let (channel_min, channel_max) = channel.bounds();
    let new_value = old_value.0 + adjustment.0;

    let mut suggestion = String::from("Suggestion");

    if adjustment.0 != 0.0 {
        let factor = if new_value > channel_max {
            1.0
        } else if new_value < channel_min {
            -1.0
        } else if adjustment.0 > 0.0 {
            adjustment.0 / (channel_max - old_value.0)
        } else {
            (new_value - old_value.0) / (old_value.0 - channel_min)
        };

        let factor_number = SassNumber {
            num: Number(factor * 100.0),
            unit: Unit::Percent,
            as_slash: None,
        };
        let factor_text = serialize_number(&factor_number, options, span)?;
        suggestion.push_str(&format!(
            "s:\n\ncolor.scale($color, ${channel_name}: {factor_text})\n"
        ));
    } else {
        suggestion.push_str(":\n\n");
    }

    let difference = SassNumber {
        num: adjustment,
        unit: channel.difference_unit(),
        as_slash: None,
    };
    let difference_text = serialize_number(&difference, options, span)?;
    suggestion.push_str(&format!(
        "color.adjust($color, ${channel_name}: {difference_text})"
    ));

    Ok(suggestion)
}

static FUNCTION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// A function implemented in rust that is accessible from within Sass
///
///
/// #### Usage
/// ```rust
/// use grass_compiler::{
///     sass_value::{ArgumentResult, SassNumber, Value},
///     Builtin, Options, Result as SassResult, Visitor,
/// };
///
/// // An example function that looks up the length of an array or map and adds 2 to it
/// fn length(mut args: ArgumentResult, visitor: &mut Visitor) -> SassResult<Value> {
///     args.max_args(1)?;
///
///     let len = args.get_err(0, "list")?.as_list().len();
///
///     Ok(Value::Dimension(SassNumber::new_unitless(len + 2)))
/// }
///
/// fn main() {
///     let options = Options::default().add_custom_fn("length", Builtin::new(length));
///     let css = grass_compiler::from_string("a { color: length([a, b]); }", &options).unwrap();
///
///     assert_eq!(css, "a {\n  color: 4;\n}\n");
/// }
/// ```
/// A function pointer usable as the body of a dynamically-registered
/// [`Builtin`] (see [`BuiltinFn::Dynamic`]). `Arc` (not `Rc`) is required
/// because `Builtin` is stored inside `GLOBAL_FUNCTIONS`, a `static
/// LazyLock<GlobalFunctionMap>` — the map's value type must be `Sync` even
/// though no `GLOBAL_FUNCTIONS` entry is ever `Dynamic`, since the bound is
/// checked against the enum's type, not any particular value. This does
/// NOT require the closure to touch `Visitor`/`Value` (both `Rc`-heavy,
/// `!Send`) across threads — only whatever state the closure *captures*
/// must be thread-safe.
pub(crate) type DynamicBuiltinFn = dyn Fn(ArgumentResult, &mut Visitor) -> SassResult<Value> + Send + Sync;

/// The callable body of a [`Builtin`].
pub(crate) enum BuiltinFn {
    /// A plain Rust function pointer — the original, zero-overhead form
    /// used by every builtin in `GLOBAL_FUNCTIONS` and by
    /// [`Builtin::new`].
    Static(fn(ArgumentResult, &mut Visitor) -> SassResult<Value>),
    /// A closure registered via
    /// [`Options::add_custom_fn_with_signature`], together with the raw
    /// `(...)` signature text (if any) used to bind call arguments to
    /// declared parameter names/defaults/rest before invoking `f`. `None`
    /// means the closure receives the raw unbound [`ArgumentResult`], same
    /// as the [`BuiltinFn::Static`] path.
    Dynamic {
        f: Arc<DynamicBuiltinFn>,
        signature: Option<Arc<str>>,
    },
}

impl Clone for BuiltinFn {
    fn clone(&self) -> Self {
        match self {
            Self::Static(f) => Self::Static(*f),
            Self::Dynamic { f, signature } => Self::Dynamic {
                f: Arc::clone(f),
                signature: signature.clone(),
            },
        }
    }
}

#[derive(Clone)]
pub struct Builtin(
    pub(crate) BuiltinFn,
    usize,
    pub(crate) Option<(&'static str, &'static str)>,
);

impl fmt::Debug for Builtin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Builtin");
        s.field("id", &self.1);
        match &self.0 {
            BuiltinFn::Static(func) => {
                s.field("fn_ptr", &(*func as usize));
            }
            BuiltinFn::Dynamic { signature, .. } => {
                s.field("dynamic_signature", signature);
            }
        }
        s.finish()
    }
}

impl Builtin {
    pub fn new(body: fn(ArgumentResult, &mut Visitor) -> SassResult<Value>) -> Builtin {
        let count = FUNCTION_COUNT.fetch_add(1, Ordering::Relaxed);
        Self(BuiltinFn::Static(body), count, None)
    }

    /// Registers a closure-backed builtin, optionally bound to a signature
    /// string (the `(...)` argument-declaration text, without the leading
    /// name) parsed lazily on first call. See
    /// [`Options::add_custom_fn_with_signature`].
    #[cfg(any(feature = "custom-builtin-fns", doc))]
    pub(crate) fn new_dynamic(f: Arc<DynamicBuiltinFn>, signature: Option<Arc<str>>) -> Builtin {
        let count = FUNCTION_COUNT.fetch_add(1, Ordering::Relaxed);
        Self(BuiltinFn::Dynamic { f, signature }, count, None)
    }

    /// Marks this global function as replaced by `{module}.{name}` in the
    /// `sass:*` module system, for the `global-builtin` deprecation warning.
    /// Only meaningful on entries in `GLOBAL_FUNCTIONS` — carrying this
    /// alongside the `Builtin` itself avoids a second hashmap lookup on
    /// every global function call in `Visitor::visit_function_call_expr`,
    /// which measurably regressed Bootstrap compile time when the mapping
    /// lived in a separate map (see solo todo #158 / scratchpad #86).
    ///
    /// Mappings are dart-derived from `lib/src/functions/*.dart`'s
    /// `withDeprecationWarning` call sites. Deliberately NOT applied here
    /// (handled elsewhere, or never deprecated by dart-sass):
    /// - `grayscale`, `invert`, `opacity`, `saturate`, `alpha`: warn only
    ///   for some argument shapes (color vs. plain-CSS-filter-number),
    ///   implemented inline at their call sites in this module.
    /// - `rgb`, `rgba`, `hsl`, `hsla`, `hwb`, `lab`, `lch`, `oklab`,
    ///   `oklch`, `color`: plain-CSS-compatible color constructors, never
    ///   deprecated.
    /// - `ie-hex-str`: permanently global-only in dart-sass, no module form.
    /// - `whiteness`, `blackness`: grass-only globals (dart-sass only
    ///   exposes these as `color.whiteness`/`color.blackness`); not part of
    ///   dart's global-builtin set at all.
    /// - `if`: no module equivalent in dart-sass.
    /// - `clamp`, `hypot`: pure CSS calculation syntax in dart-sass, never
    ///   registered as global Sass functions.
    pub(crate) fn with_deprecated_global(mut self, module: &'static str, name: &'static str) -> Self {
        self.2 = Some((module, name));
        self
    }
}

impl PartialEq for Builtin {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

impl Eq for Builtin {}

/// Splits a full custom-function signature string (e.g. `"sum($a, $b: 1)"`)
/// into its normalized bare name (`_` becomes `-`, matching `@function` name
/// parsing) and the `(...)` argument-declaration text, which is parsed
/// lazily per-compile (see `Visitor::parse_dynamic_signature`). Pure string
/// slicing — no parser involved, so this can run before any compile/CodeMap
/// exists, at `Options::add_custom_fn_with_signature` time.
#[cfg(any(feature = "custom-builtin-fns", doc))]
pub(crate) fn split_signature_name(signature: &str) -> SassResult<(String, String)> {
    let malformed = || malformed_signature_error(signature);

    let paren_idx = signature.find('(').ok_or_else(malformed)?;
    let name = signature[..paren_idx].trim();

    if name.is_empty() || !signature.trim_end().ends_with(')') {
        return Err(malformed());
    }

    let normalized_name = name.replace('_', "-");
    let arg_text = signature[paren_idx..].trim_end().to_string();

    Ok((normalized_name, arg_text))
}

/// Builds a proper `ParseError` (never `SassErrorKind::Raw`, which panics on
/// `Display`/`.kind()` if it ever escapes to a public caller) for a
/// malformed custom-function signature, using a throwaway `CodeMap` since
/// this runs before any compile has begun and there is no real `CodeMap` to
/// mint a span from yet.
#[cfg(any(feature = "custom-builtin-fns", doc))]
fn malformed_signature_error(signature: &str) -> Box<SassError> {
    let mut map = CodeMap::new();
    let file = map.add_file("<custom-fn-signature>".to_string(), signature.to_string());
    let loc = map.look_up_span(file.span);
    Box::new(SassError::from_loc(
        format!("Invalid custom function signature: {signature:?}. Expected \"name(...)\"."),
        loc,
        true,
    ))
}

pub(crate) static GLOBAL_FUNCTIONS: LazyLock<GlobalFunctionMap> = LazyLock::new(|| {
    let mut m = FxHashMap::default();
    color::declare(&mut m);
    list::declare(&mut m);
    map::declare(&mut m);
    math::declare(&mut m);
    meta::declare(&mut m);
    selector::declare(&mut m);
    string::declare(&mut m);
    m
});

pub(crate) static DISALLOWED_PLAIN_CSS_FUNCTION_NAMES: LazyLock<FxHashSet<&str>> = LazyLock::new(|| {
    GLOBAL_FUNCTIONS
        .keys()
        .copied()
        .filter(|&name| {
            !matches!(
                name,
                "rgb"
                    | "rgba"
                    | "hsl"
                    | "hsla"
                    | "grayscale"
                    | "invert"
                    | "alpha"
                    | "opacity"
                    | "saturate"
                    | "lab"
                    | "lch"
                    | "oklab"
                    | "oklch"
                    | "color"
            )
        })
        .collect()
});
