use bumpalo::Bump;
use codemap::Spanned;

use super::AstExpr;

/// The immutable, arena-backed form of an interpolation, used for AST storage.
/// Built from an [`InterpolationBuilder`] via [`InterpolationBuilder::finish`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Interpolation<'a> {
    pub contents: &'a [InterpolationPart<'a>],
}

impl<'a> Interpolation<'a> {
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    pub fn initial_plain(&self) -> &str {
        match self.contents.first() {
            Some(InterpolationPart::String(s)) => s,
            _ => "",
        }
    }

    pub fn as_plain(&self) -> Option<&str> {
        if self.contents.is_empty() {
            Some("")
        } else if self.contents.len() > 1 {
            None
        } else {
            match self.contents.first()? {
                InterpolationPart::String(s) => Some(s),
                InterpolationPart::Expr(..) => None,
            }
        }
    }

    pub fn trailing_string(&self) -> &str {
        match self.contents.last() {
            Some(InterpolationPart::String(s)) => s,
            Some(InterpolationPart::Expr(..)) | None => "",
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, InterpolationPart<'a>> {
        self.contents.iter()
    }
}

impl<'a> IntoIterator for Interpolation<'a> {
    type Item = &'a InterpolationPart<'a>;
    type IntoIter = std::slice::Iter<'a, InterpolationPart<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.contents.iter()
    }
}

/// The mutable, `std::Vec`-backed builder used incrementally by the parser.
/// Its backing buffer (and every `String` inside an
/// [`InterpolationPartBuilder::String`]) is a plain heap allocation freed
/// normally when it goes out of scope; only the finished form
/// ([`Interpolation`]) is arena-allocated via [`InterpolationBuilder::finish`],
/// which also copies each string part into the arena via `alloc_str` so no
/// `String` buffer ends up embedded by value inside the never-dropped arena.
#[derive(Debug, Clone, Default)]
pub struct InterpolationBuilder<'a> {
    pub contents: Vec<InterpolationPartBuilder<'a>>,
}

impl<'a> InterpolationBuilder<'a> {
    pub fn new() -> Self {
        Self {
            contents: Vec::new(),
        }
    }

    /// Reopens an already-finished [`Interpolation`] for further mutation,
    /// cloning its parts into a fresh `Vec` (rare — only needed where a
    /// finished interpolation must be extended, e.g. CSS-native `if()`'s
    /// adjacent-raw-item extension).
    pub fn from_interpolation(interp: Interpolation<'a>) -> Self {
        Self {
            contents: interp
                .contents
                .iter()
                .map(|part| match part {
                    InterpolationPart::String(s) => {
                        InterpolationPartBuilder::String((*s).to_owned())
                    }
                    InterpolationPart::Expr(e) => InterpolationPartBuilder::Expr(e.clone()),
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    pub fn new_with_expr(e: Spanned<AstExpr<'a>>) -> Self {
        Self {
            contents: vec![InterpolationPartBuilder::Expr(e)],
        }
    }

    pub fn new_plain(s: String) -> Self {
        Self {
            contents: vec![InterpolationPartBuilder::String(s)],
        }
    }

    pub fn add_expr(&mut self, expr: Spanned<AstExpr<'a>>) {
        self.contents.push(InterpolationPartBuilder::Expr(expr));
    }

    pub fn add_string(&mut self, s: String) {
        match self.contents.last_mut() {
            Some(InterpolationPartBuilder::String(existing)) => existing.push_str(&s),
            _ => self.contents.push(InterpolationPartBuilder::String(s)),
        }
    }

    /// Like `add_string`, but takes a borrowed `&str`. Avoids an extra
    /// allocation at call sites that would otherwise build a throwaway
    /// `String` purely to hand it to `add_string`.
    pub fn add_str(&mut self, s: &str) {
        match self.contents.last_mut() {
            Some(InterpolationPartBuilder::String(existing)) => existing.push_str(s),
            _ => self
                .contents
                .push(InterpolationPartBuilder::String(s.to_owned())),
        }
    }

    /// Returns a mutable handle to the trailing string part, creating an
    /// empty one if the interpolation is empty or ends in an expression.
    /// Appends made through this handle merge into the existing text
    /// without any intermediate `String` allocation.
    pub fn trailing_string_mut(&mut self) -> &mut String {
        if !matches!(
            self.contents.last(),
            Some(InterpolationPartBuilder::String(_))
        ) {
            self.contents
                .push(InterpolationPartBuilder::String(String::new()));
        }

        match self.contents.last_mut() {
            Some(InterpolationPartBuilder::String(s)) => s,
            _ => unreachable!(),
        }
    }

    pub fn add_char(&mut self, c: char) {
        match self.contents.last_mut() {
            Some(InterpolationPartBuilder::String(existing)) => existing.push(c),
            _ => self
                .contents
                .push(InterpolationPartBuilder::String(c.to_string())),
        }
    }

    pub fn add_interpolation(&mut self, mut other: Self) {
        self.contents.append(&mut other.contents);
    }

    pub fn initial_plain(&self) -> &str {
        match self.contents.first() {
            Some(InterpolationPartBuilder::String(s)) => s,
            _ => "",
        }
    }

    pub fn as_plain(&self) -> Option<&str> {
        if self.contents.is_empty() {
            Some("")
        } else if self.contents.len() > 1 {
            None
        } else {
            match self.contents.first()? {
                InterpolationPartBuilder::String(s) => Some(s),
                InterpolationPartBuilder::Expr(..) => None,
            }
        }
    }

    pub fn trailing_string(&self) -> &str {
        match self.contents.last() {
            Some(InterpolationPartBuilder::String(s)) => s,
            Some(InterpolationPartBuilder::Expr(..)) | None => "",
        }
    }

    /// Consumes the builder's `Vec` buffer into an arena-allocated slice.
    /// Each `String` part is copied into the arena via `alloc_str` (so its
    /// heap buffer is freed normally when the builder's `Vec` drops); the
    /// `Vec`'s own backing buffer is freed normally by `alloc_slice_fill_iter`
    /// draining it. Nothing leaks into the arena's never-dropped storage.
    pub fn finish(self, arena: &'a Bump) -> Interpolation<'a> {
        Interpolation {
            contents: arena.alloc_slice_fill_iter(self.contents.into_iter().map(|part| {
                match part {
                    InterpolationPartBuilder::String(s) => {
                        InterpolationPart::String(arena.alloc_str(&s))
                    }
                    InterpolationPartBuilder::Expr(e) => InterpolationPart::Expr(e),
                }
            })),
        }
    }
}

/// The immutable, arena-backed form of an interpolation part, used for AST
/// storage. See [`InterpolationPartBuilder`] for the mutable builder form.
#[derive(Debug, Clone)]
pub enum InterpolationPart<'a> {
    String(&'a str),
    Expr(Spanned<AstExpr<'a>>),
}

/// The mutable, `std::String`-backed builder form of an interpolation part,
/// used incrementally by the parser. See [`InterpolationPart`] for the
/// finished, arena-backed storage form.
#[derive(Debug, Clone)]
pub enum InterpolationPartBuilder<'a> {
    String(String),
    Expr(Spanned<AstExpr<'a>>),
}
