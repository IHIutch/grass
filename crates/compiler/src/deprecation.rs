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
        }
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
            | Self::BogusCombinators => false,
        }
    }
}

impl fmt::Display for Deprecation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}
