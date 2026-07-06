// A reference to the parser is only necessary for some functions
#![allow(unused_variables)]

use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};

use rustc_hash::{FxHashMap, FxHashSet};

use std::sync::LazyLock;

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

/// Builds the `function-units` deprecation message for a built-in function
/// argument that was passed a number with an invalid (or missing) unit,
/// e.g. `list.nth($list, 1px)`. `name` is the parameter name (without `$`),
/// `unit` is the unit actually passed. Mirrors dart-sass's `unitSuggestion`
/// for the single-unit case (no denominator units), which always wraps the
/// suggestion in `calc(...)` since it has a numerator unit — verified
/// against npx sass@1.97.3 (`list.nth($l, 1px)` → `calc($n / 1px)`).
pub(crate) fn function_units_message(name: &str, unit: &Unit) -> String {
    format!(
        "${name}: Passing a number with unit {unit} is deprecated.\n\nTo preserve current \
         behavior: calc(${name} / 1{unit})\n\nMore info: https://sass-lang.com/d/function-units"
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
#[derive(Clone)]
pub struct Builtin(
    pub(crate) fn(ArgumentResult, &mut Visitor) -> SassResult<Value>,
    usize,
    pub(crate) Option<(&'static str, &'static str)>,
);

impl fmt::Debug for Builtin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builtin")
            .field("id", &self.1)
            .field("fn_ptr", &(self.0 as usize))
            .finish()
    }
}

impl Builtin {
    pub fn new(body: fn(ArgumentResult, &mut Visitor) -> SassResult<Value>) -> Builtin {
        let count = FUNCTION_COUNT.fetch_add(1, Ordering::Relaxed);
        Self(body, count, None)
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
