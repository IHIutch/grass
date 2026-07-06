use std::fmt;

/// A deprecated Sass feature, identified by the same kebab-case ID dart-sass
/// uses for `--silence-deprecation` / `--fatal-deprecation` and the JS
/// `silenceDeprecations` / `fatalDeprecations` / `futureDeprecations` APIs.
///
/// Only variants that are actually wired up to an `emit_deprecation` call
/// site exist here; see dart-sass's `lib/src/deprecation.dart` for the full
/// set still to be seeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Deprecation {
    /// Using `/` for division outside of `calc()`.
    SlashDiv,
    /// Using the `@elseif` typo instead of `@else if`.
    Elseif,
    /// Declaring a new variable with `!global`.
    NewGlobal,
    /// Using the legacy `@import` rule to load a Sass file.
    Import,
    /// Calling a built-in function via its global name instead of through
    /// its `sass:*` module (e.g. `map-get()` instead of `map.get()`).
    GlobalBuiltin,
    /// Writing `left -right` (whitespace before `+`/`-` but not after), which
    /// is ambiguous between a binary operation and a unary negation.
    StrictUnary,
    /// Calling the legacy `if($condition, $if-true, $if-false)` syntax
    /// instead of the modern `if(<condition>: <value>)` CSS syntax.
    IfFunction,
    /// A selector with a leading, trailing, or doubled-up combinator (e.g.
    /// `+ .a`, `.a >`, `.a + + .b`).
    BogusCombinators,
    /// Calling a global color function (or its `color.*` module
    /// equivalent) that's been superseded by `color.channel()` /
    /// `color.adjust()` / `color.scale()`.
    ColorFunctions,
    /// Passing a string directly to `meta.call()` instead of a function
    /// reference from `meta.get-function()`.
    CallString,
    /// Using `@-moz-document`.
    MozDocument,
    /// Calling `feature-exists()` / `meta.feature-exists()`.
    FeatureExists,
    /// Using a `color.*` module function (`color.red()`, `color.hwb()`,
    /// etc.) in place of the plain-CSS function it shadows.
    ColorModuleCompat,
    /// Configuring a private (`-`/`_`-prefixed) variable via `@use ... with`,
    /// `@forward ... with`, or `load-css()`'s `$with` argument.
    WithPrivate,
    /// A rest parameter (`$args...`) declared or passed before a positional
    /// or named argument.
    MisplacedRest,
    /// Calling the global `abs()` function with a percentage argument
    /// outside of `calc()`.
    AbsPercent,
    /// Passing an invalid unit (or missing a required one) to a built-in
    /// function argument, e.g. `list.nth($list, 1px)` or `math.random(1px)`.
    FunctionUnits,
    /// Writing `!default` or `!global` more than once for the same variable
    /// declaration.
    DuplicateVarFlags,
}

impl Deprecation {
    /// The kebab-case ID used to refer to this deprecation on the command
    /// line and in the JS API, e.g. `"slash-div"`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::SlashDiv => "slash-div",
            Self::Elseif => "elseif",
            Self::NewGlobal => "new-global",
            Self::Import => "import",
            Self::GlobalBuiltin => "global-builtin",
            Self::StrictUnary => "strict-unary",
            Self::IfFunction => "if-function",
            Self::BogusCombinators => "bogus-combinators",
            Self::ColorFunctions => "color-functions",
            Self::CallString => "call-string",
            Self::MozDocument => "moz-document",
            Self::FeatureExists => "feature-exists",
            Self::ColorModuleCompat => "color-module-compat",
            Self::WithPrivate => "with-private",
            Self::MisplacedRest => "misplaced-rest",
            Self::AbsPercent => "abs-percent",
            Self::FunctionUnits => "function-units",
            Self::DuplicateVarFlags => "duplicate-var-flags",
        }
    }

    /// Look up a deprecation by its kebab-case ID, as accepted by the
    /// `--silence-deprecation` / `--fatal-deprecation` / `--future-deprecation`
    /// CLI flags and the `silenceDeprecations` / `fatalDeprecations` /
    /// `futureDeprecations` JS API options.
    ///
    /// Mirrors dart-sass's `Deprecation.fromId`; returns `None` for unknown
    /// IDs (including version strings, which dart-sass's CLI accepts only
    /// for `--fatal-deprecation` via a separate code path this does not
    /// implement).
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "slash-div" => Self::SlashDiv,
            "elseif" => Self::Elseif,
            "new-global" => Self::NewGlobal,
            "import" => Self::Import,
            "global-builtin" => Self::GlobalBuiltin,
            "strict-unary" => Self::StrictUnary,
            "if-function" => Self::IfFunction,
            "bogus-combinators" => Self::BogusCombinators,
            "color-functions" => Self::ColorFunctions,
            "call-string" => Self::CallString,
            "moz-document" => Self::MozDocument,
            "feature-exists" => Self::FeatureExists,
            "color-module-compat" => Self::ColorModuleCompat,
            "with-private" => Self::WithPrivate,
            "misplaced-rest" => Self::MisplacedRest,
            "abs-percent" => Self::AbsPercent,
            _ => return None,
        })
    }

    /// Whether this deprecation is not yet active by default and must be
    /// explicitly opted into (dart-sass's `futureDeprecations`).
    #[must_use]
    pub const fn is_future(self) -> bool {
        match self {
            Self::SlashDiv
            | Self::Elseif
            | Self::NewGlobal
            | Self::Import
            | Self::GlobalBuiltin
            | Self::StrictUnary
            | Self::IfFunction
            | Self::BogusCombinators
            | Self::ColorFunctions
            | Self::CallString
            | Self::MozDocument
            | Self::FeatureExists
            | Self::ColorModuleCompat
            | Self::WithPrivate
            | Self::MisplacedRest
            | Self::AbsPercent
            | Self::FunctionUnits
            | Self::DuplicateVarFlags => false,
        }
    }

    /// Whether this deprecation's message text can vary across repeated
    /// visits to the same (deprecation, span) call site (e.g. a mixin or
    /// function called many times in a loop) — built from EVALUATED content
    /// (a color's current channel value, an argument's runtime unit/value,
    /// an interpolated selector, a call()'d string) rather than purely
    /// static per-call-site metadata (a function/module name, a suggestion
    /// derived from unevaluated source text, which is fixed once a span is
    /// fixed).
    ///
    /// `emit_deprecation` only pays for the extra (span, message) check this
    /// requires when this is `true` — most deprecations' message can't
    /// possibly differ between repeat visits to the same span, so they keep
    /// the cheap (Deprecation, Span)-only fast path (confirmed via profiling:
    /// treating every deprecation as message-varying regressed Bootstrap
    /// ~5% instructions retired, entirely attributable to GlobalBuiltin —
    /// called from inside color-manipulation mixins many times each with an
    /// identical message).
    #[must_use]
    pub(crate) const fn message_may_vary_per_visit(self) -> bool {
        matches!(
            self,
            Self::SlashDiv
                | Self::BogusCombinators
                | Self::ColorFunctions
                | Self::CallString
                | Self::ColorModuleCompat
                | Self::AbsPercent
                | Self::FunctionUnits
                | Self::WithPrivate
        )
    }
}

impl fmt::Display for Deprecation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}
