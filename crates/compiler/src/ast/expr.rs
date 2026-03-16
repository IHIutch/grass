use std::{iter::Iterator, rc::Rc};

use codemap::{Span, Spanned};
use compact_str::CompactString;

use crate::{
    color::Color,
    common::{BinaryOp, Brackets, Identifier, ListSeparator, QuoteKind, UnaryOp},
    unit::Unit,
    value::{CalculationName, Number},
};

use super::{ArgumentInvocation, AstSupportsCondition, Interpolation, InterpolationPart};

/// Represented by the legacy `if($condition, $if-true, $if-false)` function
#[derive(Debug, Clone)]
pub struct Ternary<'a>(pub ArgumentInvocation<'a>);

/// An atom in a CSS-native `if()` condition
#[derive(Debug, Clone)]
pub enum IfConditionAtom<'a> {
    /// `sass(expr)` — evaluable at compile time
    Sass(AstExpr<'a>, Span),
    /// `css(...)`, `var(...)`, `attr(...)`, or any other CSS function — raw passthrough
    /// Contains an Interpolation for the full text (including function name and parens)
    Css(Interpolation<'a>, Span),
    /// Like Css but formed from adjacent raw substitutions (var, attr, if, interp).
    /// Cannot coexist with sass() in the same condition.
    CssRaw(Interpolation<'a>, Span),
    /// `#{...}` interpolation — evaluated then treated as raw CSS
    Interp(AstExpr<'a>, Span),
}

/// A boolean condition in a CSS-native `if()` expression
#[derive(Debug, Clone)]
pub enum IfCondition<'a> {
    Atom(IfConditionAtom<'a>),
    Not(Box<IfCondition<'a>>, Span),
    And(Vec<IfCondition<'a>>),
    Or(Vec<IfCondition<'a>>),
    Paren(Box<IfCondition<'a>>),
    /// The `else` keyword — always true
    Else,
}

/// A single clause in a CSS-native `if()`: `condition: value`
#[derive(Debug, Clone)]
pub struct IfClause<'a> {
    pub condition: IfCondition<'a>,
    pub value: AstExpr<'a>,
}

/// CSS-native `if()` expression with condition clauses
#[derive(Debug, Clone)]
pub struct CssIfExpression<'a> {
    pub clauses: Vec<IfClause<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ListExpr<'a> {
    pub elems: Vec<Spanned<AstExpr<'a>>>,
    pub separator: ListSeparator,
    pub brackets: Brackets,
}

#[derive(Debug, Clone)]
pub struct FunctionCallExpr<'a> {
    pub namespace: Option<Spanned<Identifier>>,
    pub name: Identifier,
    /// Original function name before underscore→dash normalization.
    /// Used for plain CSS function output to preserve the original casing/underscores.
    pub original_name: CompactString,
    pub arguments: &'a ArgumentInvocation<'a>,
    pub span: Span,
    /// True if the function name was written with literal `--` prefix (CSS custom function).
    /// False if it was written with `__` (Sass function that normalizes to `--`).
    pub is_css_custom_function: bool,
}

#[derive(Debug, Clone)]
pub struct InterpolatedFunction<'a> {
    pub name: Interpolation<'a>,
    pub arguments: ArgumentInvocation<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct AstSassMap<'a>(pub Vec<(Spanned<AstExpr<'a>>, AstExpr<'a>)>);

#[derive(Debug, Clone)]
pub struct BinaryOpExpr<'a> {
    pub lhs: AstExpr<'a>,
    pub op: BinaryOp,
    pub rhs: AstExpr<'a>,
    pub allows_slash: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AstExpr<'a> {
    BinaryOp(&'a BinaryOpExpr<'a>),
    True,
    False,
    Calculation {
        name: CalculationName,
        args: Vec<Self>,
    },
    Color(Rc<Color>),
    CssIf(&'a CssIfExpression<'a>),
    FunctionCall(FunctionCallExpr<'a>),
    If(&'a Ternary<'a>),
    InterpolatedFunction(&'a InterpolatedFunction<'a>),
    List(ListExpr<'a>),
    Map(AstSassMap<'a>),
    Null,
    Number {
        n: Number,
        unit: Unit,
    },
    Paren(&'a AstExpr<'a>),
    ParentSelector,
    String(StringExpr<'a>, Span),
    Supports(&'a AstSupportsCondition<'a>),
    UnaryOp(UnaryOp, &'a AstExpr<'a>, Span),
    Variable {
        name: Spanned<Identifier>,
        namespace: Option<Spanned<Identifier>>,
    },
}

// todo: make quotes bool
// todo: track span inside
#[derive(Debug, Clone)]
pub struct StringExpr<'a>(pub Interpolation<'a>, pub QuoteKind);

impl<'a> StringExpr<'a> {
    fn quote_inner_text(
        text: &str,
        quote: char,
        buffer: &mut Interpolation<'a>,
        // default=false
        is_static: bool,
    ) {
        let mut chars = text.chars().peekable();
        while let Some(char) = chars.next() {
            if char == '\n' || char == '\r' {
                buffer.add_char('\\');
                buffer.add_char('a');
                if let Some(next) = chars.peek() {
                    if next.is_ascii_whitespace() || next.is_ascii_hexdigit() {
                        buffer.add_char(' ');
                    }
                }
            } else {
                if char == quote
                    || char == '\\'
                    || (is_static && char == '#' && chars.peek() == Some(&'{'))
                {
                    buffer.add_char('\\');
                }
                buffer.add_char(char);
            }
        }
    }

    fn best_quote<'b>(strings: impl Iterator<Item = &'b str>, preferred: Option<char>) -> char {
        let mut contains_double_quote = false;
        for s in strings {
            for c in s.chars() {
                if c == '\'' {
                    return '"';
                }
                if c == '"' {
                    contains_double_quote = true;
                }
            }
        }
        if contains_double_quote {
            '\''
        } else {
            preferred.unwrap_or('"')
        }
    }

    pub fn as_interpolation(
        self,
        is_static: bool,
        preferred_quote: Option<char>,
    ) -> Interpolation<'a> {
        if self.1 == QuoteKind::None {
            return self.0;
        }

        let quote = Self::best_quote(
            self.0.contents.iter().filter_map(|c| match c {
                InterpolationPart::Expr(..) => None,
                InterpolationPart::String(text) => Some(text.as_str()),
            }),
            preferred_quote,
        );

        let mut buffer = Interpolation::new();
        buffer.add_char(quote);

        for value in self.0.contents {
            match value {
                InterpolationPart::Expr(e) => buffer.add_expr(e),
                InterpolationPart::String(text) => {
                    Self::quote_inner_text(&text, quote, &mut buffer, is_static);
                }
            }
        }

        buffer.add_char(quote);

        buffer
    }
}

impl<'a> AstExpr<'a> {
    pub fn is_variable(&self) -> bool {
        matches!(self, Self::Variable { .. })
    }

    pub fn is_slash_operand(&self) -> bool {
        match self {
            Self::Number { .. } => true,
            Self::Calculation { name, .. } => *name == CalculationName::Calc,
            Self::BinaryOp(binop) => binop.allows_slash,
            _ => false,
        }
    }

    pub fn slash(left: Self, right: Self, span: Span, arena: &'a bumpalo::Bump) -> Self {
        Self::BinaryOp(arena.alloc(BinaryOpExpr {
            lhs: left,
            op: BinaryOp::Div,
            rhs: right,
            allows_slash: true,
            span,
        }))
    }

    pub const fn span(self, span: Span) -> Spanned<Self> {
        Spanned { node: self, span }
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use std::mem::size_of;

    /// Verify AstExpr stays ≤ 64 bytes. If this fails, a new large variant
    /// was added without boxing — check which variant grew and box it.
    #[test]
    fn ast_expr_size() {
        assert!(
            size_of::<AstExpr>() <= 64,
            "AstExpr grew to {} bytes",
            size_of::<AstExpr>()
        );
    }
}
