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
}

impl Deprecation {
    /// The kebab-case ID used to refer to this deprecation on the command
    /// line and in the JS API, e.g. `"slash-div"`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::SlashDiv => "slash-div",
        }
    }

    /// Whether this deprecation is not yet active by default and must be
    /// explicitly opted into (dart-sass's `futureDeprecations`).
    #[must_use]
    pub const fn is_future(self) -> bool {
        match self {
            Self::SlashDiv => false,
        }
    }
}

impl fmt::Display for Deprecation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}
