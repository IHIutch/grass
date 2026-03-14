use std::{iter::Iterator, marker::PhantomData, rc::Rc};

use codemap::Spanned;
use compact_str::CompactString;

use crate::{
    ast::*,
    color::{Color, ColorFormat, NAMED_COLORS},
    common::{unvendor, BinaryOp, Brackets, Identifier, ListSeparator, QuoteKind, UnaryOp},
    error::SassResult,
    unit::Unit,
    utils::{as_hex, opposite_bracket},
    value::{CalculationName, Number},
    ContextFlags, Token,
};

use super::StylesheetParser;

pub(crate) type Predicate<'c, P> = &'c dyn Fn(&mut P) -> SassResult<bool>;

fn is_hex_color(interpolation: &Interpolation) -> bool {
    if let Some(plain) = interpolation.as_plain() {
        if ![3, 4, 6, 8].contains(&plain.len()) {
            return false;
        }

        return plain.chars().all(|c| c.is_ascii_hexdigit());
    }

    false
}

pub(crate) struct ValueParser<'a, 'c, P: StylesheetParser<'a>> {
    comma_expressions: Option<Vec<Spanned<AstExpr>>>,
    space_expressions: Option<Vec<Spanned<AstExpr>>>,
    binary_operators: Option<Vec<BinaryOp>>,
    operands: Option<Vec<Spanned<AstExpr>>>,
    allow_slash: bool,
    single_expression: Option<Spanned<AstExpr>>,
    start: usize,
    inside_bracketed_list: bool,
    single_equals: bool,
    parse_until: Option<Predicate<'c, P>>,
    was_consuming_newlines: bool,
    _a: PhantomData<&'a ()>,
}

impl<'a, 'c, P: StylesheetParser<'a>> ValueParser<'a, 'c, P> {
    pub fn parse_expression(
        parser: &mut P,
        parse_until: Option<Predicate<'c, P>>,
        inside_bracketed_list: bool,
        single_equals: bool,
    ) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        let mut value_parser = Self::new(parser, parse_until, inside_bracketed_list, single_equals);

        if let Some(parse_until) = value_parser.parse_until {
            if parse_until(parser)? {
                return Err(("Expected expression.", parser.toks().current_span()).into());
            }
        }

        value_parser.was_consuming_newlines = parser.is_consuming_newlines();
        if value_parser.inside_bracketed_list {
            let bracket_start = parser.toks().cursor();

            parser.expect_char('[')?;
            parser.set_consume_newlines(true);
            parser.whitespace()?;

            if parser.scan_char(']') {
                parser.set_consume_newlines(value_parser.was_consuming_newlines);
                return Ok(AstExpr::List(ListExpr {
                    elems: Vec::new(),
                    separator: ListSeparator::Undecided,
                    brackets: Brackets::Bracketed,
                })
                .span(parser.toks_mut().span_from(bracket_start)));
            }
        };

        value_parser.start = parser.toks().cursor();

        value_parser.single_expression = Some(value_parser.parse_single_expression(parser)?);

        let mut value = value_parser.parse_value(parser)?;
        value.span = parser.toks_mut().span_from(start);

        Ok(value)
    }

    pub fn new(
        parser: &mut P,
        parse_until: Option<Predicate<'c, P>>,
        inside_bracketed_list: bool,
        single_equals: bool,
    ) -> Self {
        Self {
            comma_expressions: None,
            space_expressions: None,
            binary_operators: None,
            operands: None,
            allow_slash: true,
            start: parser.toks().cursor(),
            single_expression: None,
            parse_until,
            inside_bracketed_list,
            single_equals,
            was_consuming_newlines: false,
            _a: PhantomData,
        }
    }

    /// Parse a value from a stream of tokens
    ///
    /// This function will cease parsing if the predicate returns true.
    pub(crate) fn parse_value(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        parser.whitespace()?;

        let start = parser.toks().cursor();

        let was_in_parens = parser.flags().in_parens();

        loop {
            parser.whitespace()?;

            if let Some(parse_until) = self.parse_until {
                if parse_until(parser)? {
                    break;
                }
            }

            let first = parser.toks().peek();

            match first {
                Some(Token { kind: '(', .. }) => {
                    let expr = self.parse_paren_expr(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '[', .. }) => {
                    let expr = parser.parse_expression(None, Some(true), None)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '$', .. }) => {
                    let expr = Self::parse_variable(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '&', .. }) => {
                    let expr = Self::parse_selector(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '"', .. }) | Some(Token { kind: '\'', .. }) => {
                    let expr = parser
                        .parse_interpolated_string()?
                        .map_node(|s| AstExpr::String(s, parser.toks_mut().span_from(start)));
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '#', .. }) => {
                    let expr = self.parse_hash(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '=', .. }) => {
                    parser.toks_mut().next();
                    if self.single_equals
                        && !matches!(parser.toks().peek(), Some(Token { kind: '=', .. }))
                    {
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::SingleEq,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    } else {
                        parser.expect_char('=')?;
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Equal,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    }
                }
                Some(Token { kind: '!', .. }) => match parser.toks().peek_n(1) {
                    Some(Token { kind: '=', .. }) => {
                        parser.toks_mut().next();
                        parser.toks_mut().next();
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::NotEqual,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    }
                    Some(Token { kind, .. })
                        if kind.is_ascii_whitespace() || kind == 'i' || kind == 'I' =>
                    {
                        let expr = Self::parse_important_expr(parser)?;
                        self.add_single_expression(expr, parser)?;
                    }
                    None => {
                        let expr = Self::parse_important_expr(parser)?;
                        self.add_single_expression(expr, parser)?;
                    }
                    Some(..) => break,
                },
                Some(Token { kind: '<', .. }) => {
                    parser.toks_mut().next();
                    self.add_operator(
                        Spanned {
                            node: if parser.scan_char('=') {
                                BinaryOp::LessThanEqual
                            } else {
                                BinaryOp::LessThan
                            },
                            span: parser.toks_mut().span_from(start),
                        },
                        parser,
                    )?;
                }
                Some(Token { kind: '>', .. }) => {
                    parser.toks_mut().next();
                    self.add_operator(
                        Spanned {
                            node: if parser.scan_char('=') {
                                BinaryOp::GreaterThanEqual
                            } else {
                                BinaryOp::GreaterThan
                            },
                            span: parser.toks_mut().span_from(start),
                        },
                        parser,
                    )?;
                }
                Some(Token { kind: '*', .. }) => {
                    parser.toks_mut().next();
                    self.add_operator(
                        Spanned {
                            node: BinaryOp::Mul,
                            span: parser.toks().current_span(),
                        },
                        parser,
                    )?;
                }
                Some(Token { kind: '+', .. }) => {
                    if self.single_expression.is_none() {
                        let expr = self.parse_unary_operation(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else {
                        parser.toks_mut().next();
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Plus,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    }
                }
                Some(Token { kind: '-', .. }) => {
                    if matches!(
                        parser.toks().peek_n(1),
                        Some(Token {
                            kind: '0'..='9' | '.',
                            ..
                        })
                    ) && (self.single_expression.is_none()
                        || matches!(
                            parser.toks_mut().peek_previous(),
                            Some(Token {
                                kind: ' ' | '\t' | '\n' | '\r',
                                ..
                            })
                        ))
                    {
                        let expr = ValueParser::parse_number(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else if parser.looking_at_interpolated_identifier() {
                        let expr = self.parse_identifier_like(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else if self.single_expression.is_none() {
                        let expr = self.parse_unary_operation(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else {
                        parser.toks_mut().next();
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Minus,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    }
                }
                Some(Token { kind: '/', .. }) => {
                    if self.single_expression.is_none() {
                        let expr = self.parse_unary_operation(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else {
                        parser.toks_mut().next();
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Div,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    }
                }
                Some(Token { kind: '%', .. }) => {
                    // Check if % is followed (past whitespace) by a valid
                    // expression start, indicating it's a binary mod operator.
                    // If not, treat it as a standalone CSS value.
                    let mut n = 1;
                    while matches!(parser.toks().peek_n(n), Some(Token { kind: ' ' | '\t' | '\n' | '\r', .. })) {
                        n += 1;
                    }
                    let is_binary_op = match parser.toks().peek_n(n) {
                        Some(Token { kind: '0'..='9' | '.' | '(' | '$' | '#' | '"' | '\'' | '-' | '+', .. }) => true,
                        Some(Token { kind: 'a'..='z' | 'A'..='Z' | '_' | '\\', .. }) => true,
                        Some(Token { kind: c, .. }) if c > '\u{7f}' => true,
                        _ => false,
                    };

                    if is_binary_op {
                        parser.toks_mut().next();
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Rem,
                                span: parser.toks().current_span(),
                            },
                            parser,
                        )?;
                    } else {
                        let expr_start = parser.toks().cursor();
                        parser.toks_mut().next();
                        let span = parser.toks_mut().span_from(expr_start);
                        let expr = AstExpr::String(
                            StringExpr(Interpolation::new_plain("%".to_owned()), QuoteKind::None),
                            span,
                        )
                        .span(span);
                        self.add_single_expression(expr, parser)?;
                    }
                }
                Some(Token {
                    kind: '0'..='9', ..
                }) => {
                    let expr = ValueParser::parse_number(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: '.', .. }) => {
                    if matches!(parser.toks().peek_n(1), Some(Token { kind: '.', .. })) {
                        break;
                    }
                    let expr = ValueParser::parse_number(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: 'a', .. }) => {
                    if !parser.is_plain_css() && parser.scan_identifier("and", false)? {
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::And,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    } else {
                        let expr = self.parse_identifier_like(parser)?;
                        self.add_single_expression(expr, parser)?;
                    }
                }
                Some(Token { kind: 'o', .. }) => {
                    if !parser.is_plain_css() && parser.scan_identifier("or", false)? {
                        self.add_operator(
                            Spanned {
                                node: BinaryOp::Or,
                                span: parser.toks_mut().span_from(start),
                            },
                            parser,
                        )?;
                    } else {
                        let expr = self.parse_identifier_like(parser)?;
                        self.add_single_expression(expr, parser)?;
                    }
                }
                Some(Token { kind: 'u', .. }) | Some(Token { kind: 'U', .. }) => {
                    if matches!(parser.toks().peek_n(1), Some(Token { kind: '+', .. })) {
                        let expr = Self::parse_unicode_range(parser)?;
                        self.add_single_expression(expr, parser)?;
                    } else {
                        let expr = self.parse_identifier_like(parser)?;
                        self.add_single_expression(expr, parser)?;
                    }
                }
                Some(Token {
                    kind: 'b'..='z', ..
                })
                | Some(Token {
                    kind: 'A'..='Z', ..
                })
                | Some(Token { kind: '_', .. })
                | Some(Token { kind: '\\', .. })
                | Some(Token {
                    kind: '\u{80}'..=std::char::MAX,
                    ..
                }) => {
                    let expr = self.parse_identifier_like(parser)?;
                    self.add_single_expression(expr, parser)?;
                }
                Some(Token { kind: ',', .. }) => {
                    // If we discover we're parsing a list whose first element is a
                    // division operation, and we're in parentheses, reparse outside of a
                    // paren context. This ensures that `(1/2, 1)` doesn't perform division
                    // on its first element.
                    if parser.flags().in_parens() {
                        parser.flags_mut().set(ContextFlags::IN_PARENS, false);
                        if self.allow_slash {
                            self.reset_state(parser)?;
                            continue;
                        }
                        // todo: does this branch ever get hit
                    }

                    if self.single_expression.is_none() {
                        return Err(("Expected expression.", parser.toks().current_span()).into());
                    }

                    self.resolve_space_expressions(parser)?;

                    // [resolveSpaceExpressions] can modify [singleExpression_], but it
                    // can't set it to null`.
                    self.comma_expressions
                        .get_or_insert_with(Default::default)
                        .push(self.single_expression.take().unwrap());
                    parser.toks_mut().next();
                    self.allow_slash = true;
                }
                Some(..) | None => break,
            }
        }

        if self.inside_bracketed_list {
            parser.expect_char(']')?;
            parser.set_consume_newlines(self.was_consuming_newlines);
        }

        if self.comma_expressions.is_some() {
            self.resolve_space_expressions(parser)?;

            parser
                .flags_mut()
                .set(ContextFlags::IN_PARENS, was_in_parens);

            if let Some(single_expression) = self.single_expression.take() {
                self.comma_expressions
                    .as_mut()
                    .unwrap()
                    .push(single_expression);
            }

            Ok(AstExpr::List(ListExpr {
                elems: self.comma_expressions.take().unwrap(),
                separator: ListSeparator::Comma,
                brackets: if self.inside_bracketed_list {
                    Brackets::Bracketed
                } else {
                    Brackets::None
                },
            })
            .span(parser.toks_mut().span_from(start)))
        } else if self.inside_bracketed_list && self.space_expressions.is_some() {
            self.resolve_operations(parser)?;

            self.space_expressions
                .as_mut()
                .unwrap()
                .push(self.single_expression.take().unwrap());

            Ok(AstExpr::List(ListExpr {
                elems: self.space_expressions.take().unwrap(),
                separator: ListSeparator::Space,
                brackets: Brackets::Bracketed,
            })
            .span(parser.toks_mut().span_from(start)))
        } else {
            self.resolve_space_expressions(parser)?;

            if self.inside_bracketed_list {
                return Ok(AstExpr::List(ListExpr {
                    elems: vec![self.single_expression.take().unwrap()],
                    separator: ListSeparator::Undecided,
                    brackets: Brackets::Bracketed,
                })
                .span(parser.toks_mut().span_from(start)));
            }

            Ok(self.single_expression.take().unwrap())
        }
    }

    fn parse_single_expression(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        let first = parser.toks().peek();

        match first {
            Some(Token { kind: '(', .. }) => self.parse_paren_expr(parser),
            Some(Token { kind: '/', .. }) => self.parse_unary_operation(parser),
            Some(Token { kind: '[', .. }) => Self::parse_expression(parser, None, true, false),
            Some(Token { kind: '$', .. }) => Self::parse_variable(parser),
            Some(Token { kind: '&', .. }) => Self::parse_selector(parser),
            Some(Token { kind: '"', .. }) | Some(Token { kind: '\'', .. }) => Ok(parser
                .parse_interpolated_string()?
                .map_node(|s| AstExpr::String(s, parser.toks_mut().span_from(start)))),
            Some(Token { kind: '#', .. }) => self.parse_hash(parser),
            Some(Token { kind: '+', .. }) => self.parse_plus_expr(parser),
            Some(Token { kind: '-', .. }) => self.parse_minus_expr(parser),
            Some(Token { kind: '!', .. }) => Self::parse_important_expr(parser),
            Some(Token { kind: 'u', .. }) | Some(Token { kind: 'U', .. }) => {
                if matches!(parser.toks().peek_n(1), Some(Token { kind: '+', .. })) {
                    Self::parse_unicode_range(parser)
                } else {
                    self.parse_identifier_like(parser)
                }
            }
            Some(Token {
                kind: '0'..='9', ..
            })
            | Some(Token { kind: '.', .. }) => ValueParser::parse_number(parser),
            Some(Token {
                kind: 'a'..='z', ..
            })
            | Some(Token {
                kind: 'A'..='Z', ..
            })
            | Some(Token { kind: '_', .. })
            | Some(Token { kind: '\\', .. })
            | Some(Token {
                kind: '\u{80}'..=std::char::MAX,
                ..
            }) => self.parse_identifier_like(parser),
            Some(Token { kind: '%', .. }) => {
                parser.toks_mut().next();
                let span = parser.toks_mut().span_from(start);
                Ok(AstExpr::String(
                    StringExpr(Interpolation::new_plain("%".to_owned()), QuoteKind::None),
                    span,
                )
                .span(span))
            }
            Some(..) | None => Err((
                "Expected expression.",
                parser.toks_mut().span_from(self.start),
            )
                .into()),
        }
    }

    fn resolve_one_operation(&mut self, parser: &mut P) -> SassResult<()> {
        let operator = self.binary_operators.as_mut().unwrap().pop().unwrap();
        let operands = self.operands.as_mut().unwrap();

        let left = operands.pop().unwrap();
        let right = match self.single_expression.take() {
            Some(val) => val,
            None => return Err(("Expected expression.", left.span).into()),
        };

        let span = left.span.merge(right.span);

        if self.allow_slash
            && !parser.flags().in_parens()
            && operator == BinaryOp::Div
            && left.node.is_slash_operand()
            && right.node.is_slash_operand()
        {
            self.single_expression = Some(AstExpr::slash(left.node, right.node, span).span(span));
        } else {
            self.single_expression = Some(
                AstExpr::BinaryOp(Rc::new(BinaryOpExpr {
                    lhs: left.node,
                    op: operator,
                    rhs: right.node,
                    allows_slash: false,
                    span,
                }))
                .span(span),
            );
            self.allow_slash = false;
        }

        Ok(())
    }

    fn resolve_operations(&mut self, parser: &mut P) -> SassResult<()> {
        loop {
            let should_break = match self.binary_operators.as_ref() {
                Some(bin) => bin.is_empty(),
                None => true,
            };

            if should_break {
                break;
            }

            self.resolve_one_operation(parser)?;
        }

        Ok(())
    }

    fn add_single_expression(
        &mut self,
        expression: Spanned<AstExpr>,
        parser: &mut P,
    ) -> SassResult<()> {
        if self.single_expression.is_some() {
            // If we discover we're parsing a list whose first element is a division
            // operation, and we're in parentheses, reparse outside of a paren
            // context. This ensures that `(1/2 1)` doesn't perform division on its
            // first element.
            if parser.flags().in_parens() {
                parser.flags_mut().set(ContextFlags::IN_PARENS, false);

                if self.allow_slash {
                    self.reset_state(parser)?;

                    return Ok(());
                }
            }

            if self.space_expressions.is_none() {
                self.space_expressions = Some(Vec::new());
            }

            self.resolve_operations(parser)?;

            self.space_expressions
                .as_mut()
                .unwrap()
                .push(self.single_expression.take().unwrap());

            self.allow_slash = true;
        }

        self.single_expression = Some(expression);

        Ok(())
    }

    fn add_operator(&mut self, op: Spanned<BinaryOp>, parser: &mut P) -> SassResult<()> {
        if parser.is_plain_css() && op.node != BinaryOp::Div && op.node != BinaryOp::SingleEq {
            return Err(("Operators aren't allowed in plain CSS.", op.span).into());
        }

        self.allow_slash = self.allow_slash && op.node == BinaryOp::Div;

        if self.binary_operators.is_none() {
            self.binary_operators = Some(Vec::new());
        }

        if self.operands.is_none() {
            self.operands = Some(Vec::new());
        }

        while let Some(last_op) = self.binary_operators.as_ref().unwrap_or(&Vec::new()).last() {
            if last_op.precedence() < op.precedence() {
                break;
            }

            self.resolve_one_operation(parser)?;
        }
        self.binary_operators
            .get_or_insert_with(Default::default)
            .push(op.node);

        match self.single_expression.take() {
            Some(expr) => {
                self.operands.get_or_insert_with(Vec::new).push(expr);
            }
            None => return Err(("Expected expression.", op.span).into()),
        }

        // In indented syntax, allow newlines after binary operators so
        // expressions can span multiple lines (e.g., `$a: b +\nc`).
        let temporarily_consume_newlines = parser.is_indented()
            && !parser.is_consuming_newlines()
            && matches!(
                parser.toks().peek(),
                Some(Token {
                    kind: '\n' | '\r',
                    ..
                })
            );

        if temporarily_consume_newlines {
            parser.set_consume_newlines(true);
        }

        parser.whitespace()?;

        self.single_expression = Some(self.parse_single_expression(parser)?);

        if temporarily_consume_newlines {
            parser.set_consume_newlines(false);
        }

        Ok(())
    }

    fn resolve_space_expressions(&mut self, parser: &mut P) -> SassResult<()> {
        self.resolve_operations(parser)?;

        if let Some(mut space_expressions) = self.space_expressions.take() {
            let single_expression = match self.single_expression.take() {
                Some(val) => val,
                None => return Err(("Expected expression.", parser.toks().current_span()).into()),
            };

            let span = single_expression.span;

            space_expressions.push(single_expression);

            self.single_expression = Some(
                AstExpr::List(ListExpr {
                    elems: space_expressions,
                    separator: ListSeparator::Space,
                    brackets: Brackets::None,
                })
                .span(span),
            );
        }

        Ok(())
    }

    fn parse_map(
        parser: &mut P,
        first: Spanned<AstExpr>,
        start: usize,
        restore_consume_newlines: bool,
    ) -> SassResult<Spanned<AstExpr>> {
        let mut pairs = vec![(first, parser.parse_expression_until_comma(false)?.node)];

        while parser.scan_char(',') {
            parser.whitespace()?;
            if !parser.looking_at_expression() {
                break;
            }

            let key = parser.parse_expression_until_comma(false)?;
            parser.expect_char(':')?;
            parser.whitespace()?;
            let value = parser.parse_expression_until_comma(false)?;
            pairs.push((key, value.node));
        }

        parser.expect_char(')')?;
        parser.set_consume_newlines(restore_consume_newlines);

        Ok(AstExpr::Map(AstSassMap(pairs)).span(parser.toks_mut().span_from(start)))
    }

    fn parse_paren_expr(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        if parser.is_plain_css() {
            return Err((
                "Parentheses aren't allowed in plain CSS.",
                parser.toks().current_span(),
            )
                .into());
        }

        let was_in_parentheses = parser.flags().in_parens();
        parser.flags_mut().set(ContextFlags::IN_PARENS, true);
        let was_consuming_newlines = parser.is_consuming_newlines();

        parser.expect_char('(')?;
        parser.set_consume_newlines(true);
        parser.whitespace()?;
        if !parser.looking_at_expression() {
            parser.expect_char(')')?;
            parser.set_consume_newlines(was_consuming_newlines);
            parser
                .flags_mut()
                .set(ContextFlags::IN_PARENS, was_in_parentheses);
            return Ok(AstExpr::List(ListExpr {
                elems: Vec::new(),
                separator: ListSeparator::Undecided,
                brackets: Brackets::None,
            })
            .span(parser.toks_mut().span_from(start)));
        }

        let first = parser.parse_expression_until_comma(false)?;
        if parser.scan_char(':') {
            parser.whitespace()?;
            parser
                .flags_mut()
                .set(ContextFlags::IN_PARENS, was_in_parentheses);
            return Self::parse_map(parser, first, start, was_consuming_newlines);
        }

        if !parser.scan_char(',') {
            parser.expect_char(')')?;
            parser.set_consume_newlines(was_consuming_newlines);
            parser
                .flags_mut()
                .set(ContextFlags::IN_PARENS, was_in_parentheses);
            return Ok(AstExpr::Paren(Rc::new(first.node)).span(first.span));
        }

        parser.whitespace()?;

        let mut expressions = vec![first];

        loop {
            if !parser.looking_at_expression() {
                break;
            }
            expressions.push(parser.parse_expression_until_comma(false)?);
            if !parser.scan_char(',') {
                break;
            }
            parser.whitespace()?;
        }

        parser.expect_char(')')?;
        parser.set_consume_newlines(was_consuming_newlines);

        parser
            .flags_mut()
            .set(ContextFlags::IN_PARENS, was_in_parentheses);

        Ok(AstExpr::List(ListExpr {
            elems: expressions,
            separator: ListSeparator::Comma,
            brackets: Brackets::None,
        })
        .span(parser.toks_mut().span_from(start)))
    }

    fn parse_variable(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        let name = parser.parse_variable_name()?;

        if parser.is_plain_css() {
            return Err((
                "Sass variables aren't allowed in plain CSS.",
                parser.toks_mut().span_from(start),
            )
                .into());
        }

        Ok(AstExpr::Variable {
            name: Spanned {
                node: Identifier::from(name),
                span: parser.toks_mut().span_from(start),
            },
            namespace: None,
        }
        .span(parser.toks_mut().span_from(start)))
    }

    fn parse_selector(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        if parser.is_plain_css() {
            return Err((
                "The parent selector isn't allowed in plain CSS.",
                parser.toks().current_span(),
            )
                .into());
        }

        let start = parser.toks().cursor();

        parser.expect_char('&')?;

        if parser.toks().next_char_is('&') {
            // todo: emit a warning here
            //   warn(
            //       'In Sass, "&&" means two copies of the parent selector. You '
            //       'probably want to use "and" instead.',
            //       scanner.spanFrom(start));
            //   scanner.position--;
        }

        Ok(AstExpr::ParentSelector.span(parser.toks_mut().span_from(start)))
    }

    fn parse_hash(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        debug_assert!(matches!(
            parser.toks().peek(),
            Some(Token { kind: '#', .. })
        ));

        if matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. })) {
            return self.parse_identifier_like(parser);
        }

        parser.expect_char('#')?;

        if matches!(
            parser.toks().peek(),
            Some(Token {
                kind: '0'..='9',
                ..
            })
        ) {
            let color = self.parse_hex_color_contents(parser)?;
            return Ok(AstExpr::Color(Rc::new(color)).span(parser.toks_mut().span_from(start)));
        }

        let after_hash = parser.toks().cursor();
        let ident = parser.parse_interpolated_identifier()?;
        if is_hex_color(&ident) {
            parser.toks_mut().set_cursor(after_hash);
            let color = self.parse_hex_color_contents(parser)?;
            return Ok(
                AstExpr::Color(Rc::new(color)).span(parser.toks_mut().span_from(after_hash))
            );
        }

        let mut buffer = Interpolation::new();

        buffer.add_char('#');
        buffer.add_interpolation(ident);

        let span = parser.toks_mut().span_from(start);

        Ok(AstExpr::String(StringExpr(buffer, QuoteKind::None), span).span(span))
    }

    fn parse_hex_digit(&mut self, parser: &mut P) -> SassResult<u32> {
        match parser.toks().peek() {
            Some(Token { kind, .. }) if kind.is_ascii_hexdigit() => {
                parser.toks_mut().next();
                Ok(as_hex(kind))
            }
            _ => Err(("Expected hex digit.", parser.toks().current_span()).into()),
        }
    }

    fn parse_hex_color_contents(&mut self, parser: &mut P) -> SassResult<Color> {
        let start = parser.toks().cursor();

        let digit1 = self.parse_hex_digit(parser)?;
        let digit2 = self.parse_hex_digit(parser)?;
        let digit3 = self.parse_hex_digit(parser)?;

        let red: u32;
        let green: u32;
        let blue: u32;
        let mut alpha: f64 = 1.0;

        if parser.next_is_hex() {
            let digit4 = self.parse_hex_digit(parser)?;

            if parser.next_is_hex() {
                red = (digit1 << 4) + digit2;
                green = (digit3 << 4) + digit4;
                blue = (self.parse_hex_digit(parser)? << 4) + self.parse_hex_digit(parser)?;

                if parser.next_is_hex() {
                    alpha = ((self.parse_hex_digit(parser)? << 4) + self.parse_hex_digit(parser)?)
                        as f64
                        / 0xff as f64;
                }
            } else {
                // #abcd
                red = (digit1 << 4) + digit1;
                green = (digit2 << 4) + digit2;
                blue = (digit3 << 4) + digit3;
                alpha = ((digit4 << 4) + digit4) as f64 / 0xff as f64;
            }
        } else {
            // #abc
            red = (digit1 << 4) + digit1;
            green = (digit2 << 4) + digit2;
            blue = (digit3 << 4) + digit3;
        }

        // Don't emit four- or eight-digit hex colors as hex, since that's not
        // yet well-supported in browsers.
        let format = if alpha == 1.0 {
            ColorFormat::Literal(parser.toks_mut().raw_text(start - 1))
        } else {
            let _ = parser.toks_mut().raw_text(start - 1);
            ColorFormat::Infer
        };

        Ok(Color::new_rgba(
            Number::from(red),
            Number::from(green),
            Number::from(blue),
            Number(alpha),
            format,
        ))
    }

    fn parse_unary_operation(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let op_span = parser.toks().current_span();
        let operator = Self::expect_unary_operator(parser)?;

        if parser.is_plain_css() && operator != UnaryOp::Div {
            return Err(("Operators aren't allowed in plain CSS.", op_span).into());
        }

        parser.whitespace()?;

        let operand = self.parse_single_expression(parser)?;

        let span = op_span.merge(parser.toks().current_span());

        Ok(AstExpr::UnaryOp(operator, Rc::new(operand.node), span).span(span))
    }

    fn expect_unary_operator(parser: &mut P) -> SassResult<UnaryOp> {
        let span = parser.toks().current_span();
        Ok(match parser.toks_mut().next() {
            Some(Token { kind: '+', .. }) => UnaryOp::Plus,
            Some(Token { kind: '-', .. }) => UnaryOp::Neg,
            Some(Token { kind: '/', .. }) => UnaryOp::Div,
            Some(..) | None => return Err(("Expected unary operator.", span).into()),
        })
    }

    fn consume_natural_number(parser: &mut P) -> SassResult<()> {
        if !matches!(
            parser.toks_mut().next(),
            Some(Token {
                kind: '0'..='9',
                ..
            })
        ) {
            return Err(("Expected digit.", parser.toks().prev_span()).into());
        }

        while matches!(
            parser.toks().peek(),
            Some(Token {
                kind: '0'..='9',
                ..
            })
        ) {
            parser.toks_mut().next();
        }

        Ok(())
    }

    fn parse_number(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();

        if !parser.scan_char('+') {
            parser.scan_char('-');
        }

        let after_sign = parser.toks().cursor();

        if !parser.toks().next_char_is('.') {
            ValueParser::consume_natural_number(parser)?;
        }

        ValueParser::try_decimal(parser, parser.toks().cursor() != after_sign)?;
        ValueParser::try_exponent(parser)?;

        let number: f64 = parser.toks_mut().raw_text(start).parse().unwrap();

        let unit = if parser.scan_char('%') {
            Unit::Percent
        } else if parser.looking_at_identifier()
            && (!matches!(parser.toks().peek(), Some(Token { kind: '-', .. }))
                || !matches!(parser.toks().peek_n(1), Some(Token { kind: '-', .. })))
        {
            Unit::from(parser.parse_identifier(false, true)?)
        } else {
            Unit::None
        };

        Ok(AstExpr::Number {
            n: Number::from(number),
            unit,
        }
        .span(parser.toks_mut().span_from(start)))
    }

    fn try_decimal(parser: &mut P, allow_trailing_dot: bool) -> SassResult<Option<String>> {
        if !matches!(parser.toks().peek(), Some(Token { kind: '.', .. })) {
            return Ok(None);
        }

        match parser.toks().peek_n(1) {
            Some(Token { kind, .. }) if !kind.is_ascii_digit() => {
                if allow_trailing_dot {
                    return Ok(None);
                }

                return Err(("Expected digit.", parser.toks().current_span()).into());
            }
            Some(..) => {}
            None => return Err(("Expected digit.", parser.toks().current_span()).into()),
        }

        let mut buffer = String::new();

        parser.expect_char('.')?;
        buffer.push('.');

        while let Some(Token { kind, .. }) = parser.toks().peek() {
            if !kind.is_ascii_digit() {
                break;
            }
            buffer.push(kind);
            parser.toks_mut().next();
        }

        Ok(Some(buffer))
    }

    fn try_exponent(parser: &mut P) -> SassResult<Option<String>> {
        let mut buffer = String::new();

        match parser.toks().peek() {
            Some(Token {
                kind: 'e' | 'E', ..
            }) => buffer.push('e'),
            _ => return Ok(None),
        }

        let next = match parser.toks().peek_n(1) {
            Some(Token {
                kind: kind @ ('0'..='9' | '-' | '+'),
                ..
            }) => kind,
            _ => return Ok(None),
        };

        parser.toks_mut().next();

        if next == '+' || next == '-' {
            parser.toks_mut().next();
            buffer.push(next);
        }

        match parser.toks().peek() {
            Some(Token {
                kind: '0'..='9', ..
            }) => {}
            _ => return Err(("Expected digit.", parser.toks().current_span()).into()),
        }

        while let Some(tok) = parser.toks().peek() {
            if !tok.kind.is_ascii_digit() {
                break;
            }

            buffer.push(tok.kind);

            parser.toks_mut().next();
        }

        Ok(Some(buffer))
    }

    fn parse_plus_expr(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        debug_assert!(parser.toks().next_char_is('+'));

        match parser.toks().peek_n(1) {
            Some(Token {
                kind: '0'..='9' | '.',
                ..
            }) => ValueParser::parse_number(parser),
            _ => self.parse_unary_operation(parser),
        }
    }

    fn parse_minus_expr(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        debug_assert!(parser.toks().next_char_is('-'));

        if matches!(
            parser.toks().peek_n(1),
            Some(Token {
                kind: '0'..='9' | '.',
                ..
            })
        ) {
            return ValueParser::parse_number(parser);
        }

        if parser.looking_at_interpolated_identifier() {
            return self.parse_identifier_like(parser);
        }

        self.parse_unary_operation(parser)
    }

    fn parse_important_expr(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        parser.expect_char('!')?;
        let was_cn = parser.is_consuming_newlines();
        parser.set_consume_newlines(true);
        parser.whitespace()?;
        parser.set_consume_newlines(was_cn);
        parser.expect_identifier("important", false)?;

        let span = parser.toks_mut().span_from(start);

        Ok(AstExpr::String(
            StringExpr(
                Interpolation::new_plain("!important".to_owned()),
                QuoteKind::None,
            ),
            span,
        )
        .span(span))
    }

    fn parse_identifier_like(&mut self, parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        if let Some(func) = P::IDENTIFIER_LIKE {
            return func(parser);
        }

        let start = parser.toks().cursor();

        let identifier = parser.parse_interpolated_identifier()?;

        let ident_span = parser.toks_mut().span_from(start);

        let plain = identifier.as_plain();
        let lower = plain.map(str::to_ascii_lowercase);

        if let Some(plain) = plain {
            if plain == "if" && parser.toks().next_char_is('(') {
                // Try CSS-native if() syntax first
                if let Some(css_if) =
                    super::css_if::try_parse_css_if(parser, start)?
                {
                    return Ok(css_if);
                }
                // Fall back to legacy if($condition, $if-true, $if-false)
                let call_args = parser.parse_argument_invocation(false, false)?;
                let span = call_args.span;
                return Ok(AstExpr::If(Rc::new(Ternary(call_args))).span(span));
            } else if plain == "not" {
                // In indented syntax, allow newlines after `not` so expressions
                // can span multiple lines (e.g., `$a: not\nb`).
                let temporarily_consume_newlines = parser.is_indented()
                    && !parser.is_consuming_newlines()
                    && matches!(
                        parser.toks().peek(),
                        Some(Token {
                            kind: '\n' | '\r',
                            ..
                        })
                    );

                if temporarily_consume_newlines {
                    parser.set_consume_newlines(true);
                }

                parser.whitespace()?;

                let value = self.parse_single_expression(parser)?;

                if temporarily_consume_newlines {
                    parser.set_consume_newlines(false);
                }

                let span = parser.toks_mut().span_from(start);

                return Ok(AstExpr::UnaryOp(UnaryOp::Not, Rc::new(value.node), span).span(span));
            }

            let lower_ref = lower.as_ref().unwrap();

            if !parser.toks().next_char_is('(') {
                match plain {
                    "null" => return Ok(AstExpr::Null.span(parser.toks_mut().span_from(start))),
                    "true" => return Ok(AstExpr::True.span(parser.toks_mut().span_from(start))),
                    "false" => return Ok(AstExpr::False.span(parser.toks_mut().span_from(start))),
                    _ => {}
                }

                if let Some(color) = NAMED_COLORS.get_by_name(lower_ref.as_str()) {
                    return Ok(AstExpr::Color(Rc::new(Color::new(
                        color[0],
                        color[1],
                        color[2],
                        color[3],
                        plain.to_owned(),
                    )))
                    .span(parser.toks_mut().span_from(start)));
                }
            }

            if let Some(func) = ValueParser::try_parse_special_function(parser, lower_ref, start)? {
                return Ok(func);
            }
        }

        match parser.toks().peek() {
            Some(Token { kind: '.', .. }) => {
                if matches!(parser.toks().peek_n(1), Some(Token { kind: '.', .. })) {
                    return Ok(AstExpr::String(
                        StringExpr(identifier, QuoteKind::None),
                        parser.toks_mut().span_from(start),
                    )
                    .span(parser.toks_mut().span_from(start)));
                }
                parser.toks_mut().next();

                match plain {
                    Some(s) => Self::namespaced_expression(
                        Spanned {
                            node: Identifier::from(s),
                            span: ident_span,
                        },
                        start,
                        parser,
                    ),
                    None => Err(("Interpolation isn't allowed in namespaces.", ident_span).into()),
                }
            }
            Some(Token { kind: '(', .. }) => {
                if let Some(plain) = plain {
                    let arguments =
                        parser.parse_argument_invocation(false, lower.as_deref() == Some("var"))?;

                    let is_css_custom = plain.starts_with("--");
                    Ok(AstExpr::FunctionCall(FunctionCallExpr {
                        namespace: None,
                        name: Identifier::from(plain),
                        original_name: CompactString::from(plain),
                        arguments: Rc::new(arguments),
                        span: parser.toks_mut().span_from(start),
                        is_css_custom_function: is_css_custom,
                    })
                    .span(parser.toks_mut().span_from(start)))
                } else {
                    let arguments = parser.parse_argument_invocation(false, false)?;
                    Ok(
                        AstExpr::InterpolatedFunction(Rc::new(InterpolatedFunction {
                            name: identifier,
                            arguments,
                            span: parser.toks_mut().span_from(start),
                        }))
                        .span(parser.toks_mut().span_from(start)),
                    )
                }
            }
            _ => Ok(AstExpr::String(
                StringExpr(identifier, QuoteKind::None),
                parser.toks_mut().span_from(start),
            )
            .span(parser.toks_mut().span_from(start))),
        }
    }

    fn namespaced_expression(
        namespace: Spanned<Identifier>,
        start: usize,
        parser: &mut P,
    ) -> SassResult<Spanned<AstExpr>> {
        if parser.toks().next_char_is('$') {
            let name_start = parser.toks().cursor();
            let name = parser.parse_variable_name()?;
            let span = parser.toks_mut().span_from(start);
            P::assert_public(&name, span)?;

            if parser.is_plain_css() {
                return Err(("Module namespaces aren't allowed in plain CSS.", span).into());
            }

            return Ok(AstExpr::Variable {
                name: Spanned {
                    node: Identifier::from(name),
                    span: parser.toks_mut().span_from(name_start),
                },
                namespace: Some(namespace),
            }
            .span(span));
        }

        let name = parser.parse_public_identifier()?;
        let args = parser.parse_argument_invocation(false, false)?;
        let span = parser.toks_mut().span_from(start);

        if parser.is_plain_css() {
            return Err(("Module namespaces aren't allowed in plain CSS.", span).into());
        }

        Ok(AstExpr::FunctionCall(FunctionCallExpr {
            namespace: Some(namespace),
            name: Identifier::from(&name),
            original_name: CompactString::from(name),
            arguments: Rc::new(args),
            span,
            is_css_custom_function: false,
        })
        .span(span))
    }

    fn parse_unicode_range(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let start = parser.toks().cursor();
        parser.expect_ident_char('u', false)?;
        parser.expect_char('+')?;

        let mut first_range_length = 0;

        while let Some(next) = parser.toks().peek() {
            if !next.kind.is_ascii_hexdigit() {
                break;
            }

            parser.toks_mut().next();
            first_range_length += 1;
        }

        let mut has_question_mark = false;

        while parser.scan_char('?') {
            has_question_mark = true;
            first_range_length += 1;
        }

        let span = parser.toks_mut().span_from(start);
        if first_range_length == 0 {
            return Err(("Expected hex digit or \"?\".", parser.toks().current_span()).into());
        } else if first_range_length > 6 {
            return Err(("Expected at most 6 digits.", span).into());
        } else if has_question_mark {
            return Ok(AstExpr::String(
                StringExpr(
                    Interpolation::new_plain(parser.toks_mut().raw_text(start)),
                    QuoteKind::None,
                ),
                span,
            )
            .span(span));
        }

        if parser.scan_char('-') {
            let second_range_start = parser.toks().cursor();
            let mut second_range_length = 0;

            while let Some(next) = parser.toks().peek() {
                if !next.kind.is_ascii_hexdigit() {
                    break;
                }

                parser.toks_mut().next();
                second_range_length += 1;
            }

            if second_range_length == 0 {
                return Err(("Expected hex digit.", parser.toks().current_span()).into());
            } else if second_range_length > 6 {
                return Err((
                    "Expected at most 6 digits.",
                    parser.toks_mut().span_from(second_range_start),
                )
                    .into());
            }
        }

        if parser.looking_at_interpolated_identifier_body() {
            return Err(("Expected end of identifier.", parser.toks().current_span()).into());
        }

        let span = parser.toks_mut().span_from(start);

        Ok(AstExpr::String(
            StringExpr(
                Interpolation::new_plain(parser.toks_mut().raw_text(start)),
                QuoteKind::None,
            ),
            span,
        )
        .span(span))
    }

    pub(crate) fn try_parse_special_function(
        parser: &mut P,
        name: &str,
        start: usize,
    ) -> SassResult<Option<Spanned<AstExpr>>> {
        let normalized = unvendor(name);

        if matches!(parser.toks().peek(), Some(Token { kind: '(', .. })) {
            if let Some(calculation) = ValueParser::try_parse_calculation(parser, name, start)? {
                return Ok(Some(calculation));
            }

            // When unprefixed calc() parsing fails and returns None (dynamic
            // content detected), skip the CSS passthrough below and fall
            // through to the regular function call path, which properly
            // evaluates Sass variables and expressions. Prefixed calc
            // (e.g. -a-calc) still uses CSS passthrough.
            if name == "calc" {
                return Ok(None);
            }
        }

        let mut buffer;

        // `type()` is a special function only when unprefixed
        if name == "type" {
            if !parser.scan_char('(') {
                return Ok(None);
            }
            buffer = Interpolation::new_plain(name.to_owned());
            buffer.add_char('(');

            buffer.add_interpolation(parser.parse_interpolated_declaration_value(false, true, true)?);
            parser.expect_char(')')?;
            buffer.add_char(')');

            return Ok(Some(
                AstExpr::String(
                    StringExpr(buffer, QuoteKind::None),
                    parser.toks_mut().span_from(start),
                )
                .span(parser.toks_mut().span_from(start)),
            ));
        }

        match normalized {
            "calc" | "element" | "expression" => {
                if !parser.scan_char('(') {
                    return Ok(None);
                }

                buffer = Interpolation::new_plain(name.to_owned());
                buffer.add_char('(');
            }
            "progid" => {
                if !parser.scan_char(':') {
                    return Ok(None);
                }
                buffer = Interpolation::new_plain(name.to_owned());
                buffer.add_char(':');

                while let Some(Token { kind, .. }) = parser.toks().peek() {
                    if !kind.is_alphabetic() && kind != '.' {
                        break;
                    }
                    buffer.add_char(kind);
                    parser.toks_mut().next();
                }
                parser.expect_char('(')?;
                buffer.add_char('(');
            }
            "url" => {
                return Ok(parser.try_url_contents(None)?.map(|contents| {
                    AstExpr::String(
                        StringExpr(contents, QuoteKind::None),
                        parser.toks_mut().span_from(start),
                    )
                    .span(parser.toks_mut().span_from(start))
                }))
            }
            _ => return Ok(None),
        }

        buffer.add_interpolation(parser.parse_interpolated_declaration_value(false, true, true)?);
        parser.expect_char(')')?;
        buffer.add_char(')');

        Ok(Some(
            AstExpr::String(
                StringExpr(buffer, QuoteKind::None),
                parser.toks_mut().span_from(start),
            )
            .span(parser.toks_mut().span_from(start)),
        ))
    }

    /// Quick scan of calc() content for dynamic elements ($variables,
    /// var(), env()) that can't be fully validated at parse time. When
    /// present, space-separated values should be allowed and the
    /// calculation parser falls back to a regular function call.
    /// Cursor must be at `(`. Resets cursor after scanning.
    fn scan_for_dynamic_calc_content(parser: &mut P) -> bool {
        let start = parser.toks().cursor();
        let mut parens = 0i32;

        while let Some(tok) = parser.toks_mut().next() {
            match tok.kind {
                '(' => parens += 1,
                ')' => {
                    parens -= 1;
                    if parens < 0 {
                        break;
                    }
                }
                '$' if parser.toks().peek().is_some_and(|t| {
                    t.kind.is_ascii_alphabetic() || t.kind == '_'
                }) => {
                    parser.toks_mut().set_cursor(start);
                    return true;
                }
                '#' if parser.toks().peek().is_some_and(|t| t.kind == '{') => {
                    parser.toks_mut().set_cursor(start);
                    return true;
                }
                'v' | 'V' | 'e' | 'E' => {
                    // Check for var( or env(
                    let remaining_start = parser.toks().cursor();
                    let mut name = String::from(tok.kind);
                    while let Some(t) = parser.toks().peek() {
                        if t.kind.is_ascii_alphanumeric() || t.kind == '-' || t.kind == '_' {
                            name.push(t.kind);
                            parser.toks_mut().next();
                        } else {
                            break;
                        }
                    }
                    let lower = name.to_ascii_lowercase();
                    if (lower == "var" || lower == "env")
                        && matches!(parser.toks().peek(), Some(Token { kind: '(', .. }))
                    {
                        parser.toks_mut().set_cursor(start);
                        return true;
                    }
                    parser.toks_mut().set_cursor(remaining_start);
                }
                _ => {}
            }
        }

        parser.toks_mut().set_cursor(start);
        false
    }

    fn contains_calculation_interpolation(parser: &mut P) -> SassResult<bool> {
        let mut parens = 0;
        let mut brackets = Vec::new();
        let mut has_interpolation = false;
        let mut has_top_level_comma = false;

        let start = parser.toks().cursor();

        // If the first non-whitespace character can't start a calculation
        // value, don't use the greedy interpolation path — let the normal
        // parser run so it produces the proper error message.
        if let Some(first) = parser.toks().peek() {
            if !matches!(first.kind,
                '+' | '-' | '.' | '0'..='9' | '$' | '(' | '#' | '\'' | '"'
                | ' ' | '\t' | '\n' | '\r'
            ) && !first.kind.is_ascii_alphabetic()
                && first.kind != '_'
            {
                return Ok(false);
            }
        }

        while let Some(next) = parser.toks().peek() {
            match next.kind {
                '\\' => {
                    parser.toks_mut().next();
                    // todo: i wonder if this can be broken (not for us but dart-sass)
                    parser.toks_mut().next();
                }
                '/' => {
                    if !parser.scan_comment()? {
                        parser.toks_mut().next();
                    }
                }
                '\'' | '"' => {
                    parser.parse_interpolated_string()?;
                }
                '#' => {
                    if parens == 0
                        && matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. }))
                    {
                        has_interpolation = true;
                    }
                    parser.toks_mut().next();
                }
                ',' => {
                    if parens == 0 && brackets.is_empty() {
                        has_top_level_comma = true;
                    }
                    parser.toks_mut().next();
                }
                '(' | '{' | '[' => {
                    if next.kind == '(' {
                        parens += 1;
                    }
                    brackets.push(opposite_bracket(next.kind));
                    parser.toks_mut().next();
                }
                ')' | '}' | ']' => {
                    if next.kind == ')' {
                        parens -= 1;
                    }
                    if brackets.is_empty() || brackets.pop() != Some(next.kind) {
                        parser.toks_mut().set_cursor(start);
                        // When there are multiple comma-separated arguments,
                        // don't use the greedy interpolation path — parse each
                        // argument individually so they can be resolved separately
                        return Ok(has_interpolation && !has_top_level_comma);
                    }
                    parser.toks_mut().next();
                }
                _ => {
                    parser.toks_mut().next();
                }
            }
        }

        parser.toks_mut().set_cursor(start);
        Ok(false)
    }

    fn try_parse_calculation_interpolation(
        parser: &mut P,
        start: usize,
    ) -> SassResult<Option<AstExpr>> {
        Ok(
            if ValueParser::contains_calculation_interpolation(parser)? {
                Some(AstExpr::String(
                    StringExpr(
                        parser.parse_interpolated_declaration_value(false, false, true)?,
                        QuoteKind::None,
                    ),
                    parser.toks_mut().span_from(start),
                ))
            } else {
                None
            },
        )
    }

    fn parse_calculation_value(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        match parser.toks().peek() {
            Some(Token {
                kind: sign @ ('+' | '-'),
                ..
            }) if !matches!(
                parser.toks().peek_n(1),
                Some(Token {
                    kind: '.' | '0'..='9',
                    ..
                })
            ) && parser.toks().peek_n(1).is_some_and(|t| {
                t.kind.is_ascii_alphabetic() || t.kind == '_'
            }) => {
                let start = parser.toks().cursor();
                parser.toks_mut().next();
                let value = ValueParser::parse_calculation_value(parser)?;
                if sign == '-' {
                    let span = parser.toks_mut().span_from(start);
                    Ok(AstExpr::UnaryOp(UnaryOp::Neg, Rc::new(value.node), span)
                        .span(span))
                } else {
                    Ok(value)
                }
            }
            Some(Token {
                kind: '+' | '-' | '.' | '0'..='9',
                ..
            }) => ValueParser::parse_number(parser),
            Some(Token { kind: '$', .. }) => ValueParser::parse_variable(parser),
            Some(Token { kind: '(', .. }) => {
                let start = parser.toks().cursor();
                parser.toks_mut().next();

                let value = match ValueParser::try_parse_calculation_interpolation(parser, start)? {
                    Some(v) => v,
                    None => {
                        parser.whitespace()?;
                        ValueParser::parse_calculation_sum(parser)?.node
                    }
                };

                parser.whitespace()?;
                parser.expect_char(')')?;

                Ok(AstExpr::Paren(Rc::new(value)).span(parser.toks_mut().span_from(start)))
            }
            Some(Token { kind: '#', .. })
                if matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. })) =>
            {
                let start = parser.toks().cursor();
                let interpolation = parser.parse_single_interpolation()?;
                let span = parser.toks_mut().span_from(start);
                Ok(AstExpr::String(
                    StringExpr(interpolation, QuoteKind::None),
                    span,
                )
                .span(span))
            }
            _ if !parser.looking_at_identifier() => Err((
                "Expected number, variable, function, or calculation.",
                parser.toks().current_span(),
            )
                .into()),
            _ => {
                let start = parser.toks().cursor();
                let ident = parser.parse_identifier(false, false)?;
                let ident_span = parser.toks_mut().span_from(start);
                if parser.scan_char('.') {
                    return ValueParser::namespaced_expression(
                        Spanned {
                            node: Identifier::from(&ident),
                            span: ident_span,
                        },
                        start,
                        parser,
                    );
                }

                if !parser.toks().next_char_is('(') {
                    let lowercase = ident.to_ascii_lowercase();
                    match lowercase.as_str() {
                        "pi" => {
                            return Ok(AstExpr::Number {
                                n: Number(std::f64::consts::PI),
                                unit: Unit::None,
                            }
                            .span(parser.toks_mut().span_from(start)));
                        }
                        "e" => {
                            return Ok(AstExpr::Number {
                                n: Number(std::f64::consts::E),
                                unit: Unit::None,
                            }
                            .span(parser.toks_mut().span_from(start)));
                        }
                        "infinity" => {
                            return Ok(AstExpr::Number {
                                n: Number(f64::INFINITY),
                                unit: Unit::None,
                            }
                            .span(parser.toks_mut().span_from(start)));
                        }
                        "nan" => {
                            return Ok(AstExpr::Number {
                                n: Number(f64::NAN),
                                unit: Unit::None,
                            }
                            .span(parser.toks_mut().span_from(start)));
                        }
                        _ => {
                            let span = parser.toks_mut().span_from(start);
                            return Ok(AstExpr::String(
                                StringExpr(
                                    Interpolation::new_plain(ident),
                                    QuoteKind::None,
                                ),
                                span,
                            )
                            .span(span));
                        }
                    }
                }

                let lowercase = ident.to_ascii_lowercase();
                let calculation =
                    ValueParser::try_parse_calculation_inner(parser, &lowercase, start, true)?;

                if let Some(calc) = calculation {
                    Ok(calc)
                } else if lowercase == "if" {
                    Ok(AstExpr::If(Rc::new(Ternary(
                        parser.parse_argument_invocation(false, false)?,
                    )))
                    .span(parser.toks_mut().span_from(start)))
                } else {
                    let is_css_custom = ident.starts_with("--");
                    Ok(AstExpr::FunctionCall(FunctionCallExpr {
                        namespace: None,
                        name: Identifier::from(&ident),
                        original_name: CompactString::from(ident),
                        arguments: Rc::new(parser.parse_argument_invocation(false, false)?),
                        span: parser.toks_mut().span_from(start),
                        is_css_custom_function: is_css_custom,
                    })
                    .span(parser.toks_mut().span_from(start)))
                }
            }
        }
    }
    fn parse_calculation_product(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let mut product = ValueParser::parse_calculation_value(parser)?;

        loop {
            parser.whitespace()?;
            match parser.toks().peek() {
                Some(Token {
                    kind: op @ ('*' | '/'),
                    ..
                }) => {
                    parser.toks_mut().next();
                    parser.whitespace()?;

                    let rhs = ValueParser::parse_calculation_value(parser)?;

                    let span = product.span.merge(rhs.span);

                    product.node = AstExpr::BinaryOp(Rc::new(BinaryOpExpr {
                        lhs: product.node,
                        op: if op == '*' {
                            BinaryOp::Mul
                        } else {
                            BinaryOp::Div
                        },
                        rhs: rhs.node,
                        allows_slash: false,
                        span,
                    }));

                    product.span = span;
                }
                _ => return Ok(product),
            }
        }
    }
    fn parse_calculation_sum(parser: &mut P) -> SassResult<Spanned<AstExpr>> {
        let mut sum = ValueParser::parse_calculation_product(parser)?;

        loop {
            match parser.toks().peek() {
                Some(Token {
                    kind: next @ ('+' | '-'),
                    ..
                }) => {
                    if !matches!(
                        parser.toks().peek_n_backwards(1),
                        Some(Token {
                            kind: ' ' | '\t' | '\r' | '\n',
                            ..
                        })
                    ) || !matches!(
                        parser.toks().peek_n(1),
                        Some(Token {
                            kind: ' ' | '\t' | '\r' | '\n',
                            ..
                        })
                    ) {
                        return Err((
                            "\"+\" and \"-\" must be surrounded by whitespace in calculations.",
                            parser.toks().current_span(),
                        )
                            .into());
                    }

                    parser.toks_mut().next();
                    parser.whitespace()?;

                    let rhs = ValueParser::parse_calculation_product(parser)?;

                    let span = sum.span.merge(rhs.span);

                    sum = AstExpr::BinaryOp(Rc::new(BinaryOpExpr {
                        lhs: sum.node,
                        op: if next == '+' {
                            BinaryOp::Plus
                        } else {
                            BinaryOp::Minus
                        },
                        rhs: rhs.node,
                        allows_slash: false,
                        span,
                    }))
                    .span(span);
                }
                _ => return Ok(sum),
            }
        }
    }

    fn parse_calculation_arguments(
        parser: &mut P,
        max_args: Option<usize>,
        start: usize,
    ) -> SassResult<Vec<AstExpr>> {
        Self::parse_calculation_arguments_inner(parser, max_args, start, false)
    }

    fn parse_calculation_arguments_inner(
        parser: &mut P,
        max_args: Option<usize>,
        start: usize,
        skip_interpolation_check: bool,
    ) -> SassResult<Vec<AstExpr>> {
        parser.expect_char('(')?;
        let was_consuming_newlines = parser.is_consuming_newlines();
        parser.set_consume_newlines(true);
        if !skip_interpolation_check {
            if let Some(interpolation) =
                ValueParser::try_parse_calculation_interpolation(parser, start)?
            {
                parser.expect_char(')')?;
                parser.set_consume_newlines(was_consuming_newlines);
                return Ok(vec![interpolation]);
            }
        }

        parser.whitespace()?;
        let mut arguments = vec![ValueParser::parse_calculation_sum(parser)?.node];

        while (max_args.is_none() || arguments.len() < max_args.unwrap()) && parser.scan_char(',') {
            parser.whitespace()?;
            arguments.push(ValueParser::parse_calculation_sum(parser)?.node);
        }

        parser.expect_char_with_message(
            ')',
            if Some(arguments.len()) == max_args {
                r#""+", "-", "*", "/", or ")""#
            } else {
                r#""+", "-", "*", "/", ",", or ")""#
            },
        )?;
        parser.set_consume_newlines(was_consuming_newlines);

        Ok(arguments)
    }

    fn try_parse_calculation(
        parser: &mut P,
        name: &str,
        start: usize,
    ) -> SassResult<Option<Spanned<AstExpr>>> {
        Self::try_parse_calculation_inner(parser, name, start, false)
    }

    fn try_parse_calculation_inner(
        parser: &mut P,
        name: &str,
        start: usize,
        _in_calculation: bool,
    ) -> SassResult<Option<Spanned<AstExpr>>> {
        debug_assert!(parser.toks().next_char_is('('));

        Ok(Some(match name {
            "calc" => {
                // calc() is parsed as a calculation if possible. When parsing
                // fails and the content contains dynamic elements (var(), env(),
                // $variables, or #{interpolation}), fall back to a normal
                // function call so those elements get evaluated. For purely
                // static content, propagate the calculation error.
                //
                // Skip the greedy interpolation check so expressions like
                // `100%/2 - #{$x}` are parsed structurally (allowing `100%/2`
                // to simplify to `50%`). The try/catch handles cases where
                // interpolation makes structured parsing impossible.
                let before_args = parser.toks().cursor();
                match ValueParser::parse_calculation_arguments_inner(parser, Some(1), start, true) {
                    Ok(args) => AstExpr::Calculation {
                        name: CalculationName::Calc,
                        args,
                    }
                    .span(parser.toks_mut().span_from(start)),
                    Err(e) => {
                        parser.toks_mut().set_cursor(before_args);
                        if Self::scan_for_dynamic_calc_content(parser) {
                            parser.toks_mut().set_cursor(before_args);
                            return Ok(None);
                        }
                        return Err(e);
                    }
                }
            }
            "min" | "max" => {
                // min() and max() are parsed as calculations if possible, and otherwise
                // are parsed as normal Sass functions.
                let before_args = parser.toks().cursor();

                let args = match ValueParser::parse_calculation_arguments_inner(parser, None, start, true) {
                    Ok(args) => args,
                    Err(..) => {
                        parser.toks_mut().set_cursor(before_args);
                        return Ok(None);
                    }
                };

                AstExpr::Calculation {
                    name: if name == "min" {
                        CalculationName::Min
                    } else {
                        CalculationName::Max
                    },
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "clamp" => {
                let args = ValueParser::parse_calculation_arguments(parser, Some(3), start)?;
                AstExpr::Calculation {
                    name: CalculationName::Clamp,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "abs" => {
                let before_args = parser.toks().cursor();
                match ValueParser::parse_calculation_arguments_inner(parser, Some(1), start, true) {
                    Ok(args) => AstExpr::Calculation {
                        name: CalculationName::Abs,
                        args,
                    }
                    .span(parser.toks_mut().span_from(start)),
                    Err(..) => {
                        parser.toks_mut().set_cursor(before_args);
                        return Ok(None);
                    }
                }
            }
            "acos" | "asin" | "atan" | "cos" | "exp" | "sign" | "sin" | "sqrt" | "tan" => {
                let calc_name = match name {
                    "acos" => CalculationName::Acos,
                    "asin" => CalculationName::Asin,
                    "atan" => CalculationName::Atan,
                    "cos" => CalculationName::Cos,
                    "exp" => CalculationName::Exp,
                    "sign" => CalculationName::Sign,
                    "sin" => CalculationName::Sin,
                    "sqrt" => CalculationName::Sqrt,
                    "tan" => CalculationName::Tan,
                    _ => unreachable!(),
                };
                let args = ValueParser::parse_calculation_arguments(parser, Some(1), start)?;
                AstExpr::Calculation {
                    name: calc_name,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "atan2" | "mod" | "rem" => {
                let calc_name = match name {
                    "atan2" => CalculationName::Atan2,
                    "mod" => CalculationName::Mod,
                    "rem" => CalculationName::Rem,
                    _ => unreachable!(),
                };
                let args = ValueParser::parse_calculation_arguments(parser, Some(2), start)?;
                AstExpr::Calculation {
                    name: calc_name,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "pow" => {
                let args = ValueParser::parse_calculation_arguments(parser, Some(2), start)?;
                AstExpr::Calculation {
                    name: CalculationName::Pow,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "log" => {
                let args = ValueParser::parse_calculation_arguments(parser, Some(2), start)?;
                AstExpr::Calculation {
                    name: CalculationName::Log,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "hypot" => {
                let args = ValueParser::parse_calculation_arguments(parser, None, start)?;
                AstExpr::Calculation {
                    name: CalculationName::Hypot,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "calc-size" => {
                let args = ValueParser::parse_calculation_arguments(parser, Some(2), start)?;
                AstExpr::Calculation {
                    name: CalculationName::CalcSize,
                    args,
                }
                .span(parser.toks_mut().span_from(start))
            }
            "round" => {
                let before_args = parser.toks().cursor();
                match ValueParser::parse_calculation_arguments_inner(parser, Some(3), start, true) {
                    Ok(args) => AstExpr::Calculation {
                        name: CalculationName::Round,
                        args,
                    }
                    .span(parser.toks_mut().span_from(start)),
                    Err(..) => {
                        parser.toks_mut().set_cursor(before_args);
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        }))
    }

    fn reset_state(&mut self, parser: &mut P) -> SassResult<()> {
        self.comma_expressions = None;
        self.space_expressions = None;
        self.binary_operators = None;
        self.operands = None;
        parser.toks_mut().set_cursor(self.start);
        self.allow_slash = true;
        self.single_expression = Some(self.parse_single_expression(parser)?);

        Ok(())
    }
}
