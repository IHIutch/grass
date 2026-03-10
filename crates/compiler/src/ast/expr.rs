use std::{iter::Iterator, sync::Arc};

use codemap::{Span, Spanned};

use crate::{
    color::Color,
    common::{BinaryOp, Brackets, Identifier, ListSeparator, QuoteKind, UnaryOp},
    unit::Unit,
    value::{CalculationName, Number},
};

use super::{ArgumentInvocation, AstSupportsCondition, Interpolation, InterpolationPart};

/// Represented by the legacy `if($condition, $if-true, $if-false)` function
#[derive(Debug, Clone)]
pub struct Ternary(pub ArgumentInvocation);

/// An atom in a CSS-native `if()` condition
#[derive(Debug, Clone)]
pub enum IfConditionAtom {
    /// `sass(expr)` — evaluable at compile time
    Sass(AstExpr, Span),
    /// `css(...)`, `var(...)`, `attr(...)`, or any other CSS function — raw passthrough
    /// Contains an Interpolation for the full text (including function name and parens)
    Css(Interpolation, Span),
    /// Like Css but formed from adjacent raw substitutions (var, attr, if, interp).
    /// Cannot coexist with sass() in the same condition.
    CssRaw(Interpolation, Span),
    /// `#{...}` interpolation — evaluated then treated as raw CSS
    Interp(AstExpr, Span),
}

/// A boolean condition in a CSS-native `if()` expression
#[derive(Debug, Clone)]
pub enum IfCondition {
    Atom(IfConditionAtom),
    Not(Box<IfCondition>, Span),
    And(Vec<IfCondition>),
    Or(Vec<IfCondition>),
    Paren(Box<IfCondition>),
    /// The `else` keyword — always true
    Else,
}

/// A single clause in a CSS-native `if()`: `condition: value`
#[derive(Debug, Clone)]
pub struct IfClause {
    pub condition: IfCondition,
    pub value: AstExpr,
}

/// CSS-native `if()` expression with condition clauses
#[derive(Debug, Clone)]
pub struct CssIfExpression {
    pub clauses: Vec<IfClause>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ListExpr {
    pub elems: Vec<Spanned<AstExpr>>,
    pub separator: ListSeparator,
    pub brackets: Brackets,
}

#[derive(Debug, Clone)]
pub struct FunctionCallExpr {
    pub namespace: Option<Spanned<Identifier>>,
    pub name: Identifier,
    /// Original function name before underscore→dash normalization.
    /// Used for plain CSS function output to preserve the original casing/underscores.
    pub original_name: String,
    pub arguments: Arc<ArgumentInvocation>,
    pub span: Span,
    /// True if the function name was written with literal `--` prefix (CSS custom function).
    /// False if it was written with `__` (Sass function that normalizes to `--`).
    pub is_css_custom_function: bool,
}

#[derive(Debug, Clone)]
pub struct InterpolatedFunction {
    pub name: Interpolation,
    pub arguments: ArgumentInvocation,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct AstSassMap(pub Vec<(Spanned<AstExpr>, AstExpr)>);

#[derive(Debug, Clone)]
pub struct BinaryOpExpr {
    pub lhs: AstExpr,
    pub op: BinaryOp,
    pub rhs: AstExpr,
    pub allows_slash: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AstExpr {
    BinaryOp(Arc<BinaryOpExpr>),
    True,
    False,
    Calculation {
        name: CalculationName,
        args: Vec<Self>,
    },
    Color(Arc<Color>),
    CssIf(Arc<CssIfExpression>),
    FunctionCall(FunctionCallExpr),
    If(Arc<Ternary>),
    InterpolatedFunction(Arc<InterpolatedFunction>),
    List(ListExpr),
    Map(AstSassMap),
    Null,
    Number {
        n: Number,
        unit: Unit,
    },
    Paren(Arc<Self>),
    ParentSelector,
    String(StringExpr, Span),
    Supports(Arc<AstSupportsCondition>),
    UnaryOp(UnaryOp, Arc<Self>, Span),
    Variable {
        name: Spanned<Identifier>,
        namespace: Option<Spanned<Identifier>>,
    },
}

// todo: make quotes bool
// todo: track span inside
#[derive(Debug, Clone)]
pub struct StringExpr(pub Interpolation, pub QuoteKind);

impl StringExpr {
    fn quote_inner_text(
        text: &str,
        quote: char,
        buffer: &mut Interpolation,
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

    fn best_quote<'a>(strings: impl Iterator<Item = &'a str>) -> char {
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
            '"'
        }
    }

    pub fn as_interpolation(self, is_static: bool) -> Interpolation {
        if self.1 == QuoteKind::None {
            return self.0;
        }

        let quote = Self::best_quote(self.0.contents.iter().filter_map(|c| match c {
            InterpolationPart::Expr(..) => None,
            InterpolationPart::String(text) => Some(text.as_str()),
        }));

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

impl AstExpr {
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

    pub fn slash(left: Self, right: Self, span: Span) -> Self {
        Self::BinaryOp(Arc::new(BinaryOpExpr {
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
