use std::sync::Arc;

use codemap::Spanned;

use crate::{
    ast::*,
    error::SassResult,
    Token,
};

use super::StylesheetParser;

pub(crate) fn try_parse_css_if<'a>(
    parser: &mut impl StylesheetParser<'a>,
    start: usize,
) -> SassResult<Option<Spanned<AstExpr>>> {
    let before_paren = parser.toks().cursor();

    if !parser.toks().next_char_is('(') {
        return Ok(None);
    }

    let outer_consuming_newlines = parser.is_consuming_newlines();
    parser.scan_char('(');
    parser.set_consume_newlines(true);
    parser.whitespace()?;

    let is_new_syntax = detect_css_if_syntax(parser)?;

    parser.toks_mut().set_cursor(before_paren);
    parser.set_consume_newlines(outer_consuming_newlines);

    if !is_new_syntax {
        return Ok(None);
    }

    parser.expect_char('(')?;
    let was_consuming_newlines = parser.is_consuming_newlines();
    parser.set_consume_newlines(true);
    parser.whitespace()?;

    let mut clauses = Vec::new();

    loop {
        parser.whitespace()?;

        if parser.toks().next_char_is(')') {
            break;
        }

        let condition = parse_if_condition(parser)?;
        parser.whitespace()?;
        parser.expect_char(':')?;
        parser.whitespace()?;

        let value = parse_clause_value(parser)?;

        clauses.push(IfClause { condition, value });

        parser.whitespace()?;

        if parser.scan_char(';') {
            parser.whitespace()?;
            if parser.toks().next_char_is(')') {
                break;
            }
            continue;
        }

        break;
    }

    parser.expect_char(')')?;
    parser.set_consume_newlines(was_consuming_newlines);
    let span = parser.toks_mut().span_from(start);

    Ok(Some(
        AstExpr::CssIf(Arc::new(CssIfExpression { clauses, span })).span(span),
    ))
}

/// Detect whether we're looking at new CSS if() syntax vs legacy if().
fn detect_css_if_syntax<'a>(parser: &mut impl StylesheetParser<'a>) -> SassResult<bool> {
    let start = parser.toks().cursor();

    // `else` keyword (not followed by identifier body char) → new syntax
    if parser.scan_identifier("else", false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(start);
        return Ok(true);
    }
    parser.toks_mut().set_cursor(start);

    // `not` followed by whitespace → new syntax (not operator)
    // `not(` → ambiguous: scan balanced parens and check for `:` (new) vs `,`/`)` (legacy)
    if parser.scan_identifier("not", false)? && !parser.looking_at_identifier_body() {
        if !parser.toks().next_char_is('(') {
            // `not` + whitespace → definitely new syntax
            parser.toks_mut().set_cursor(start);
            return Ok(true);
        }
        // `not(` — scan balanced parens, check for `:`
        parser.scan_char('(');
        let mut depth = 1;
        while depth > 0 {
            match parser.toks().peek() {
                Some(Token { kind: '(', .. }) => {
                    depth += 1;
                    parser.toks_mut().next();
                }
                Some(Token { kind: ')', .. }) => {
                    depth -= 1;
                    parser.toks_mut().next();
                }
                Some(_) => {
                    parser.toks_mut().next();
                }
                None => break,
            }
        }
        parser.whitespace()?;
        let found_colon = parser.toks().next_char_is(':');
        parser.toks_mut().set_cursor(start);
        if found_colon {
            return Ok(true);
        }
    }
    parser.toks_mut().set_cursor(start);

    // `(` → grouping → new syntax
    if parser.toks().next_char_is('(') {
        return Ok(true);
    }

    // `#{` → interpolation → new syntax
    if matches!(parser.toks().peek(), Some(Token { kind: '#', .. }))
        && matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. }))
    {
        return Ok(true);
    }

    // identifier followed by `(`
    if parser.looking_at_identifier() {
        let ident_start = parser.toks().cursor();
        let _ = parser.parse_identifier(false, false);

        if parser.toks().next_char_is('(') {
            let name = parser.toks().raw_text(ident_start);
            let lower = name.to_ascii_lowercase();

            // Known new-syntax functions
            if matches!(lower.as_str(), "sass" | "css" | "var" | "attr" | "if") {
                parser.toks_mut().set_cursor(start);
                return Ok(true);
            }

            // Unknown function: scan balanced parens then check for `:`
            parser.scan_char('(');
            let mut depth = 1;
            while depth > 0 {
                match parser.toks().peek() {
                    Some(Token { kind: '(', .. }) => {
                        depth += 1;
                        parser.toks_mut().next();
                    }
                    Some(Token { kind: ')', .. }) => {
                        depth -= 1;
                        parser.toks_mut().next();
                    }
                    Some(_) => {
                        parser.toks_mut().next();
                    }
                    None => break,
                }
            }
            // Check for `:` or whitespace+`:`
            parser.whitespace()?;
            let found_colon = parser.toks().next_char_is(':');
            parser.toks_mut().set_cursor(start);
            if found_colon {
                return Ok(true);
            }
        }

        parser.toks_mut().set_cursor(start);
    }

    Ok(false)
}

/// Parse a top-level if() condition (with and/or combinators).
fn parse_if_condition<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<IfCondition> {
    // Check for `else` keyword
    let start = parser.toks().cursor();
    if parser.scan_identifier("else", false)? && !parser.looking_at_identifier_body() {
        return Ok(IfCondition::Else);
    }
    parser.toks_mut().set_cursor(start);

    // Check for `not`
    if parser.scan_identifier("not", false)? {
        if parser.toks().next_char_is('(') {
            return Err((
                "Whitespace is required between \"not\" and \"(\"".to_string(),
                parser.toks().current_span(),
            )
                .into());
        }
        if !parser.looking_at_identifier_body() {
            parser.whitespace()?;
            let inner = parse_condition_primary(parser)?;
            let span = parser.toks_mut().span_from(start);

            // `not` cannot be followed by `and`, `or`, or raw items
            parser.whitespace()?;
            check_not_followed_by_combinator(parser)?;
            check_not_followed_by_raw(parser)?;

            return Ok(IfCondition::Not(Box::new(inner), span));
        }
        parser.toks_mut().set_cursor(start);
    }
    parser.toks_mut().set_cursor(start);

    // Parse first operand (may include adjacent raw items)
    let first = parse_condition_operand(parser)?;
    parser.whitespace()?;

    // Check for sass-before-raw conflict: if first operand contains sass
    // (even in parens) and next thing looks like a raw function, error
    check_sass_before_raw(parser, &first)?;

    // Check for `and` / `or` combinator
    let comb_start = parser.toks().cursor();

    if parser.scan_identifier("and", false)? {
        if parser.toks().next_char_is('(') {
            parser.toks_mut().set_cursor(comb_start);
            return Err((
                "Whitespace is required between \"and\" and \"(\"".to_string(),
                parser.toks().current_span(),
            )
                .into());
        }
        if !parser.looking_at_identifier_body() {
            parser.whitespace()?;
            return parse_and_chain(parser, first);
        }
        parser.toks_mut().set_cursor(comb_start);
    }
    parser.toks_mut().set_cursor(comb_start);

    if parser.scan_identifier("or", false)? {
        if parser.toks().next_char_is('(') {
            parser.toks_mut().set_cursor(comb_start);
            return Err((
                "Whitespace is required between \"or\" and \"(\"".to_string(),
                parser.toks().current_span(),
            )
                .into());
        }
        if !parser.looking_at_identifier_body() {
            parser.whitespace()?;
            return parse_or_chain(parser, first);
        }
        parser.toks_mut().set_cursor(comb_start);
    }
    parser.toks_mut().set_cursor(comb_start);

    Ok(first)
}

/// Check if a condition contains any sass() atoms, crossing paren boundaries.
fn condition_contains_sass(cond: &IfCondition) -> bool {
    match cond {
        IfCondition::Atom(IfConditionAtom::Sass(_, _)) => true,
        IfCondition::Atom(_) => false,
        IfCondition::Else => false,
        IfCondition::Not(inner, _) | IfCondition::Paren(inner) => condition_contains_sass(inner),
        IfCondition::And(ops) | IfCondition::Or(ops) => ops.iter().any(condition_contains_sass),
    }
}

/// If the current operand contains sass() and the next token looks like a raw
/// function call (var, attr, css, if, etc.), error.
fn check_sass_before_raw<'a>(
    parser: &mut impl StylesheetParser<'a>,
    operand: &IfCondition,
) -> SassResult<()> {
    if !condition_contains_sass(operand) {
        return Ok(());
    }

    let pos = parser.toks().cursor();

    // Check for identifier followed by `(`
    if parser.looking_at_identifier() {
        let id_start = parser.toks().cursor();
        if let Ok(name) = parser.parse_identifier(false, false) {
            let lower = name.to_ascii_lowercase();
            if parser.toks().next_char_is('(')
                && matches!(lower.as_str(), "var" | "attr" | "css" | "if")
            {
                parser.toks_mut().set_cursor(pos);
                return Err((
                    "if() conditions with arbitrary substitutions may not contain sass() expressions.",
                    parser.toks().current_span(),
                )
                    .into());
            }
        }
        parser.toks_mut().set_cursor(id_start);
    }
    parser.toks_mut().set_cursor(pos);
    Ok(())
}

fn check_not_followed_by_combinator<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<()> {
    let pos = parser.toks().cursor();
    if parser.scan_identifier("and", false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(pos);
        return Err(("expected \":\".", parser.toks().current_span()).into());
    }
    parser.toks_mut().set_cursor(pos);
    if parser.scan_identifier("or", false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(pos);
        return Err(("expected \":\".", parser.toks().current_span()).into());
    }
    parser.toks_mut().set_cursor(pos);
    Ok(())
}

/// After `not`, check that no raw items follow (var, identifier+paren, etc.)
fn check_not_followed_by_raw<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<()> {
    let pos = parser.toks().cursor();

    // Check for identifier followed by `(`
    if parser.looking_at_identifier() {
        let id_start = parser.toks().cursor();
        let _ = parser.parse_identifier(false, false);
        if parser.toks().next_char_is('(') {
            parser.toks_mut().set_cursor(pos);
            return Err(("expected \":\".", parser.toks().current_span()).into());
        }
        parser.toks_mut().set_cursor(id_start);
    }
    parser.toks_mut().set_cursor(pos);
    Ok(())
}

fn parse_and_chain<'a>(
    parser: &mut impl StylesheetParser<'a>,
    first: IfCondition,
) -> SassResult<IfCondition> {
    let mut operands = vec![first];
    operands.push(parse_and_or_operand(parser, "and")?);
    loop {
        parser.whitespace()?;
        let pos = parser.toks().cursor();
        if parser.scan_identifier("and", false)? {
            if parser.toks().next_char_is('(') {
                parser.toks_mut().set_cursor(pos);
                return Err((
                    "Whitespace is required between \"and\" and \"(\"".to_string(),
                    parser.toks().current_span(),
                )
                    .into());
            }
            if !parser.looking_at_identifier_body() {
                parser.whitespace()?;
                operands.push(parse_and_or_operand(parser, "and")?);
            } else {
                parser.toks_mut().set_cursor(pos);
                break;
            }
        } else {
            parser.toks_mut().set_cursor(pos);
            break;
        }
    }
    Ok(IfCondition::And(operands))
}

fn parse_or_chain<'a>(
    parser: &mut impl StylesheetParser<'a>,
    first: IfCondition,
) -> SassResult<IfCondition> {
    let mut operands = vec![first];
    operands.push(parse_and_or_operand(parser, "or")?);
    loop {
        parser.whitespace()?;
        let pos = parser.toks().cursor();
        if parser.scan_identifier("or", false)? {
            if parser.toks().next_char_is('(') {
                parser.toks_mut().set_cursor(pos);
                return Err((
                    "Whitespace is required between \"or\" and \"(\"".to_string(),
                    parser.toks().current_span(),
                )
                    .into());
            }
            if !parser.looking_at_identifier_body() {
                parser.whitespace()?;
                operands.push(parse_and_or_operand(parser, "or")?);
            } else {
                parser.toks_mut().set_cursor(pos);
                break;
            }
        } else {
            parser.toks_mut().set_cursor(pos);
            break;
        }
    }
    Ok(IfCondition::Or(operands))
}

/// Parse an operand for `and`/`or`.
fn parse_and_or_operand<'a>(
    parser: &mut impl StylesheetParser<'a>,
    context: &str,
) -> SassResult<IfCondition> {
    let pos = parser.toks().cursor();

    // Disallow bare `not`
    if parser.scan_identifier("not", false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(pos);
        return Err(("expected \"(\".", parser.toks().current_span()).into());
    }
    parser.toks_mut().set_cursor(pos);

    // Disallow bare `else`
    if parser.scan_identifier("else", false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(pos);
        return Err(("expected \"(\".", parser.toks().current_span()).into());
    }
    parser.toks_mut().set_cursor(pos);

    // Disallow mixing combinators
    let other = if context == "and" { "or" } else { "and" };
    if parser.scan_identifier(other, false)? && !parser.looking_at_identifier_body() {
        parser.toks_mut().set_cursor(pos);
        return Err(("expected \":\".", parser.toks().current_span()).into());
    }
    parser.toks_mut().set_cursor(pos);

    parse_condition_operand(parser)
}

/// Parse a condition operand — a primary condition optionally followed by
/// adjacent raw items (var(), attr(), if(), #{}, other css functions).
fn parse_condition_operand<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<IfCondition> {
    let primary = parse_condition_primary(parser)?;

    // After a CSS, CssRaw, or Interp atom, check for adjacent raw items
    match &primary {
        IfCondition::Atom(IfConditionAtom::Css(_, _))
        | IfCondition::Atom(IfConditionAtom::CssRaw(_, _))
        | IfCondition::Atom(IfConditionAtom::Interp(_, _)) => {
            try_extend_with_raw(parser, primary)
        }
        _ => Ok(primary),
    }
}

/// After parsing a CSS atom, consume additional adjacent raw items.
fn try_extend_with_raw<'a>(
    parser: &mut impl StylesheetParser<'a>,
    first: IfCondition,
) -> SassResult<IfCondition> {
    let (first_interp, first_span) = match first {
        IfCondition::Atom(IfConditionAtom::Css(interp, span))
        | IfCondition::Atom(IfConditionAtom::CssRaw(interp, span)) => (interp, span),
        IfCondition::Atom(IfConditionAtom::Interp(expr, span)) => {
            // Convert interpolation to a CSS atom with an interpolation expression
            let mut interp = Interpolation::new();
            interp.add_expr(Spanned { node: expr, span });
            (interp, span)
        }
        _ => unreachable!(),
    };

    let mut buffer = first_interp;
    let mut has_extra = false;

    loop {
        let pos = parser.toks().cursor();
        let had_whitespace = matches!(
            parser.toks().peek(),
            Some(Token {
                kind: ' ' | '\t' | '\n' | '\r',
                ..
            })
        );
        if had_whitespace {
            parser.whitespace()?;
        }

        // End of condition markers
        if matches!(
            parser.toks().peek(),
            Some(Token { kind: ':' | ';' | ')', .. }) | None
        ) {
            parser.toks_mut().set_cursor(pos);
            break;
        }

        // `(` — check if it contains sass(), which would be an error
        if parser.toks().next_char_is('(') {
            let peek_pos = parser.toks().cursor();
            parser.toks_mut().next(); // (
            parser.whitespace()?;
            let mut found_sass = false;
            if parser.looking_at_identifier() {
                let id_pos = parser.toks().cursor();
                if let Ok(name) = parser.parse_identifier(false, false) {
                    if name.eq_ignore_ascii_case("sass") && parser.toks().next_char_is('(') {
                        found_sass = true;
                    }
                }
                if !found_sass {
                    parser.toks_mut().set_cursor(id_pos);
                }
            }
            parser.toks_mut().set_cursor(peek_pos);
            if found_sass {
                return Err((
                    "if() conditions with arbitrary substitutions may not contain sass() expressions.",
                    parser.toks().current_span(),
                )
                    .into());
            }
            parser.toks_mut().set_cursor(pos);
            break;
        }

        // `#{` → interpolation as raw item
        if matches!(parser.toks().peek(), Some(Token { kind: '#', .. }))
            && matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. }))
        {
            parser.toks_mut().next(); // #
            parser.toks_mut().next(); // {
            let expr = parser.parse_expression(None, None, None)?;
            parser.expect_char('}')?;
            if parser.is_plain_css() {
                return Err(("Interpolation isn't allowed in plain CSS.", expr.span).into());
            }
            let span = parser.toks_mut().span_from(pos);
            if had_whitespace {
                buffer.add_char(' ');
            }
            buffer.add_expr(Spanned { node: expr.node, span });
            has_extra = true;
            continue;
        }

        // identifier followed by `(`
        if parser.looking_at_identifier() {
            let _ident_start = parser.toks().cursor();
            let name = parser.parse_identifier(false, false)?;
            let lower = name.to_ascii_lowercase();

            // Check for `and`/`or`/`not`/`else` keywords
            match lower.as_str() {
                "and" | "or" => {
                    if !parser.looking_at_identifier_body() {
                        // Keyword combinator — stop raw extension
                        parser.toks_mut().set_cursor(pos);
                        break;
                    }
                }
                "not" => {
                    if !parser.looking_at_identifier_body() {
                        if has_extra {
                            return Err((
                                "expected \"(\".",
                                parser.toks().current_span(),
                            )
                                .into());
                        }
                        parser.toks_mut().set_cursor(pos);
                        break;
                    }
                }
                "else" => {
                    if !parser.looking_at_identifier_body() {
                        // `else` after a raw condition — always error
                        return Err((
                            "expected \"(\".",
                            parser.toks().current_span(),
                        )
                            .into());
                    }
                }
                _ => {}
            }

            if parser.toks().next_char_is('(') {
                // sass() is not allowed in raw context
                if lower == "sass" {
                    return Err((
                        "if() conditions with arbitrary substitutions may not contain sass() expressions.",
                        parser.toks().current_span(),
                    )
                        .into());
                }

                // `and(`/`or(` without space → error
                if matches!(lower.as_str(), "and" | "or") && !parser.looking_at_identifier_body() {
                    return Err((
                        format!(
                            "Whitespace is required between \"{}\" and \"(\"",
                            name
                        ),
                        parser.toks().current_span(),
                    )
                        .into());
                }

                // Consume as raw CSS function
                parser.expect_char('(')?;
                let content = parse_css_function_args(parser)?;
                parser.expect_char(')')?;

                if had_whitespace {
                    buffer.add_char(' ');
                }
                buffer.add_string(format!("{}(", name));
                buffer.add_interpolation(content);
                buffer.add_char(')');
                has_extra = true;
                continue;
            }

            // Identifier not followed by `(` — not raw
            parser.toks_mut().set_cursor(pos);
            break;
        }

        parser.toks_mut().set_cursor(pos);
        break;
    }

    if !has_extra {
        return Ok(IfCondition::Atom(IfConditionAtom::Css(buffer, first_span)));
    }

    Ok(IfCondition::Atom(IfConditionAtom::CssRaw(buffer, first_span)))
}

/// Parse a primary condition atom.
fn parse_condition_primary<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<IfCondition> {
    // `(` → grouping
    if parser.toks().next_char_is('(') {
        parser.toks_mut().next();
        let was_consuming = parser.is_consuming_newlines();
        parser.set_consume_newlines(true);
        parser.whitespace()?;

        // `(else)` is not allowed
        if parser.toks().next_char_is(')') {
            return Err(("Expected identifier.", parser.toks().current_span()).into());
        }

        let inner = parse_if_condition(parser)?;

        // `else` inside parens is not allowed
        if matches!(inner, IfCondition::Else) {
            return Err(("expected \"(\".", parser.toks().current_span()).into());
        }

        parser.whitespace()?;
        parser.expect_char(')')?;
        parser.set_consume_newlines(was_consuming);
        return Ok(IfCondition::Paren(Box::new(inner)));
    }

    // `#{` → interpolation (may form function name if followed by `(`)
    if matches!(parser.toks().peek(), Some(Token { kind: '#', .. }))
        && matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. }))
    {
        let start = parser.toks().cursor();
        parser.toks_mut().next(); // #
        parser.toks_mut().next(); // {
        let expr = parser.parse_expression(None, None, None)?;
        parser.expect_char('}')?;

        if parser.is_plain_css() {
            return Err(("Interpolation isn't allowed in plain CSS.", expr.span).into());
        }

        // Check if followed by `(` — forms an interpolated function name
        if parser.toks().next_char_is('(') {
            parser.expect_char('(')?;
            let content = parse_css_function_args(parser)?;
            parser.expect_char(')')?;

            let mut interp = Interpolation::new();
            let expr_span = parser.toks_mut().span_from(start);
            interp.add_expr(Spanned { node: expr.node, span: expr_span });
            interp.add_char('(');
            interp.add_interpolation(content);
            interp.add_char(')');
            let span = parser.toks_mut().span_from(start);
            return Ok(IfCondition::Atom(IfConditionAtom::Css(interp, span)));
        }

        let span = parser.toks_mut().span_from(start);
        return Ok(IfCondition::Atom(IfConditionAtom::Interp(
            expr.node, span,
        )));
    }

    // Must be an identifier
    if !parser.looking_at_identifier() {
        return Err(("Expected identifier.", parser.toks().current_span()).into());
    }

    let ident_start = parser.toks().cursor();
    let name = parser.parse_identifier(false, false)?;
    let lower = name.to_ascii_lowercase();

    // Disallowed keywords
    match lower.as_str() {
        "not" | "and" | "or" => {
            if parser.toks().next_char_is('(') {
                return Err((
                    format!(
                        "Whitespace is required between \"{}\" and \"(\"",
                        name
                    ),
                    parser.toks().current_span(),
                )
                    .into());
            }
            // These keywords are not valid as condition primaries
            // Error pointing at what follows them (expecting a `(` for grouping)
            return Err(("expected \"(\".", parser.toks().current_span()).into());
        }
        "else" => {
            if !parser.toks().next_char_is('(') {
                return Err(("expected \"(\".", parser.toks().current_span()).into());
            }
        }
        _ => {}
    }

    // Must be followed by `(`
    if !parser.toks().next_char_is('(') {
        parser.toks_mut().set_cursor(ident_start);
        return Err(("Expected identifier.", parser.toks().current_span()).into());
    }

    parser.expect_char('(')?;
    let was_consuming = parser.is_consuming_newlines();
    parser.set_consume_newlines(true);

    match lower.as_str() {
        "sass" => {
            if parser.is_plain_css() {
                let span = parser.toks_mut().span_from(ident_start);
                return Err(("sass() conditions aren't allowed in plain CSS", span).into());
            }
            parser.whitespace()?;
            let expr = parser.parse_expression(None, None, None)?;
            parser.whitespace()?;
            parser.expect_char(')')?;
            parser.set_consume_newlines(was_consuming);
            let span = parser.toks_mut().span_from(ident_start);
            Ok(IfCondition::Atom(IfConditionAtom::Sass(expr.node, span)))
        }
        _ => {
            let content = parse_css_function_args(parser)?;
            parser.expect_char(')')?;
            parser.set_consume_newlines(was_consuming);

            let mut interp = Interpolation::new_plain(format!("{}(", name));
            interp.add_interpolation(content);
            interp.add_char(')');
            let span = parser.toks_mut().span_from(ident_start);
            Ok(IfCondition::Atom(IfConditionAtom::Css(interp, span)))
        }
    }
}

/// Parse the arguments of a CSS function as an Interpolation.
/// Preserves raw text exactly (including quote style), only processing #{...}.
/// Stops at the unmatched closing `)` (not consumed).
fn parse_css_function_args<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<Interpolation> {
    let mut buffer = Interpolation::new();
    let mut depth = 0; // track nested parens

    while let Some(tok) = parser.toks().peek() {
        match tok.kind {
            ')' if depth == 0 => break,
            ')' => {
                depth -= 1;
                parser.toks_mut().next();
                buffer.add_char(')');
            }
            '(' => {
                depth += 1;
                parser.toks_mut().next();
                buffer.add_char('(');
            }
            '#' if matches!(parser.toks().peek_n(1), Some(Token { kind: '{', .. })) => {
                parser.toks_mut().next(); // #
                parser.toks_mut().next(); // {
                let expr = parser.parse_expression(None, None, None)?;
                parser.expect_char('}')?;
                if parser.is_plain_css() {
                    return Err(("Interpolation isn't allowed in plain CSS.", expr.span).into());
                }
                buffer.add_expr(expr);
            }
            '\'' | '"' => {
                // Consume quoted string verbatim, preserving original quote character
                let quote = tok.kind;
                parser.toks_mut().next();
                buffer.add_char(quote);
                loop {
                    match parser.toks().peek() {
                        Some(Token { kind, .. }) if kind == quote => {
                            parser.toks_mut().next();
                            buffer.add_char(quote);
                            break;
                        }
                        Some(Token { kind: '\\', .. }) => {
                            parser.toks_mut().next();
                            buffer.add_char('\\');
                            if let Some(next) = parser.toks().peek() {
                                parser.toks_mut().next();
                                buffer.add_char(next.kind);
                            }
                        }
                        Some(tok) => {
                            let c = tok.kind;
                            parser.toks_mut().next();
                            buffer.add_char(c);
                        }
                        None => break,
                    }
                }
            }
            ' ' | '\t' | '\n' | '\r' if parser.is_consuming_newlines() => {
                // In indented syntax with consume_newlines, collapse whitespace
                parser.toks_mut().next();
                while matches!(parser.toks().peek(), Some(Token { kind: ' ' | '\t' | '\n' | '\r', .. })) {
                    parser.toks_mut().next();
                }
                // Only add space if not at start or end of args
                if !buffer.trailing_string().is_empty()
                    && !matches!(parser.toks().peek(), Some(Token { kind: ')', .. }) | None)
                {
                    buffer.add_char(' ');
                }
            }
            _ => {
                parser.toks_mut().next();
                buffer.add_char(tok.kind);
            }
        }
    }

    Ok(buffer)
}

/// Parse the value portion of a clause (after `:`).
fn parse_clause_value<'a>(
    parser: &mut impl StylesheetParser<'a>,
) -> SassResult<AstExpr> {
    let expr = parser.parse_expression(
        Some(&|p| {
            Ok(matches!(
                p.toks().peek(),
                Some(Token { kind: ';' | ')', .. })
            ))
        }),
        None,
        None,
    )?;
    Ok(expr.node)
}
