use std::{
    cell::Cell,
    ffi::OsString,
    mem,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::common::SmallOrderedMap;

static MIXIN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Maximum nesting depth allowed for recursive parsing: entering a nested
/// block's children (`with_children`) or a parenthesized/bracketed
/// sub-expression (`parse_paren_expr`, the `[` dispatch arm). Deeply nested
/// input can overflow the stack — a Rust stack overflow always aborts the
/// process, so this guard exists to reject such input with a normal error
/// instead. See `MAX_CALLABLE_RECURSION_DEPTH` in evaluate/visitor.rs for the
/// separate, much tighter limit on recursive function/mixin/content-block
/// *evaluation* — parser frames and evaluator-callable frames have very
/// different stack costs, so one shared constant would force the worst of
/// both (this is why the two are split; an earlier version of this guard
/// used a single constant and was too strict — see solo todo #123).
///
/// Sized from the two real environments grass runs in, not from how deep
/// real stylesheets nest (Bootstrap ~10 levels; sass-spec's deepest
/// legitimate fixture, non_conformant/scss/huge.hrx, nests 59). Measured
/// unguarded parser-only crash boundaries (deeply nested `a{a{a{...}}}`, no
/// user callables) on an explicit small-stack thread:
///
///   - release build, 1 MiB stack (napi's default worker-thread size):
///     survives 300, crashes at 384.
///   - debug build, 2 MiB stack (cargo test's actual default thread stack):
///     survives 224, crashes at 256.
///
/// debug+2 MiB is the binding constraint. 128 gives exactly 2x margin below
/// that 256 crash point (and >2x under release+1 MiB), while still clearing
/// huge.hrx's 59 levels with over 2x headroom. dart-sass 1.97.3 itself
/// stack-overflows on this machine around 450-500 levels of brace nesting,
/// so matching or exceeding dart-sass's own ceiling was never in tension
/// with this value.
///
/// With the default-on `stacker` feature (todo #148), `with_recursion_guard`
/// grows the *parser's own* stack on demand (see `crate::stack::maybe_grow`)
/// instead of relying on a fixed small stack. This constant could in
/// principle go much higher on parsing cost alone — and, as of todo #196, it
/// now does: **evaluation of plain nested style rules is a second, separate
/// recursion** (`Visitor::visit_ruleset` in evaluate/visitor.rs, guarded by
/// its own `MAX_STYLE_RULE_RECURSION_DEPTH` and wrapped in the same
/// `maybe_grow` helper — see that constant's doc comment) that used to be
/// the real, unguarded ceiling for the full `grass::from_string` pipeline
/// (parse + evaluate + serialize) even after this parser guard alone was
/// raised. With both chokepoints now guarded and stack-growing, this parser
/// limit and the evaluator's gate the same nesting and are kept in sync.
///
/// Historical unguarded full-pipeline crash boundaries for plain nested
/// rules (`a{a{a{...}}}`, no user callables), measured before todo #196's
/// fix (this parser guard's stack growth active, evaluator side not yet
/// guarded):
///
///   - release-napi profile, 1 MiB stack (napi's real worker-thread size,
///     the actual napi deployment ceiling): survives 370, crashes at 380.
///   - debug build, 2 MiB stack (cargo test's own default thread stack, not
///     a deployment target — see `MAX_CALLABLE_RECURSION_DEPTH`'s doc
///     comment for why this project treats debug+2 MiB as cargo-test-only):
///     survives 260, crashes at 270.
///
/// After todo #196 guarded the evaluator chokepoint too, the full-pipeline
/// crash boundary moved out dramatically: confirmed safe (no crash) at depth
/// 1500 on release-napi/1 MiB and at depth 1024 on debug/2 MiB (see
/// `MAX_STYLE_RULE_RECURSION_DEPTH`'s doc comment for the fuller
/// measurement, including where a crash reappears far out at ~12-15k depth —
/// not root-caused, time-boxed, and irrelevant at any realistic nesting
/// depth).
///
/// 1024 is set to match `MAX_STYLE_RULE_RECURSION_DEPTH` — both constants
/// gate the same plain-nesting recursion from their respective layers, so
/// keeping them equal means neither is a surprise bottleneck under the
/// other. This is a 4x increase over the post-#148, pre-#196 value (256),
/// and clears dart-sass 1.97.3's own ~450-500 level tolerance with over 2x
/// headroom — matching or exceeding dart-sass's ceiling end-to-end, not just
/// at the parser layer. Tests exercising nesting depths near this limit must
/// spawn an explicit larger-stack thread (see `is_ok_on_8mib_stack` in
/// deep_nesting.rs) so they don't crash on cargo test's own debug+2 MiB
/// thread, exactly as the existing callable-recursion tests already do. When
/// the feature is off (the wasm32 build, where `stacker` isn't supported),
/// parser stack growth never happens either, so the limit must stay at the
/// smaller value measured safe for parsing alone.
#[cfg(feature = "stacker")]
pub(crate) const MAX_PARSER_RECURSION_DEPTH: usize = 1024;
#[cfg(not(feature = "stacker"))]
pub(crate) const MAX_PARSER_RECURSION_DEPTH: usize = 128;

use codemap::{Span, Spanned};
use rustc_hash::FxHashSet;

use crate::{
    ast::*,
    common::{unvendor, Identifier, QuoteKind},
    deprecation::Deprecation,
    error::SassResult,
    lexer::Lexer,
    utils::{is_name, is_name_start, is_plain_css_import, opposite_bracket},
    ContextFlags, Options, Token,
};

use super::{
    value::{Predicate, ValueParser},
    BaseParser, DeclarationOrBuffer, ScssParser, VariableDeclOrInterpolation, RESERVED_IDENTIFIERS,
};

/// Default implementations are oriented towards the SCSS syntax, as both CSS and
/// SCSS share the behavior
pub(crate) trait StylesheetParser<'a>: BaseParser + Sized {
    // todo: make constant?
    fn is_plain_css(&self) -> bool;
    // todo: make constant?
    fn is_indented(&self) -> bool;
    fn options(&self) -> &Options<'_>;
    fn path(&self) -> &Path;
    fn empty_span(&self) -> Span;
    fn current_indentation(&self) -> usize;
    fn flags(&self) -> &ContextFlags;
    fn flags_mut(&mut self) -> &mut ContextFlags;
    fn arena(&self) -> &'a bumpalo::Bump;
    fn recursion_depth(&self) -> &Cell<usize>;
    /// Deprecation warnings discovered while parsing (e.g. `@elseif`),
    /// drained into the resulting `StyleSheet` at the end of `__parse` and
    /// replayed by `Visitor::visit_stylesheet` once a logger is available.
    fn parse_time_warnings_mut(&mut self) -> &mut Vec<(Deprecation, Span, String)>;

    /// Guards a recursive parse of a nested construct (a block's children, or
    /// a parenthesized/bracketed sub-expression), erroring instead of
    /// overflowing the stack once `MAX_RECURSION_DEPTH` is exceeded.
    fn with_recursion_guard<T>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> SassResult<T>,
    ) -> SassResult<T> {
        let depth = self.recursion_depth().get();

        if depth >= MAX_PARSER_RECURSION_DEPTH {
            return Err(("Too much nesting.", span).into());
        }

        self.recursion_depth().set(depth + 1);
        let result = crate::stack::maybe_grow(256 * 1024, 1024 * 1024, || f(self));
        self.recursion_depth().set(depth);
        result
    }

    #[allow(clippy::type_complexity)]
    const IDENTIFIER_LIKE: Option<fn(&mut Self) -> SassResult<Spanned<AstExpr<'a>>>> = None;

    /// Sets whether the indented-syntax parser should consume newlines as
    /// whitespace. No-op for SCSS and CSS parsers.
    fn set_consume_newlines(&mut self, _consume: bool) {}

    /// Returns whether the indented-syntax parser is currently consuming
    /// newlines. Always false for SCSS and CSS parsers.
    fn is_consuming_newlines(&self) -> bool {
        false
    }

    /// Convert a Vec of statements to an arena-allocated slice.
    fn alloc_stmts(&self, stmts: Vec<AstStmt<'a>>) -> &'a [AstStmt<'a>] {
        self.arena().alloc_slice_fill_iter(stmts)
    }

    fn parse_style_rule_selector(&mut self) -> SassResult<InterpolationBuilder<'a>> {
        self.almost_any_value(false)
    }

    fn expect_statement_separator(&mut self, _name: Option<&str>) -> SassResult<()> {
        self.whitespace()?;
        match self.toks().peek() {
            Some(Token {
                kind: ';' | '}', ..
            })
            | None => Ok(()),
            _ => {
                self.expect_char(';')?;
                unreachable!();
            }
        }
    }

    fn at_end_of_statement(&self) -> bool {
        matches!(
            self.toks().peek(),
            Some(Token {
                kind: ';' | '}' | '{',
                ..
            }) | None
        )
    }

    fn looking_at_children(&mut self) -> SassResult<bool> {
        Ok(matches!(self.toks().peek(), Some(Token { kind: '{', .. })))
    }

    fn scan_else(&mut self, _if_indentation: usize) -> SassResult<bool> {
        let start = self.toks().cursor();

        self.whitespace()?;

        let before_at = self.toks().cursor();

        if self.scan_char('@') {
            if self.scan_identifier("else", true)? {
                return Ok(true);
            }

            if self.scan_identifier("elseif", true)? {
                let span = self.toks_mut().span_from(before_at);
                self.parse_time_warnings_mut().push((
                    Deprecation::Elseif,
                    span,
                    "@elseif is deprecated and will not be supported in future Sass \
                     versions.\n\nRecommendation: @else if"
                        .to_string(),
                ));

                let new_cursor = self.toks().cursor() - 2;
                self.toks_mut().set_cursor(new_cursor);
                return Ok(true);
            }
        }

        self.toks_mut().set_cursor(start);

        Ok(false)
    }

    fn parse_children(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<Vec<AstStmt<'a>>> {
        self.expect_char('{')?;
        self.whitespace_without_comments();
        let mut children = Vec::new();

        let mut found_matching_brace = false;

        while let Some(tok) = self.toks().peek() {
            match tok.kind {
                '$' => children.push(AstStmt::VariableDecl(self.arena().alloc(
                    self.parse_variable_declaration_without_namespace(None, None)?,
                ))),
                '/' => match self.toks().peek_n(1) {
                    Some(Token { kind: '/', .. }) => {
                        children.push(self.parse_silent_comment()?);
                        self.whitespace_without_comments();
                    }
                    Some(Token { kind: '*', .. }) => {
                        children.push(AstStmt::LoudComment(self.parse_loud_comment()?));
                        self.whitespace_without_comments();
                    }
                    _ => children.push(child(self)?),
                },
                ';' => {
                    self.toks_mut().next();
                    self.whitespace_without_comments();
                }
                '}' => {
                    self.expect_char('}')?;
                    found_matching_brace = true;
                    break;
                }
                _ => children.push(child(self)?),
            }
        }

        if !found_matching_brace {
            return Err(("expected \"}\".", self.toks().current_span()).into());
        }

        Ok(children)
    }

    fn parse_statements(
        &mut self,
        statement: fn(&mut Self) -> SassResult<Option<AstStmt<'a>>>,
    ) -> SassResult<Vec<AstStmt<'a>>> {
        let mut stmts = Vec::new();
        self.whitespace_without_comments();
        while let Some(tok) = self.toks().peek() {
            match tok.kind {
                '$' => stmts.push(AstStmt::VariableDecl(self.arena().alloc(
                    self.parse_variable_declaration_without_namespace(None, None)?,
                ))),
                '/' => match self.toks().peek_n(1) {
                    Some(Token { kind: '/', .. }) => {
                        stmts.push(self.parse_silent_comment()?);
                        self.whitespace_without_comments();
                    }
                    Some(Token { kind: '*', .. }) => {
                        stmts.push(AstStmt::LoudComment(self.parse_loud_comment()?));
                        self.whitespace_without_comments();
                    }
                    _ => {
                        if let Some(stmt) = statement(self)? {
                            stmts.push(stmt);
                        }
                    }
                },
                ';' => {
                    self.toks_mut().next();
                    self.whitespace_without_comments();
                }
                _ => {
                    if let Some(stmt) = statement(self)? {
                        stmts.push(stmt);
                    }
                }
            }
        }

        Ok(stmts)
    }

    // todo: rename
    fn __parse(&mut self) -> SassResult<StyleSheet<'a>> {
        let mut style_sheet = StyleSheet::new(
            self.is_plain_css(),
            self.options()
                .fs
                .canonicalize(self.path())
                .unwrap_or_else(|_| self.path().to_path_buf()),
        );

        // Allow a byte-order mark at the beginning of the document.
        self.scan_char('\u{feff}');

        let body_stmts = self.parse_statements(|parser| {
            if parser.next_matches("@charset") {
                parser.expect_char('@')?;
                parser.expect_identifier("charset", false)?;
                parser.whitespace()?;
                parser.parse_string()?;
                return Ok(None);
            }

            Ok(Some(parser.parse_statement()?))
        })?;
        style_sheet.body = self.alloc_stmts(body_stmts);

        for (idx, child) in style_sheet.body.iter().enumerate() {
            match child {
                AstStmt::VariableDecl(_) | AstStmt::LoudComment(_) | AstStmt::SilentComment(_) => {
                    continue
                }
                AstStmt::Use(..) => style_sheet.uses.push(idx),
                AstStmt::Forward(..) => style_sheet.forwards.push(idx),
                _ => break,
            }
        }

        style_sheet.collect_pre_declared_global_variables();
        style_sheet.collect_configurable_variables();
        style_sheet.parse_time_warnings = mem::take(self.parse_time_warnings_mut());

        Ok(style_sheet)
    }

    fn looking_at_expression(&mut self) -> bool {
        let character = if let Some(c) = self.toks().peek() {
            c
        } else {
            return false;
        };

        match character.kind {
            '.' => !matches!(self.toks().peek_n(1), Some(Token { kind: '.', .. })),
            '!' => match self.toks().peek_n(1) {
                Some(Token {
                    kind: 'i' | 'I', ..
                })
                | None => true,
                Some(Token { kind, .. }) => kind.is_ascii_whitespace(),
            },
            '(' | '/' | '[' | '\'' | '"' | '#' | '+' | '-' | '\\' | '$' | '&' | '%' => true,
            c => is_name_start(c) || c.is_ascii_digit(),
        }
    }

    fn parse_argument_declaration(&mut self) -> SassResult<ArgumentDeclaration<'a>> {
        self.expect_char('(')?;
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        let mut arguments = Vec::new();
        let mut named = FxHashSet::default();

        let mut rest_argument: Option<Identifier> = None;

        while self.toks_mut().next_char_is('$') {
            let name_start = self.toks().cursor();
            let name = Identifier::from(self.parse_variable_name()?);
            let name_span = self.toks_mut().span_from(name_start);
            self.whitespace()?;

            let mut default_value: Option<AstExpr<'a>> = None;

            if self.scan_char(':') {
                self.whitespace()?;
                default_value = Some(self.parse_expression_until_comma(false)?.node);
            } else if self.scan_char('.') {
                self.expect_char('.')?;
                self.expect_char('.')?;
                self.whitespace()?;
                rest_argument = Some(name);
                self.scan_char(',');
                self.whitespace()?;
                break;
            }

            arguments.push(Argument {
                name,
                default: default_value,
            });

            if !named.insert(name) {
                self.set_consume_newlines(was_consuming_newlines);
                return Err(("Duplicate argument.", name_span).into());
            }

            if !self.scan_char(',') {
                break;
            }
            self.whitespace()?;
        }
        self.expect_char(')')?;
        self.set_consume_newlines(was_consuming_newlines);

        Ok(ArgumentDeclaration {
            args: self.arena().alloc_slice_fill_iter(arguments),
            rest: rest_argument,
        })
    }

    fn plain_at_rule_name(&mut self) -> SassResult<String> {
        self.expect_char('@')?;
        let name = self.parse_identifier(false, false)?;
        self.whitespace()?;
        Ok(name)
    }

    fn with_children(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<Spanned<Vec<AstStmt<'a>>>> {
        let start = self.toks().cursor();
        let guard_span = self.toks().current_span();
        let children =
            self.with_recursion_guard(guard_span, |parser| parser.parse_children(child))?;
        let span = self.toks_mut().span_from(start);
        self.whitespace_without_comments();
        Ok(Spanned {
            node: children,
            span,
        })
    }

    fn parse_at_root_query(&mut self) -> SassResult<InterpolationBuilder<'a>> {
        let mut buffer = InterpolationBuilder::new();
        self.expect_char('(')?;
        buffer.add_char('(');

        // In indented syntax, allow newlines inside @at-root query parens
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        buffer.add_expr(self.parse_expression(None, None, None)?);

        if self.scan_char(':') {
            self.whitespace()?;
            buffer.add_char(':');
            buffer.add_char(' ');
            buffer.add_expr(self.parse_expression(None, None, None)?);
        }

        self.set_consume_newlines(was_consuming_newlines);
        self.expect_char(')')?;
        self.whitespace()?;
        buffer.add_char(')');

        Ok(buffer)
    }

    fn parse_at_root_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        Ok(AstStmt::AtRootRule(if self.toks_mut().next_char_is('(') {
            let query_start = self.toks().cursor();
            let query = self.parse_at_root_query()?;
            let query_span = self.toks_mut().span_from(query_start);
            self.whitespace()?;
            let children = self.with_children(Self::parse_statement)?.node;

            AstAtRootRule {
                query: Some(Spanned {
                    node: query.finish(self.arena()),
                    span: query_span,
                }),
                body: self.alloc_stmts(children),
                span: self.toks_mut().span_from(start),
            }
        } else if self.looking_at_children()? {
            let children = self.with_children(Self::parse_statement)?.node;
            AstAtRootRule {
                query: None,
                body: self.alloc_stmts(children),
                span: self.toks_mut().span_from(start),
            }
        } else if self.is_indented() && self.at_end_of_statement() {
            // Empty @at-root with no children in indented syntax
            AstAtRootRule {
                query: None,
                body: &[],
                span: self.toks_mut().span_from(start),
            }
        } else {
            let child = self.parse_style_rule(None, None)?;
            AstAtRootRule {
                query: None,
                body: self.alloc_stmts(vec![child]),
                span: self.toks_mut().span_from(start),
            }
        }))
    }

    fn parse_content_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        if !self.flags().in_mixin() {
            return Err((
                "@content is only allowed within mixin declarations.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        self.whitespace()?;

        let args = if self.toks_mut().next_char_is('(') {
            self.parse_argument_invocation(true, false)?
        } else {
            ArgumentInvocation::empty(self.toks().current_span())
        };

        self.expect_statement_separator(Some("@content rule"))?;

        self.flags_mut().set(ContextFlags::FOUND_CONTENT_RULE, true);

        Ok(AstStmt::ContentRule(self.arena().alloc(AstContentRule { args })))
    }

    fn parse_debug_rule(&mut self) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newline between @debug and its expression
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        let value = self.parse_expression(None, None, None)?;
        self.expect_statement_separator(Some("@debug rule"))?;

        Ok(AstStmt::Debug(AstDebugRule {
            value: value.node,
            span: value.span,
        }))
    }

    fn parse_each_rule(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<AstStmt<'a>> {
        let was_in_control_directive = self.flags().in_control_flow();
        self.flags_mut().set(ContextFlags::IN_CONTROL_FLOW, true);

        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        let mut variables = vec![Identifier::from(self.parse_variable_name()?)];
        self.whitespace()?;
        while self.scan_char(',') {
            self.whitespace()?;
            variables.push(Identifier::from(self.parse_variable_name()?));
            self.whitespace()?;
        }

        self.expect_identifier("in", false)?;
        self.whitespace()?;

        self.set_consume_newlines(was_consuming_newlines);
        let list = self.parse_expression(None, None, None)?;
        let list_span = list.span;
        let list = list.node;

        let body = self.with_children(child)?.node;

        self.flags_mut()
            .set(ContextFlags::IN_CONTROL_FLOW, was_in_control_directive);

        Ok(AstStmt::Each(self.arena().alloc(AstEach {
            variables,
            list,
            list_span,
            body: self.alloc_stmts(body),
        })))
    }

    fn parse_disallowed_at_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        self.almost_any_value(false)?;
        Err((
            "This at-rule is not allowed here.",
            self.toks_mut().span_from(start),
        )
            .into())
    }

    fn parse_error_rule(&mut self) -> SassResult<AstStmt<'a>> {
        let value = self.parse_expression(None, None, None)?;
        self.expect_statement_separator(Some("@error rule"))?;
        Ok(AstStmt::ErrorRule(AstErrorRule {
            value: value.node,
            span: value.span,
        }))
    }

    fn parse_extend_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        if !self.flags().in_style_rule()
            && !self.flags().in_mixin()
            && !self.flags().in_content_block()
        {
            return Err((
                "@extend may only be used within style rules.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        // In indented syntax, allow newline before selector
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);

        let value = self.almost_any_value(false)?;

        let is_optional = self.scan_char('!');

        if is_optional {
            self.expect_identifier("optional", false)?;
        }

        self.expect_statement_separator(Some("@extend rule"))?;

        Ok(AstStmt::Extend(AstExtendRule {
            value: value.finish(self.arena()),
            is_optional,
            span: self.toks_mut().span_from(start),
        }))
    }

    fn parse_for_rule(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<AstStmt<'a>> {
        let was_in_control_directive = self.flags().in_control_flow();
        self.flags_mut().set(ContextFlags::IN_CONTROL_FLOW, true);

        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        let var_start = self.toks().cursor();
        let variable = Spanned {
            node: Identifier::from(self.parse_variable_name()?),
            span: self.toks_mut().span_from(var_start),
        };
        self.whitespace()?;

        self.expect_identifier("from", false)?;
        self.whitespace()?;

        let exclusive: Cell<Option<bool>> = Cell::new(None);

        let from = self.parse_expression(
            Some(&|parser| {
                if !parser.looking_at_identifier() {
                    return Ok(false);
                }
                Ok(if parser.scan_identifier("to", false)? {
                    exclusive.set(Some(true));
                    true
                } else if parser.scan_identifier("through", false)? {
                    exclusive.set(Some(false));
                    true
                } else {
                    false
                })
            }),
            None,
            None,
        )?;

        let is_exclusive = match exclusive.get() {
            Some(b) => b,
            None => {
                return Err((
                    "Expected \"to\" or \"through\".",
                    self.toks().current_span(),
                )
                    .into())
            }
        };

        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);

        let to = self.parse_expression(None, None, None)?;

        let body = self.with_children(child)?.node;

        self.flags_mut()
            .set(ContextFlags::IN_CONTROL_FLOW, was_in_control_directive);

        Ok(AstStmt::For(self.arena().alloc(AstFor {
            variable,
            from,
            to,
            is_exclusive,
            body: self.alloc_stmts(body),
        })))
    }

    fn parse_function_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newlines around function name
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        // CSS custom functions (@function --name()) should be passed through as-is
        let before_name = self.toks().cursor();
        let name_start = self.toks().cursor();
        // Parse without normalization first to get the raw name for validation
        let raw_name = self.parse_identifier(false, false)?;
        let name_span = self.toks_mut().span_from(name_start);
        // Normalize underscores to hyphens for actual use
        let name = raw_name.replace('_', "-");
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);

        if raw_name.starts_with("--") {
            // CSS custom function: rewind to before the name and parse as unknown at-rule
            self.toks_mut().set_cursor(before_name);
            let at_rule_name = InterpolationBuilder::new_plain("function".to_string());
            return self.unknown_at_rule(at_rule_name, start);
        }

        let arguments = self.parse_argument_declaration()?;

        if self.flags().in_mixin() || self.flags().in_content_block() {
            return Err((
                "Mixins may not contain function declarations.",
                self.toks_mut().span_from(start),
            )
                .into());
        } else if self.flags().in_control_flow() {
            return Err((
                "Functions may not be declared in control directives.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        let lower_name = name.to_ascii_lowercase();
        if lower_name == "type" {
            return Err((
                "This name is reserved for the plain-CSS function.",
                name_span,
            )
                .into());
        }

        // Use the raw (un-normalized) name for the reserved check so that
        // names like `-moz_calc` and `_moz-calc` are not incorrectly rejected.
        if RESERVED_IDENTIFIERS.contains(&unvendor(&raw_name)) {
            return Err(("Invalid function name.", self.toks_mut().span_from(start)).into());
        }

        self.whitespace()?;

        let children = self.with_children(Self::function_child)?.node;

        Ok(AstStmt::FunctionDecl(AstFunctionDecl {
            name: Spanned {
                node: Identifier::from(name),
                span: name_span,
            },
            arguments,
            body: self.alloc_stmts(children),
        }))
    }

    fn parse_variable_declaration_with_namespace(&mut self) -> SassResult<AstVariableDecl<'a>> {
        let start = self.toks().cursor();
        let namespace = self.parse_identifier(false, false)?;
        let namespace_span = self.toks_mut().span_from(start);
        self.expect_char('.')?;
        self.parse_variable_declaration_without_namespace(
            Some(Spanned {
                node: Identifier::from(namespace),
                span: namespace_span,
            }),
            Some(start),
        )
    }

    fn function_child(&mut self) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();
        if !self.toks_mut().next_char_is('@') {
            match self.parse_variable_declaration_with_namespace() {
                Ok(decl) => return Ok(AstStmt::VariableDecl(self.arena().alloc(decl))),
                Err(e) => {
                    self.toks_mut().set_cursor(start);
                    let stmt = match self.parse_declaration_or_style_rule() {
                        Ok(stmt) => stmt,
                        Err(..) => return Err(e),
                    };

                    let (is_style_rule, span) = match stmt {
                        AstStmt::RuleSet(ruleset) => (true, ruleset.span),
                        AstStmt::Style(style) => (false, style.span),
                        _ => unreachable!(),
                    };

                    return Err((
                        format!(
                            "@function rules may not contain {}.",
                            if is_style_rule {
                                "style rules"
                            } else {
                                "declarations"
                            }
                        ),
                        span,
                    )
                        .into());
                }
            }
        }

        match self.plain_at_rule_name()?.as_str() {
            "debug" => self.parse_debug_rule(),
            "each" => self.parse_each_rule(Self::function_child),
            "else" => self.parse_disallowed_at_rule(start),
            "error" => self.parse_error_rule(),
            "for" => self.parse_for_rule(Self::function_child),
            "if" => self.parse_if_rule(Self::function_child),
            "return" => self.parse_return_rule(),
            "warn" => self.parse_warn_rule(),
            "while" => self.parse_while_rule(Self::function_child),
            _ => self.parse_disallowed_at_rule(start),
        }
    }

    fn parse_if_rule(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<AstStmt<'a>> {
        let if_indentation = self.current_indentation();

        let was_in_control_directive = self.flags().in_control_flow();
        self.flags_mut().set(ContextFlags::IN_CONTROL_FLOW, true);
        // In indented syntax, allow newline before condition
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        let condition = self.parse_expression(None, None, None)?.node;
        let body = self.parse_children(child)?;
        self.whitespace_without_comments();

        let mut clauses = vec![AstIfClause { condition, body: self.alloc_stmts(body) }];

        let mut last_clause: Option<&'a [AstStmt<'a>]> = None;

        while self.scan_else(if_indentation)? {
            self.whitespace()?;
            if self.scan_identifier("if", false)? {
                // In indented syntax, allow newline before else-if condition
                self.set_consume_newlines(true);
                self.whitespace()?;
                self.set_consume_newlines(was_consuming_newlines);
                let condition = self.parse_expression(None, None, None)?.node;
                let body = self.parse_children(child)?;
                clauses.push(AstIfClause { condition, body: self.alloc_stmts(body) });
            } else {
                let else_body = self.parse_children(child)?;
                last_clause = Some(self.alloc_stmts(else_body));
                break;
            }
        }

        self.flags_mut()
            .set(ContextFlags::IN_CONTROL_FLOW, was_in_control_directive);
        self.whitespace_without_comments();

        Ok(AstStmt::If(AstIf {
            if_clauses: self.arena().alloc_slice_fill_iter(clauses),
            else_clause: last_clause,
        }))
    }

    fn try_parse_import_supports_function(&mut self) -> SassResult<Option<AstSupportsCondition<'a>>> {
        if !self.looking_at_interpolated_identifier() {
            return Ok(None);
        }

        let start = self.toks().cursor();
        let name = self.parse_interpolated_identifier()?;
        debug_assert!(name.as_plain() != Some("not"));

        if !self.scan_char('(') {
            self.toks_mut().set_cursor(start);
            return Ok(None);
        }

        let value = self.parse_interpolated_declaration_value(true, true, true)?;
        self.expect_char(')')?;

        Ok(Some(AstSupportsCondition::Function {
            name: name.finish(self.arena()),
            args: value.finish(self.arena()),
        }))
    }

    fn parse_import_supports_query(&mut self) -> SassResult<AstSupportsCondition<'a>> {
        self.whitespace()?;
        Ok(if self.scan_identifier("not", false)? {
            self.whitespace()?;
            AstSupportsCondition::Negation(self.arena().alloc(self.supports_condition_in_parens()?))
        } else if self.toks_mut().next_char_is('(') {
            self.parse_supports_condition()?
        } else {
            match self.try_parse_import_supports_function()? {
                Some(function) => function,
                None => {
                    let start = self.toks().cursor();
                    let name = self.parse_expression(None, None, None)?;
                    self.expect_char(':')?;
                    self.supports_declaration_value(name.node, start)?
                }
            }
        })
    }

    fn try_import_modifiers(&mut self) -> SassResult<Option<InterpolationBuilder<'a>>> {
        // Exit before allocating anything if we're not looking at any modifiers, as
        // is the most common case.
        if !self.looking_at_interpolated_identifier() && !self.toks_mut().next_char_is('(') {
            return Ok(None);
        }

        let mut buffer = InterpolationBuilder::new();

        loop {
            if self.looking_at_interpolated_identifier() {
                if !buffer.is_empty() {
                    buffer.add_char(' ');
                }

                let identifier = self.parse_interpolated_identifier()?;
                let name = identifier.as_plain().map(str::to_ascii_lowercase);
                buffer.add_interpolation(identifier);

                if name.as_deref() != Some("and") && self.scan_char('(') {
                    let was_cn = self.is_consuming_newlines();
                    self.set_consume_newlines(true);

                    if name.as_deref() == Some("supports") {
                        let query = self.parse_import_supports_query()?;
                        let is_declaration =
                            matches!(query, AstSupportsCondition::Declaration { .. });

                        if !is_declaration {
                            buffer.add_char('(');
                        }

                        buffer.add_expr(AstExpr::Supports(self.arena().alloc(query)).span(self.empty_span()));

                        if !is_declaration {
                            buffer.add_char(')');
                        }
                    } else {
                        buffer.add_char('(');
                        buffer.add_interpolation(
                            self.parse_interpolated_declaration_value(true, true, true)?,
                        );
                        buffer.add_char(')');
                    }

                    self.expect_char(')')?;
                    self.set_consume_newlines(was_cn);
                    self.whitespace()?;
                } else {
                    self.whitespace()?;
                    if self.scan_char(',') {
                        buffer.add_char(',');
                        buffer.add_char(' ');
                        buffer.add_interpolation(self.parse_media_query_list()?);
                        return Ok(Some(buffer));
                    }
                }
            } else if self.toks_mut().next_char_is('(') {
                if !buffer.is_empty() {
                    buffer.add_char(' ');
                }

                buffer.add_interpolation(self.parse_media_query_list()?);
                return Ok(Some(buffer));
            } else {
                return Ok(Some(buffer));
            }
        }
    }

    fn try_url_contents(&mut self, name: Option<&str>) -> SassResult<Option<InterpolationBuilder<'a>>> {
        let start = self.toks().cursor();
        if !self.scan_char('(') {
            return Ok(None);
        }
        self.whitespace_without_comments();

        // Match Ruby Sass's behavior: parse a raw URL() if possible, and if not
        // backtrack and re-parse as a function expression.
        let mut buffer = InterpolationBuilder::new();
        buffer.add_string(name.unwrap_or("url").to_owned());
        buffer.add_char('(');

        while let Some(next) = self.toks().peek() {
            match next.kind {
                '\\' => buffer.add_string(self.parse_escape(false)?),
                '!' | '%' | '&' | '*'..='~' | '\u{80}'..=char::MAX => {
                    self.toks_mut().next();
                    buffer.add_char(next.kind);
                }
                '#' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) {
                        let interpolation = self.parse_single_interpolation()?;
                        buffer.add_interpolation(interpolation);
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(next.kind);
                    }
                }
                ')' => {
                    self.toks_mut().next();
                    buffer.add_char(next.kind);
                    return Ok(Some(buffer));
                }
                ' ' | '\t' | '\n' | '\r' => {
                    self.whitespace_without_comments();
                    if !self.toks_mut().next_char_is(')') {
                        break;
                    }
                }
                _ => break,
            }
        }

        self.toks_mut().set_cursor(start);

        Ok(None)
    }

    fn parse_dynamic_url(&mut self) -> SassResult<AstExpr<'a>> {
        let start = self.toks().cursor();
        self.expect_identifier("url", false)?;

        Ok(match self.try_url_contents(None)? {
            Some(contents) => AstExpr::String(
                StringExpr(contents.finish(self.arena()), QuoteKind::None),
                self.toks_mut().span_from(start),
            ),
            None => AstExpr::InterpolatedFunction(self.arena().alloc(InterpolatedFunction {
                name: InterpolationBuilder::new_plain("url".to_owned()).finish(self.arena()),
                arguments: self.parse_argument_invocation(false, false)?,
                span: self.toks_mut().span_from(start),
            })),
        })
    }

    fn parse_import_argument(&mut self, start: usize) -> SassResult<AstImport<'a>> {
        if self.toks_mut().next_char_is('u') || self.toks_mut().next_char_is('U') {
            // In indented syntax, only try url() if the identifier is actually
            // "url" (not another identifier starting with 'u' like "unquoted").
            let try_url = if self.is_indented() {
                let saved = self.toks().cursor();
                let is_url = self.scan_identifier("url", false)?
                    && self.toks().peek().is_some_and(|t| t.kind == '(');
                self.toks_mut().set_cursor(saved);
                is_url
            } else {
                true
            };

            if try_url {
                let url = self.parse_dynamic_url()?;
                self.whitespace()?;
                let modifiers = self
                    .try_import_modifiers()?
                    .map(|m| m.finish(self.arena()));
                return Ok(AstImport::Plain(AstPlainCssImport {
                    url: InterpolationBuilder::new_with_expr(
                        url.span(self.toks_mut().span_from(start)),
                    )
                    .finish(self.arena()),
                    modifiers,
                    span: self.toks_mut().span_from(start),
                }));
            }
        }

        // In indented syntax, try parsing an unquoted URL if the next char
        // is not a quote character.
        if self.is_indented()
            && !self.toks_mut().next_char_is('"')
            && !self.toks_mut().next_char_is('\'')
        {
            let start = self.toks().cursor();
            let mut url = String::new();
            while let Some(tok) = self.toks().peek() {
                if matches!(
                    tok.kind,
                    ' ' | '\t' | '\n' | '\r' | ',' | ';'
                ) || tok.kind == ')' {
                    break;
                }
                url.push(tok.kind);
                self.toks_mut().next();
            }
            self.whitespace()?;
            let modifiers = self
                .try_import_modifiers()?
                .map(|m| m.finish(self.arena()));
            let span = self.toks_mut().span_from(start);

            // Wrap the unquoted URL in double quotes, matching dart-sass behavior
            let quoted_url = format!("\"{}\"", url);

            if is_plain_css_import(&url) || modifiers.is_some() {
                return Ok(AstImport::Plain(AstPlainCssImport {
                    url: InterpolationBuilder::new_plain(quoted_url).finish(self.arena()),
                    modifiers,
                    span,
                }));
            } else {
                return Ok(AstImport::Sass(AstSassImport { url, span }));
            }
        }

        let start = self.toks().cursor();
        let url = self.parse_string()?;
        let raw_url = self.toks().raw_text(start);
        self.whitespace()?;
        let modifiers = self
            .try_import_modifiers()?
            .map(|m| m.finish(self.arena()));

        let span = self.toks_mut().span_from(start);

        if is_plain_css_import(&url) || modifiers.is_some() {
            Ok(AstImport::Plain(AstPlainCssImport {
                url: InterpolationBuilder::new_plain(raw_url).finish(self.arena()),
                modifiers,
                span,
            }))
        } else {
            Ok(AstImport::Sass(AstSassImport { url, span }))
        }
    }

    fn parse_import_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        let mut imports = Vec::new();

        loop {
            self.whitespace()?;
            let argument = self.parse_import_argument(self.toks().cursor())?;

            if let AstImport::Sass(ref dynamic_import) = argument {
                self.parse_time_warnings_mut().push((
                    Deprecation::Import,
                    dynamic_import.span,
                    "Sass @import rules are deprecated and will be removed in Dart Sass \
                     3.0.0.\n\nMore info and automated migrator: https://sass-lang.com/d/import"
                        .to_string(),
                ));
            }

            // todo: _inControlDirective
            if (self.flags().in_control_flow() || self.flags().in_mixin()) && argument.is_dynamic()
            {
                self.parse_disallowed_at_rule(start)?;
            }

            imports.push(argument);
            self.whitespace()?;

            if !self.scan_char(',') {
                break;
            }
        }

        self.expect_statement_separator(Some("@import rule"))?;

        Ok(AstStmt::ImportRule(AstImportRule { imports }))
    }

    fn parse_public_identifier(&mut self) -> SassResult<String> {
        let start = self.toks().cursor();
        let ident = self.parse_identifier(true, false)?;
        Self::assert_public(&ident, self.toks_mut().span_from(start))?;

        Ok(ident)
    }

    fn parse_include_rule(&mut self) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newline before mixin name
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);

        let mut namespace: Option<Spanned<Identifier>> = None;

        let name_start = self.toks().cursor();
        let mut name = self.parse_identifier(false, false)?;

        if self.scan_char('.') {
            let namespace_span = self.toks_mut().span_from(name_start);
            namespace = Some(Spanned {
                node: Identifier::from(name),
                span: namespace_span,
            });
            name = self.parse_public_identifier()?;
        }

        let name_span = self.toks_mut().span_from(name_start);

        if name.starts_with("--") {
            return Err((
                "Sass @mixin names beginning with -- are forbidden for forward-compatibility with plain CSS mixins.",
                name_span,
            )
                .into());
        }

        let name = Identifier::from(name);

        self.whitespace()?;

        let args = if self.toks_mut().next_char_is('(') {
            self.parse_argument_invocation(true, false)?
        } else {
            ArgumentInvocation::empty(self.toks().current_span())
        };

        self.whitespace()?;

        let content_args = if self.scan_identifier("using", false)? {
            // In indented syntax, allow newline between "using" and arg declaration
            let was_cn = self.is_consuming_newlines();
            self.set_consume_newlines(true);
            self.whitespace()?;
            self.set_consume_newlines(was_cn);
            let args = self.parse_argument_declaration()?;
            self.whitespace()?;
            Some(args)
        } else {
            None
        };

        let mut content_block: Option<AstContentBlock<'a>> = None;

        if content_args.is_some() || self.looking_at_children()? {
            let content_args = content_args.unwrap_or_else(ArgumentDeclaration::empty);
            let was_in_content_block = self.flags().in_content_block();
            self.flags_mut().set(ContextFlags::IN_CONTENT_BLOCK, true);
            let body = self.with_children(Self::parse_statement)?.node;
            content_block = Some(AstContentBlock {
                args: content_args,
                body: self.alloc_stmts(body),
            });
            self.flags_mut()
                .set(ContextFlags::IN_CONTENT_BLOCK, was_in_content_block);
        } else {
            self.expect_statement_separator(None)?;
        }

        Ok(AstStmt::Include(self.arena().alloc(AstInclude {
            namespace,
            name: Spanned {
                node: name,
                span: name_span,
            },
            args,
            content: content_block,
            span: name_span,
        })))
    }

    fn parse_media_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        let query_start = self.toks().cursor();
        let query = self.parse_media_query_list()?;
        let query_span = self.toks_mut().span_from(query_start);

        let body = self.with_children(Self::parse_statement)?.node;

        Ok(AstStmt::Media(AstMedia {
            query: query.finish(self.arena()),
            query_span,
            body: self.alloc_stmts(body),
            span: self.toks_mut().span_from(start),
        }))
    }

    fn parse_interpolated_string(&mut self) -> SassResult<Spanned<StringExpr<'a>>> {
        let start = self.toks().cursor();
        let quote = match self.toks_mut().next() {
            Some(Token {
                kind: kind @ ('"' | '\''),
                ..
            }) => kind,
            Some(..) | None => unreachable!("Expected string."),
        };

        let mut buffer = InterpolationBuilder::new();

        let mut found_match = false;

        while let Some(next) = self.toks().peek() {
            match next.kind {
                c if c == quote => {
                    self.toks_mut().next();
                    found_match = true;
                    break;
                }
                '\n' => break,
                '\\' => {
                    match self.toks().peek_n(1) {
                        // todo: if (second == $cr) scanner.scanChar($lf);
                        // we basically need to stop normalizing to gain parity
                        Some(Token { kind: '\n', .. }) => {
                            self.toks_mut().next();
                            self.toks_mut().next();
                        }
                        _ => buffer.add_char(self.consume_escaped_char()?),
                    }
                }
                '#' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) {
                        buffer.add_interpolation(self.parse_single_interpolation()?);
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(next.kind);
                    }
                }
                _ => {
                    buffer.add_char(next.kind);
                    self.toks_mut().next();
                }
            }
        }

        if !found_match {
            return Err((
                format!("Expected {quote}.", quote = quote),
                self.toks().current_span(),
            )
                .into());
        }

        Ok(Spanned {
            node: StringExpr(buffer.finish(self.arena()), QuoteKind::Quoted),
            span: self.toks_mut().span_from(start),
        })
    }

    fn parse_return_rule(&mut self) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newline before value
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        let value = self.parse_expression(None, None, None)?;
        self.expect_statement_separator(None)?;
        Ok(AstStmt::Return(AstReturn {
            val: value.node,
            span: value.span,
        }))
    }

    fn parse_mixin_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newline before mixin name
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        // Parse raw name first to check for CSS custom mixin prefix
        let _raw_name_start = self.toks().cursor();
        let raw_name = self.parse_identifier(false, false)?;
        if raw_name.starts_with("--") {
            return Err((
                "Sass @mixin names beginning with -- are forbidden for forward-compatibility with plain CSS mixins.",
                self.toks_mut().span_from(start),
            ).into());
        }
        let name = Identifier::from(raw_name.replace('_', "-"));
        self.whitespace()?;
        let args = if self.toks_mut().next_char_is('(') {
            self.parse_argument_declaration()?
        } else {
            ArgumentDeclaration::empty()
        };

        if self.flags().in_mixin() || self.flags().in_content_block() {
            return Err((
                "Mixins may not contain mixin declarations.",
                self.toks_mut().span_from(start),
            )
                .into());
        } else if self.flags().in_control_flow() {
            return Err((
                "Mixins may not be declared in control directives.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        self.whitespace()?;

        let old_found_content_rule = self.flags().found_content_rule();
        self.flags_mut()
            .set(ContextFlags::FOUND_CONTENT_RULE, false);
        self.flags_mut().set(ContextFlags::IN_MIXIN, true);

        let body = self.with_children(Self::parse_statement)?.node;

        let has_content = self.flags_mut().found_content_rule();

        self.flags_mut()
            .set(ContextFlags::FOUND_CONTENT_RULE, old_found_content_rule);
        self.flags_mut().set(ContextFlags::IN_MIXIN, false);

        Ok(AstStmt::Mixin(AstMixin {
            name,
            args,
            body: self.alloc_stmts(body),
            has_content,
            id: MIXIN_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }))
    }

    fn unknown_at_rule(&mut self, name: InterpolationBuilder<'a>, start: usize) -> SassResult<AstStmt<'a>> {
        let was_in_unknown_at_rule = self.flags().in_unknown_at_rule();
        self.flags_mut().set(ContextFlags::IN_UNKNOWN_AT_RULE, true);

        // Strip comments from @-moz-document values (dart-sass compatibility)
        let omit_comments = name.as_plain()
            .is_some_and(|n| n.eq_ignore_ascii_case("-moz-document"));

        let value: Option<Interpolation<'a>> =
            if !self.toks_mut().next_char_is('!') && !self.at_end_of_statement() {
                Some(self.almost_any_value(omit_comments)?.finish(self.arena()))
            } else {
                None
            };

        // CSS custom function: @function --name() or @FUNCTION --name()
        let is_css_function = name.as_plain()
            .is_some_and(|n| n.eq_ignore_ascii_case("function"))
            && value.as_ref().is_some_and(|v| v.initial_plain().starts_with("--"));
        let was_in_css_function = self.flags().in_css_function_body();
        if is_css_function {
            self.flags_mut().set(ContextFlags::IN_CSS_FUNCTION_BODY, true);
        }

        let children = if self.looking_at_children()? {
            Some(self.with_children(Self::parse_statement)?.node)
        } else {
            self.expect_statement_separator(None)?;
            None
        };

        self.flags_mut()
            .set(ContextFlags::IN_UNKNOWN_AT_RULE, was_in_unknown_at_rule);
        if is_css_function {
            self.flags_mut()
                .set(ContextFlags::IN_CSS_FUNCTION_BODY, was_in_css_function);
        }

        let span = self.toks_mut().span_from(start);

        if omit_comments
            && value
                .as_ref()
                .is_some_and(moz_document_prelude_needs_deprecation_warning)
        {
            self.parse_time_warnings_mut().push((
                Deprecation::MozDocument,
                span,
                "@-moz-document is deprecated and support will be removed in Dart Sass \
                 2.0.0.\n\nFor details, see https://sass-lang.com/d/moz-document."
                    .to_string(),
            ));
        }

        Ok(AstStmt::UnknownAtRule(self.arena().alloc(AstUnknownAtRule {
            name: name.finish(self.arena()),
            value,
            body: children.map(|c| &*self.arena().alloc_slice_fill_iter(c)),
            span,
        })))
    }

    fn try_supports_operation(
        &mut self,
        interpolation: &InterpolationBuilder<'a>,
        _start: usize,
    ) -> SassResult<Option<AstSupportsCondition<'a>>> {
        if interpolation.contents.len() != 1 {
            return Ok(None);
        }

        let expression = match interpolation.contents.first() {
            Some(InterpolationPartBuilder::Expr(e)) => e,
            Some(InterpolationPartBuilder::String(..)) => return Ok(None),
            None => unreachable!(),
        };

        let before_whitespace = self.toks().cursor();
        self.whitespace()?;

        let mut operation: Option<AstSupportsCondition<'a>> = None;
        let mut operator: Option<String> = None;

        while self.looking_at_identifier() {
            if let Some(operator) = &operator {
                self.expect_identifier(operator, false)?;
            } else if self.scan_identifier("and", false)? {
                operator = Some("and".to_owned());
            } else if self.scan_identifier("or", false)? {
                operator = Some("or".to_owned());
            } else {
                self.toks_mut().set_cursor(before_whitespace);
                return Ok(None);
            }

            self.whitespace()?;

            let right = self.supports_condition_in_parens()?;
            operation = Some(AstSupportsCondition::Operation {
                left: self.arena().alloc(operation.unwrap_or_else(|| {
                    AstSupportsCondition::Interpolation(expression.clone().node)
                })),
                operator: operator.clone(),
                right: self.arena().alloc(right),
            });
            self.whitespace()?;
        }

        Ok(operation)
    }

    fn supports_declaration_value(
        &mut self,
        name: AstExpr<'a>,
        start: usize,
    ) -> SassResult<AstSupportsCondition<'a>> {
        let value = match &name {
            AstExpr::String(StringExpr(text, QuoteKind::None), ..)
                if text.initial_plain().starts_with("--") =>
            {
                let text = self.parse_interpolated_declaration_value(false, false, true)?;
                AstExpr::String(
                    StringExpr(text.finish(self.arena()), QuoteKind::None),
                    self.toks_mut().span_from(start),
                )
            }
            _ => {
                self.whitespace()?;
                self.parse_expression(None, None, None)?.node
            }
        };

        Ok(AstSupportsCondition::Declaration { name, value })
    }

    fn supports_condition_in_parens(&mut self) -> SassResult<AstSupportsCondition<'a>> {
        let start = self.toks().cursor();

        if self.looking_at_interpolated_identifier() {
            let identifier = self.parse_interpolated_identifier()?;
            let ident_span = self.toks_mut().span_from(start);

            if identifier.as_plain().unwrap_or("").eq_ignore_ascii_case("not") {
                return Err((r#""not" is not a valid identifier here."#, ident_span).into());
            }

            if self.scan_char('(') {
                let was_cn = self.is_consuming_newlines();
                self.set_consume_newlines(true);
                let arguments = self.parse_interpolated_declaration_value(true, true, true)?;
                self.expect_char(')')?;
                self.set_consume_newlines(was_cn);
                return Ok(AstSupportsCondition::Function {
                    name: identifier.finish(self.arena()),
                    args: arguments.finish(self.arena()),
                });
            } else if identifier.contents.len() != 1
                || !matches!(
                    identifier.contents.first(),
                    Some(InterpolationPartBuilder::Expr(..))
                )
            {
                return Err(("Expected @supports condition.", ident_span).into());
            } else {
                match identifier.contents.first() {
                    Some(InterpolationPartBuilder::Expr(e)) => {
                        return Ok(AstSupportsCondition::Interpolation(e.clone().node))
                    }
                    _ => unreachable!(),
                }
            }
        }

        self.expect_char('(')?;
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        if self.scan_identifier("not", false)? {
            self.whitespace()?;
            let condition = self.supports_condition_in_parens()?;
            self.expect_char(')')?;
            self.set_consume_newlines(was_consuming_newlines);
            return Ok(AstSupportsCondition::Negation(self.arena().alloc(condition)));
        } else if self.toks_mut().next_char_is('(') {
            let condition = self.parse_supports_condition()?;
            self.expect_char(')')?;
            self.set_consume_newlines(was_consuming_newlines);
            return Ok(condition);
        }

        // Unfortunately, we may have to backtrack here. The grammar is:
        //
        //       Expression ":" Expression
        //     | InterpolatedIdentifier InterpolatedAnyValue?
        //
        // These aren't ambiguous because this `InterpolatedAnyValue` is forbidden
        // from containing a top-level colon, but we still have to parse the full
        // expression to figure out if there's a colon after it.
        //
        // We could avoid the overhead of a full expression parse by looking ahead
        // for a colon (outside of balanced brackets), but in practice we expect the
        // vast majority of real uses to be `Expression ":" Expression`, so it makes
        // sense to parse that case faster in exchange for less code complexity and
        // a slower backtracking case.

        let name_start = self.toks().cursor();
        let was_in_parens = self.flags().in_parens();

        let expr = self.parse_expression(None, None, None);
        let found_colon = self.expect_char(':');
        let name: AstExpr<'a> = match (expr, found_colon) {
            (Ok(val), Ok(..)) => {
                val.node
            }
            (Ok(..), Err(e)) | (Err(e), Ok(..)) | (Err(e), Err(..)) => {
                self.toks_mut().set_cursor(name_start);
                self.flags_mut().set(ContextFlags::IN_PARENS, was_in_parens);

                let identifier = self.parse_interpolated_identifier()?;

                // todo: superfluous clone?
                if let Some(operation) = self.try_supports_operation(&identifier, name_start)? {
                    self.expect_char(')')?;
                    self.set_consume_newlines(was_consuming_newlines);
                    return Ok(operation);
                }

                // If parsing an expression fails, try to parse an
                // `InterpolatedAnyValue` instead. But if that value runs into a
                // top-level colon, then this is probably intended to be a declaration
                // after all, so we rethrow the declaration-parsing error.
                let mut contents = InterpolationBuilder::new();
                contents.add_interpolation(identifier);
                contents.add_interpolation(
                    self.parse_interpolated_declaration_value(true, true, false)?,
                );

                if self.toks_mut().next_char_is(':') {
                    return Err(e);
                }

                self.expect_char(')')?;
                self.set_consume_newlines(was_consuming_newlines);

                return Ok(AstSupportsCondition::Anything {
                    contents: contents.finish(self.arena()),
                });
            }
        };

        let declaration = self.supports_declaration_value(name, start)?;
        self.expect_char(')')?;
        self.set_consume_newlines(was_consuming_newlines);

        Ok(declaration)
    }

    fn parse_supports_condition(&mut self) -> SassResult<AstSupportsCondition<'a>> {
        if self.scan_identifier("not", false)? {
            self.whitespace()?;
            return Ok(AstSupportsCondition::Negation(self.arena().alloc(
                self.supports_condition_in_parens()?,
            )));
        }

        let mut condition = self.supports_condition_in_parens()?;
        self.whitespace()?;

        let mut operator: Option<String> = None;

        while self.looking_at_identifier() {
            if let Some(operator) = &operator {
                self.expect_identifier(operator, false)?;
            } else if self.scan_identifier("or", false)? {
                operator = Some("or".to_owned());
            } else {
                self.expect_identifier("and", false)?;
                operator = Some("and".to_owned());
            }

            self.whitespace()?;
            let right = self.supports_condition_in_parens()?;
            condition = AstSupportsCondition::Operation {
                left: self.arena().alloc(condition),
                operator: operator.clone(),
                right: self.arena().alloc(right),
            };
            self.whitespace()?;
        }

        Ok(condition)
    }

    fn parse_supports_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        let condition = self.parse_supports_condition()?;
        self.whitespace()?;
        let at_rule_span = self.toks_mut().span_from(start);
        let children = self.with_children(Self::parse_statement)?;

        Ok(AstStmt::Supports(self.arena().alloc(AstSupportsRule {
            condition,
            body: self.alloc_stmts(children.node),
            span: children.span,
            at_rule_span,
        })))
    }

    fn parse_warn_rule(&mut self) -> SassResult<AstStmt<'a>> {
        // In indented syntax, allow newline before value
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        let value = self.parse_expression(None, None, None)?;
        self.expect_statement_separator(Some("@warn rule"))?;
        Ok(AstStmt::Warn(AstWarn {
            value: value.node,
            span: value.span,
        }))
    }

    fn parse_while_rule(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<AstStmt<'a>> {
        let was_in_control_directive = self.flags().in_control_flow();
        self.flags_mut().set(ContextFlags::IN_CONTROL_FLOW, true);

        // In indented syntax, allow newline before condition
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);
        let condition = self.parse_expression(None, None, None)?.node;

        let body = self.with_children(child)?.node;

        self.flags_mut()
            .set(ContextFlags::IN_CONTROL_FLOW, was_in_control_directive);

        Ok(AstStmt::While(self.arena().alloc(AstWhile { condition, body: self.alloc_stmts(body) })))
    }
    fn parse_forward_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        self.set_consume_newlines(true);
        self.whitespace()?;
        let url = PathBuf::from(self.parse_url_string()?);
        self.set_consume_newlines(false);
        self.whitespace()?;

        let prefix = if self.scan_identifier("as", false)? {
            self.set_consume_newlines(true);
            self.whitespace()?;
            let prefix = self.parse_identifier(true, false)?;
            self.expect_char('*')?;
            self.set_consume_newlines(false);
            self.whitespace()?;
            Some(prefix)
        } else {
            None
        };

        let mut shown_mixins_and_functions: Option<FxHashSet<Identifier>> = None;
        let mut shown_variables: Option<FxHashSet<Identifier>> = None;
        let mut hidden_mixins_and_functions: Option<FxHashSet<Identifier>> = None;
        let mut hidden_variables: Option<FxHashSet<Identifier>> = None;

        if self.scan_identifier("show", false)? {
            self.set_consume_newlines(true);
            let members = self.parse_member_list()?;
            self.set_consume_newlines(false);
            shown_mixins_and_functions = Some(members.0);
            shown_variables = Some(members.1);
        } else if self.scan_identifier("hide", false)? {
            self.set_consume_newlines(true);
            let members = self.parse_member_list()?;
            self.set_consume_newlines(false);
            hidden_mixins_and_functions = Some(members.0);
            hidden_variables = Some(members.1);
        }

        let config = self.parse_configuration(true)?;
        let config: &'a [ConfiguredVariable<'a>] =
            self.arena().alloc_slice_fill_iter(config.unwrap_or_default());

        self.expect_statement_separator(Some("@forward rule"))?;
        let span = self.toks_mut().span_from(start);

        if !self.flags().is_use_allowed() {
            return Err((
                "@forward rules must be written before any other rules.",
                span,
            )
                .into());
        }

        Ok(AstStmt::Forward(
            if let (Some(shown_mixins_and_functions), Some(shown_variables)) =
                (shown_mixins_and_functions, shown_variables)
            {
                self.arena().alloc(AstForwardRule::show(
                    url,
                    shown_mixins_and_functions,
                    shown_variables,
                    prefix,
                    config,
                    span,
                ))
            } else if let (Some(hidden_mixins_and_functions), Some(hidden_variables)) =
                (hidden_mixins_and_functions, hidden_variables)
            {
                self.arena().alloc(AstForwardRule::hide(
                    url,
                    hidden_mixins_and_functions,
                    hidden_variables,
                    prefix,
                    config,
                    span,
                ))
            } else {
                self.arena().alloc(AstForwardRule::new(url, prefix, config, span))
            },
        ))
    }

    fn parse_member_list(&mut self) -> SassResult<(FxHashSet<Identifier>, FxHashSet<Identifier>)> {
        let mut identifiers = FxHashSet::default();
        let mut variables = FxHashSet::default();

        loop {
            self.set_consume_newlines(true);
            self.whitespace()?;

            // todo: withErrorMessage("Expected variable, mixin, or function name"
            if self.toks_mut().next_char_is('$') {
                variables.insert(Identifier::from(self.parse_variable_name()?));
            } else {
                identifiers.insert(Identifier::from(self.parse_identifier(true, false)?));
            }

            self.set_consume_newlines(false);
            self.whitespace()?;

            if !self.scan_char(',') {
                break;
            }
        }

        Ok((identifiers, variables))
    }

    fn parse_url_string(&mut self) -> SassResult<String> {
        // todo: real uri parsing
        self.parse_string()
    }

    fn use_namespace(
        &mut self,
        url: &Path,
        _start: usize,
        url_span: Span,
    ) -> SassResult<Option<String>> {
        if self.scan_identifier("as", false)? {
            self.set_consume_newlines(true);
            self.whitespace()?;
            let result = if self.scan_char('*') {
                None
            } else {
                Some(self.parse_identifier(false, false)?)
            };
            self.set_consume_newlines(false);
            return Ok(result);
        }

        let base_name = url
            .file_name()
            .map_or_else(OsString::new, ToOwned::to_owned);
        let base_name = base_name.to_string_lossy();
        let dot = base_name.find('.');

        let start = if base_name.starts_with('_') { 1 } else { 0 };
        let end = dot.unwrap_or(base_name.len());
        let namespace = if url.to_string_lossy().starts_with("sass:") {
            return Ok(Some(url.to_string_lossy().into_owned()));
        } else {
            &base_name[start..end]
        };

        let mut toks = Lexer::new_from_string(namespace, url_span);

        // if namespace is empty, avoid attempting to parse an identifier from
        // an empty string, as there will be no span to emit
        let identifier = if namespace.is_empty() {
            Err(("", self.empty_span()).into())
        } else {
            mem::swap(self.toks_mut(), &mut toks);
            let ident = self.parse_identifier(false, false);
            mem::swap(self.toks_mut(), &mut toks);
            ident
        };

        match (identifier, toks.peek().is_none()) {
            (Ok(i), true) => Ok(Some(i)),
            _ => {
                Err((
                    format!(
                        "The default namespace \"{namespace}\" is not a valid Sass identifier.\n\nRecommendation: add an \"as\" clause to define an explicit namespace.", 
                        namespace = namespace
                    ),
                    self.toks_mut().span_from(start)
                ).into())
            }
        }
    }

    fn parse_configuration(
        &mut self,
        // default=false
        allow_guarded: bool,
    ) -> SassResult<Option<Vec<ConfiguredVariable<'a>>>> {
        if !self.scan_identifier("with", false)? {
            return Ok(None);
        }

        let mut variable_names = FxHashSet::default();
        let mut configuration = Vec::new();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.expect_char('(')?;

        loop {
            self.whitespace()?;
            let var_start = self.toks().cursor();
            let name = Identifier::from(self.parse_variable_name()?);
            let name_span = self.toks_mut().span_from(var_start);

            if !name.is_public() {
                self.parse_time_warnings_mut().push((
                    Deprecation::WithPrivate,
                    name_span,
                    "Configuring private variables is deprecated.\nThis will be an error in \
                     Dart Sass 2.0.0."
                        .to_string(),
                ));
            }

            self.whitespace()?;
            self.expect_char(':')?;
            self.whitespace()?;
            let expr = self.parse_expression_until_comma(false)?;

            let mut is_guarded = false;
            let flag_start = self.toks().cursor();
            if allow_guarded && self.scan_char('!') {
                let flag = self.parse_identifier(false, false)?;
                if flag == "default" {
                    is_guarded = true;
                    self.whitespace()?;
                } else {
                    self.set_consume_newlines(false);
                    return Err(
                        ("Invalid flag name.", self.toks_mut().span_from(flag_start)).into(),
                    );
                }
            }

            let span = self.toks_mut().span_from(var_start);
            if variable_names.contains(&name) {
                self.set_consume_newlines(false);
                return Err(("The same variable may only be configured once.", span).into());
            }

            variable_names.insert(name);
            configuration.push(ConfiguredVariable {
                name: Spanned {
                    node: name,
                    span: name_span,
                },
                expr,
                is_guarded,
            });

            if !self.scan_char(',') {
                break;
            }
            self.whitespace()?;
            if !self.looking_at_expression() {
                break;
            }
        }

        self.expect_char(')')?;
        self.set_consume_newlines(false);

        Ok(Some(configuration))
    }

    fn parse_use_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        self.set_consume_newlines(true);
        self.whitespace()?;
        let url_start = self.toks().cursor();
        let url = self.parse_url_string()?;
        let url_span = self.toks().span_from(url_start);
        self.set_consume_newlines(false);
        self.whitespace()?;

        let path = PathBuf::from(url);

        let namespace = self.use_namespace(path.as_ref(), start, url_span)?;
        self.set_consume_newlines(false);
        self.whitespace()?;
        let configuration = self.parse_configuration(false)?;
        self.set_consume_newlines(false);
        self.whitespace()?;

        self.expect_statement_separator(Some("@use rule"))?;

        let span = self.toks_mut().span_from(start);

        if !self.flags().is_use_allowed() {
            return Err((
                "@use rules must be written before any other rules.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        self.expect_statement_separator(Some("@use rule"))?;

        let configuration = self
            .arena()
            .alloc_slice_fill_iter(configuration.unwrap_or_default());

        Ok(AstStmt::Use(self.arena().alloc(AstUseRule {
            url: path,
            namespace,
            configuration,
            span,
        })))
    }

    fn parse_at_rule(
        &mut self,
        child: fn(&mut Self) -> SassResult<AstStmt<'a>>,
    ) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();

        self.expect_char('@')?;
        let name = self.parse_interpolated_identifier()?;
        self.whitespace()?;

        // We want to set [_isUseAllowed] to `false` *unless* we're parsing
        // `@charset`, `@forward`, or `@use`. To avoid double-comparing the rule
        // name, we always set it to `false` and then set it back to its previous
        // value if we're parsing an allowed rule.
        let was_use_allowed = self.flags().is_use_allowed();
        self.flags_mut().set(ContextFlags::IS_USE_ALLOWED, false);

        match name.as_plain() {
            Some("at-root") => self.parse_at_root_rule(start),
            Some("content") => self.parse_content_rule(start),
            Some("debug") => self.parse_debug_rule(),
            Some("each") => self.parse_each_rule(child),
            Some("else") | Some("return") => self.parse_disallowed_at_rule(start),
            Some("error") => self.parse_error_rule(),
            Some("extend") => self.parse_extend_rule(start),
            Some("for") => self.parse_for_rule(child),
            Some("forward") => {
                self.flags_mut()
                    .set(ContextFlags::IS_USE_ALLOWED, was_use_allowed);
                // if (!root) {
                //     _disallowedAtRule();
                // }
                self.parse_forward_rule(start)
            }
            Some("function") => self.parse_function_rule(start),
            Some("if") => self.parse_if_rule(child),
            Some("import") => self.parse_import_rule(start),
            Some("include") => self.parse_include_rule(),
            Some("media") => self.parse_media_rule(start),
            Some("mixin") => self.parse_mixin_rule(start),
            // todo: support -moz-document
            // Some("-moz-document") => self.parse_moz_document_rule(name),
            Some("supports") => self.parse_supports_rule(start),
            Some("use") => {
                self.flags_mut()
                    .set(ContextFlags::IS_USE_ALLOWED, was_use_allowed);
                // if (!root) {
                //     _disallowedAtRule();
                // }
                self.parse_use_rule(start)
            }
            Some("warn") => self.parse_warn_rule(),
            Some("while") => self.parse_while_rule(child),
            Some(..) | None => self.unknown_at_rule(name, start),
        }
    }

    fn parse_statement(&mut self) -> SassResult<AstStmt<'a>> {
        match self.toks().peek() {
            Some(Token { kind: '@', .. }) => self.parse_at_rule(Self::parse_statement),
            Some(Token { kind: '+', .. }) => {
                if !self.is_indented() {
                    return self.parse_style_rule(None, None);
                }

                let start = self.toks().cursor();

                self.toks_mut().next();

                if !self.looking_at_identifier() {
                    self.toks_mut().set_cursor(start);
                    return self.parse_style_rule(None, None);
                }

                self.flags_mut().set(ContextFlags::IS_USE_ALLOWED, false);
                self.parse_include_rule()
            }
            Some(Token { kind: '=', .. }) => {
                if !self.is_indented() {
                    return self.parse_style_rule(None, None);
                }

                self.flags_mut().set(ContextFlags::IS_USE_ALLOWED, false);
                let start = self.toks().cursor();
                self.toks_mut().next();
                self.whitespace()?;
                self.parse_mixin_rule(start)
            }
            Some(Token { kind: '}', .. }) => {
                Err(("unmatched \"}\".", self.toks().current_span()).into())
            }
            _ => {
                if self.flags().in_style_rule()
                    || self.flags().in_unknown_at_rule()
                    || self.flags().in_mixin()
                    || self.flags().in_content_block()
                {
                    self.parse_declaration_or_style_rule()
                } else {
                    self.parse_variable_declaration_or_style_rule()
                }
            }
        }
    }

    fn parse_declaration_or_style_rule(&mut self) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();

        // The indented syntax allows a single backslash to distinguish a style rule
        // from old-style property syntax. We don't support old property syntax, but
        // we do support the backslash because it's easy to do.
        if self.is_indented() && self.scan_char('\\') {
            return self.parse_style_rule(None, None);
        };

        match self.parse_declaration_or_buffer()? {
            DeclarationOrBuffer::Stmt(s) => Ok(s),
            DeclarationOrBuffer::Buffer(existing_buffer) => {
                self.parse_style_rule(Some(existing_buffer), Some(start))
            }
        }
    }

    fn parse_property_or_variable_declaration(
        &mut self,
        // default=true
        parse_custom_properties: bool,
    ) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();

        let name = if matches!(
            self.toks().peek(),
            Some(Token {
                kind: ':' | '*' | '.',
                ..
            })
        ) || (matches!(self.toks().peek(), Some(Token { kind: '#', .. }))
            && !matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })))
        {
            // Allow the "*prop: val", ":prop: val", "#prop: val", and ".prop: val"
            // hacks.
            let mut name_buffer = InterpolationBuilder::new();
            name_buffer.add_char(self.toks_mut().next().unwrap().kind);
            self.append_raw_text(name_buffer.trailing_string_mut(), Self::whitespace);
            name_buffer.add_interpolation(self.parse_interpolated_identifier()?);
            name_buffer
        } else if !self.is_plain_css() {
            match self.parse_variable_declaration_or_interpolation()? {
                VariableDeclOrInterpolation::Interpolation(interpolation) => interpolation,
                VariableDeclOrInterpolation::VariableDecl(decl) => {
                    return Ok(AstStmt::VariableDecl(self.arena().alloc(decl)))
                }
            }
        } else {
            self.parse_interpolated_identifier()?
        };

        self.whitespace()?;
        self.expect_char(':')?;

        if parse_custom_properties && name.initial_plain().starts_with("--") {
            let interpolation = self.parse_interpolated_declaration_value(false, true, true)?;
            let value_span = self.toks_mut().span_from(start);
            let value = AstExpr::String(
                StringExpr(interpolation.finish(self.arena()), QuoteKind::None),
                value_span,
            )
            .span(value_span);
            self.expect_statement_separator(Some("custom property"))?;
            return Ok(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name.finish(self.arena()),
                value: Some(value),
                body: &[],
                span: value_span,
            })));
        }

        self.whitespace()?;

        if self.looking_at_children()? {
            if self.is_plain_css() {
                return Err((
                    "Nested declarations aren't allowed in plain CSS.",
                    self.toks().current_span(),
                )
                    .into());
            }

            if name.initial_plain().starts_with("--") {
                return Err((
                    "Declarations whose names begin with \"--\" may not be nested",
                    self.toks_mut().span_from(start),
                )
                    .into());
            }

            let children = self.with_children(Self::parse_declaration_child)?.node;

            return Ok(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name.finish(self.arena()),
                value: None,
                body: self.alloc_stmts(children),
                span: self.toks_mut().span_from(start),

            })));
        }

        let value = self.parse_expression(None, None, None)?;
        if self.looking_at_children()? {
            if self.is_plain_css() {
                return Err((
                    "Nested declarations aren't allowed in plain CSS.",
                    self.toks().current_span(),
                )
                    .into());
            }

            if name.initial_plain().starts_with("--") && !matches!(value.node, AstExpr::String(..))
            {
                return Err((
                    "Declarations whose names begin with \"--\" may not be nested",
                    self.toks_mut().span_from(start),
                )
                    .into());
            }

            let children = self.with_children(Self::parse_declaration_child)?.node;

            Ok(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name.finish(self.arena()),
                value: Some(value),
                body: self.alloc_stmts(children),
                span: self.toks_mut().span_from(start),

            })))
        } else {
            self.expect_statement_separator(None)?;
            Ok(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name.finish(self.arena()),
                value: Some(value),
                body: &[],
                span: self.toks_mut().span_from(start),

            })))
        }
    }

    fn parse_single_interpolation(&mut self) -> SassResult<InterpolationBuilder<'a>> {
        self.expect_char('#')?;
        self.expect_char('{')?;
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        let contents = self.parse_expression(None, None, None)?;
        self.set_consume_newlines(was_consuming_newlines);
        self.expect_char('}')?;

        if self.is_plain_css() {
            return Err(("Interpolation isn't allowed in plain CSS.", contents.span).into());
        }

        let mut interpolation = InterpolationBuilder::new();
        interpolation
            .contents
            .push(InterpolationPartBuilder::Expr(contents));

        Ok(interpolation)
    }

    fn parse_interpolated_identifier_body(&mut self, buffer: &mut InterpolationBuilder<'a>) -> SassResult<()> {
        while let Some(next) = self.toks().peek() {
            match next.kind {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '\u{80}'..=std::char::MAX => {
                    buffer.add_char(next.kind);
                    self.toks_mut().next();
                }
                '\\' => {
                    buffer.add_string(self.parse_escape(false)?);
                }
                '#' if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) => {
                    buffer.add_interpolation(self.parse_single_interpolation()?);
                }
                _ => break,
            }
        }

        Ok(())
    }

    fn parse_interpolated_identifier(&mut self) -> SassResult<InterpolationBuilder<'a>> {
        let mut buffer = InterpolationBuilder::new();

        if self.scan_char('-') {
            buffer.add_char('-');

            if self.scan_char('-') {
                buffer.add_char('-');
                self.parse_interpolated_identifier_body(&mut buffer)?;
                return Ok(buffer);
            }
        }

        match self.toks().peek() {
            Some(tok) if is_name_start(tok.kind) => {
                buffer.add_char(tok.kind);
                self.toks_mut().next();
            }
            Some(Token { kind: '\\', .. }) => {
                buffer.add_string(self.parse_escape(true)?);
            }
            Some(Token { kind: '#', .. })
                if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) =>
            {
                buffer.add_interpolation(self.parse_single_interpolation()?);
            }
            Some(..) | None => {
                return Err(("Expected identifier.", self.toks().current_span()).into())
            }
        }

        self.parse_interpolated_identifier_body(&mut buffer)?;

        Ok(buffer)
    }

    fn looking_at_interpolated_identifier(&mut self) -> bool {
        let first = match self.toks().peek() {
            Some(Token { kind: '\\', .. }) => return true,
            Some(Token { kind: '#', .. }) => {
                return matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. }))
            }
            Some(Token { kind, .. }) if is_name_start(kind) => return true,
            Some(tok) => tok,
            None => return false,
        };

        if first.kind != '-' {
            return false;
        }

        match self.toks().peek_n(1) {
            Some(Token { kind: '#', .. }) => {
                matches!(self.toks().peek_n(2), Some(Token { kind: '{', .. }))
            }
            Some(Token {
                kind: '\\' | '-', ..
            }) => true,
            Some(Token { kind, .. }) => is_name_start(kind),
            None => false,
        }
    }

    fn parse_loud_comment(&mut self) -> SassResult<AstLoudComment<'a>> {
        let start = self.toks().cursor();
        self.expect_char('/')?;
        self.expect_char('*')?;

        let mut buffer = InterpolationBuilder::new_plain("/*".to_owned());

        while let Some(tok) = self.toks().peek() {
            match tok.kind {
                '#' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) {
                        buffer.add_interpolation(self.parse_single_interpolation()?);
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                    }
                }
                '*' => {
                    self.toks_mut().next();
                    buffer.add_char(tok.kind);

                    if self.scan_char('/') {
                        buffer.add_char('/');

                        return Ok(AstLoudComment {
                            text: buffer.finish(self.arena()),
                            span: self.toks_mut().span_from(start),
                        });
                    }
                }
                '\r' => {
                    self.toks_mut().next();
                    // todo: does \r even exist at this point? (removed by lexer)
                    if !self.toks_mut().next_char_is('\n') {
                        buffer.add_char('\n');
                    }
                }
                _ => {
                    buffer.add_char(tok.kind);
                    self.toks_mut().next();
                }
            }
        }

        Err(("expected more input.", self.toks().current_span()).into())
    }

    fn parse_interpolated_declaration_value(
        &mut self,
        // default=false
        allow_semicolon: bool,
        // default=false
        allow_empty: bool,
        // default=true
        allow_colon: bool,
    ) -> SassResult<InterpolationBuilder<'a>> {
        self.parse_interpolated_declaration_value_inner(allow_semicolon, allow_empty, allow_colon, true)
    }

    /// Like `parse_interpolated_declaration_value` but with `silent_comments=false`,
    /// preserving `//` as literal text (for custom property values).
    fn parse_interpolated_declaration_value_no_strip_comments(
        &mut self,
        allow_semicolon: bool,
        allow_empty: bool,
        allow_colon: bool,
    ) -> SassResult<InterpolationBuilder<'a>> {
        self.parse_interpolated_declaration_value_inner(allow_semicolon, allow_empty, allow_colon, false)
    }

    fn parse_interpolated_declaration_value_inner(
        &mut self,
        allow_semicolon: bool,
        allow_empty: bool,
        allow_colon: bool,
        // When true, `//` is treated as a silent comment and stripped.
        // When false (custom properties), `//` is preserved as literal text.
        silent_comments: bool,
    ) -> SassResult<InterpolationBuilder<'a>> {
        let mut buffer = InterpolationBuilder::new();

        let mut brackets = Vec::new();
        let mut wrote_newline = false;
        let mut ident_buf = String::new();

        while let Some(tok) = self.toks().peek() {
            match tok.kind {
                '\\' => {
                    buffer.add_string(self.parse_escape(true)?);
                    wrote_newline = false;
                }
                '"' | '\'' => {
                    let original_quote = tok.kind;
                    buffer.add_interpolation(InterpolationBuilder::from_interpolation(
                        self.parse_interpolated_string()?.node.as_interpolation(
                            false,
                            Some(original_quote),
                            self.arena(),
                        ),
                    ));
                    wrote_newline = false;
                }
                '/' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '*', .. })) {
                        self.fallible_append_raw_text(
                            buffer.trailing_string_mut(),
                            Self::skip_loud_comment,
                        )?;
                    } else if silent_comments
                        && matches!(self.toks().peek_n(1), Some(Token { kind: '/', .. }))
                    {
                        self.skip_silent_comment()?;
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                    }

                    wrote_newline = false;
                }
                '#' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) {
                        // Add a full interpolated identifier to handle cases like
                        // "#{...}--1", since "--1" isn't a valid identifier on its own.
                        buffer.add_interpolation(self.parse_interpolated_identifier()?);
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                    }

                    wrote_newline = false;
                }
                ' ' | '\t' => {
                    if wrote_newline
                        || !matches!(
                            self.toks().peek_n(1),
                            Some(Token {
                                kind: ' ' | '\r' | '\t' | '\n',
                                ..
                            })
                        )
                    {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                    } else {
                        self.toks_mut().next();
                    }
                }
                '\n' | '\r' => {
                    if self.is_indented()
                        && brackets.is_empty()
                        && !self.is_consuming_newlines()
                    {
                        break;
                    }
                    buffer.add_char('\n');
                    self.toks_mut().next();
                    wrote_newline = true;
                }
                '(' | '{' | '[' => {
                    self.toks_mut().next();
                    buffer.add_char(tok.kind);
                    brackets.push(opposite_bracket(tok.kind));
                    wrote_newline = false;
                }
                ')' | '}' | ']' => {
                    if brackets.is_empty() {
                        break;
                    }
                    buffer.add_char(tok.kind);
                    self.expect_char(brackets.pop().unwrap())?;
                    wrote_newline = false;
                }
                ';' => {
                    if !allow_semicolon && brackets.is_empty() {
                        break;
                    }
                    buffer.add_char(tok.kind);
                    self.toks_mut().next();
                    wrote_newline = false;
                }
                ':' => {
                    if !allow_colon && brackets.is_empty() {
                        break;
                    }
                    buffer.add_char(tok.kind);
                    self.toks_mut().next();
                    wrote_newline = false;
                }
                'u' | 'U' => {
                    let before_url = self.toks().cursor();

                    if !self.scan_identifier("url", false)? {
                        buffer.add_char(tok.kind);
                        self.toks_mut().next();
                        wrote_newline = false;
                        continue;
                    }

                    match self.try_url_contents(None)? {
                        Some(contents) => {
                            buffer.add_interpolation(contents);
                        }
                        None => {
                            self.toks_mut().set_cursor(before_url);
                            buffer.add_char(tok.kind);
                            self.toks_mut().next();
                        }
                    }

                    wrote_newline = false;
                }
                _ => {
                    if self.looking_at_identifier() {
                        ident_buf.clear();
                        self.parse_identifier_into(&mut ident_buf, false, false)?;
                        buffer.add_str(&ident_buf);
                    } else {
                        buffer.add_char(tok.kind);
                        self.toks_mut().next();
                    }
                    wrote_newline = false;
                }
            }
        }

        if let Some(&last) = brackets.last() {
            self.expect_char(last)?;
        }

        if !allow_empty && buffer.contents.is_empty() {
            return Err(("Expected token.", self.toks().current_span()).into());
        }

        Ok(buffer)
    }

    fn parse_expression_until_comma(
        &mut self,
        // default=false
        single_equals: bool,
    ) -> SassResult<Spanned<AstExpr<'a>>> {
        ValueParser::parse_expression(
            self,
            Some(&|parser| {
                Ok(matches!(
                    parser.toks().peek(),
                    Some(Token { kind: ',', .. })
                ))
            }),
            false,
            single_equals,
        )
    }

    fn parse_argument_invocation(
        &mut self,
        for_mixin: bool,
        allow_empty_second_arg: bool,
    ) -> SassResult<ArgumentInvocation<'a>> {
        let start = self.toks().cursor();

        self.expect_char('(')?;
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        let mut positional = Vec::new();
        let mut named = SmallOrderedMap::default();

        let mut rest: Option<AstExpr<'a>> = None;
        let mut keyword_rest: Option<AstExpr<'a>> = None;
        let mut emitted_rest_deprecation = false;

        while self.looking_at_expression() {
            let expression = self.parse_expression_until_comma(!for_mixin)?;
            self.whitespace()?;

            if expression.node.is_variable() && self.scan_char(':') {
                let name = match expression.node {
                    AstExpr::Variable { name, .. } => name,
                    _ => unreachable!(),
                };

                self.whitespace()?;
                if named.contains_key(&name.node) {
                    return Err(("Duplicate argument.", name.span).into());
                }

                let value = self.parse_expression_until_comma(!for_mixin)?;

                if rest.is_some() && !emitted_rest_deprecation {
                    emitted_rest_deprecation = true;
                    self.parse_time_warnings_mut().push((
                        Deprecation::MisplacedRest,
                        name.span.merge(value.span),
                        "Named arguments must come before rest arguments.\nThis will be an \
                         error in Dart Sass 2.0.0."
                            .to_string(),
                    ));
                }

                named.insert(name.node, value.node);
            } else if self.scan_char('.') {
                self.expect_char('.')?;
                self.expect_char('.')?;

                if rest.is_none() {
                    rest = Some(expression.node);
                } else {
                    keyword_rest = Some(expression.node);
                    self.whitespace()?;
                    self.scan_char(',');
                    self.whitespace()?;
                    break;
                }
            } else if !named.is_empty() {
                return Err((
                    "Positional arguments must come before keyword arguments.",
                    expression.span,
                )
                    .into());
            } else {
                if rest.is_some() && !emitted_rest_deprecation {
                    emitted_rest_deprecation = true;
                    self.parse_time_warnings_mut().push((
                        Deprecation::MisplacedRest,
                        expression.span,
                        "Positional arguments must come before rest arguments.\nThis will be \
                         an error in Dart Sass 2.0.0."
                            .to_string(),
                    ));
                }

                positional.push(expression.node);
            }

            self.whitespace()?;
            if !self.scan_char(',') {
                break;
            }
            self.whitespace()?;

            if allow_empty_second_arg
                && positional.len() == 1
                && named.is_empty()
                && rest.is_none()
                && matches!(self.toks().peek(), Some(Token { kind: ')', .. }))
            {
                positional.push(AstExpr::String(
                    StringExpr(InterpolationBuilder::new().finish(self.arena()), QuoteKind::None),
                    self.toks().current_span(),
                ));
                break;
            }
        }

        self.expect_char(')')?;
        self.set_consume_newlines(was_consuming_newlines);

        Ok(ArgumentInvocation {
            positional: self.arena().alloc_slice_fill_iter(positional),
            named: self.arena().alloc_slice_fill_iter(named),
            rest,
            keyword_rest,
            span: self.toks_mut().span_from(start),
        })
    }

    fn parse_expression(
        &mut self,
        parse_until: Option<Predicate<'_, Self>>,
        inside_bracketed_list: Option<bool>,
        single_equals: Option<bool>,
    ) -> SassResult<Spanned<AstExpr<'a>>> {
        ValueParser::parse_expression(
            self,
            parse_until,
            inside_bracketed_list.unwrap_or(false),
            single_equals.unwrap_or(false),
        )
    }

    fn parse_declaration_or_buffer(&mut self) -> SassResult<DeclarationOrBuffer<'a>> {
        let start = self.toks().cursor();
        let mut name_buffer = InterpolationBuilder::new();

        // Allow the "*prop: val", ":prop: val", "#prop: val", and ".prop: val"
        // hacks.
        let first = self.toks().peek();
        let mut starts_with_punctuation = false;

        if matches!(
            first,
            Some(Token {
                kind: ':' | '*' | '.',
                ..
            })
        ) || (matches!(first, Some(Token { kind: '#', .. }))
            && !matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })))
        {
            starts_with_punctuation = true;
            name_buffer.add_char(self.toks_mut().next().unwrap().kind);
            self.append_raw_text(name_buffer.trailing_string_mut(), Self::whitespace);
        }

        if !self.looking_at_interpolated_identifier() {
            return Ok(DeclarationOrBuffer::Buffer(name_buffer));
        }

        let variable_or_interpolation = if starts_with_punctuation {
            VariableDeclOrInterpolation::Interpolation(self.parse_interpolated_identifier()?)
        } else {
            self.parse_variable_declaration_or_interpolation()?
        };

        match variable_or_interpolation {
            VariableDeclOrInterpolation::Interpolation(int) => name_buffer.add_interpolation(int),
            VariableDeclOrInterpolation::VariableDecl(v) => {
                return Ok(DeclarationOrBuffer::Stmt(AstStmt::VariableDecl(self.arena().alloc(v))))
            }
        }

        self.flags_mut().set(ContextFlags::IS_USE_ALLOWED, false);

        if self.next_matches("/*") {
            self.fallible_append_raw_text(name_buffer.trailing_string_mut(), Self::skip_loud_comment)?;
        }

        let mut mid_buffer = String::new();
        self.append_raw_text(&mut mid_buffer, Self::whitespace);

        if !self.scan_char(':') {
            if !mid_buffer.is_empty() {
                name_buffer.add_char(' ');
            }
            return Ok(DeclarationOrBuffer::Buffer(name_buffer));
        }
        mid_buffer.push(':');

        // Parse custom properties and CSS function body declarations as raw CSS.
        let is_custom_property = name_buffer.initial_plain().starts_with("--");
        let is_css_fn_decl = self.flags().in_css_function_body() && name_buffer.as_plain().is_some();
        if is_custom_property || is_css_fn_decl {
            // For CSS function body declarations, consume whitespace so the
            // serializer's ": " doesn't create a double space.
            if is_css_fn_decl && !is_custom_property {
                self.whitespace()?;
            }
            let value_start = self.toks().cursor();
            let value = self.parse_interpolated_declaration_value_no_strip_comments(false, true, true)?;
            let value_span = self.toks_mut().span_from(value_start);
            let separator_name = if is_css_fn_decl && !is_custom_property {
                Some("@function result")
            } else {
                Some("custom property")
            };
            self.expect_statement_separator(separator_name)?;
            return Ok(DeclarationOrBuffer::Stmt(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name_buffer.finish(self.arena()),
                value: Some(
                    AstExpr::String(
                        StringExpr(value.finish(self.arena()), QuoteKind::None),
                        value_span,
                    )
                    .span(value_span),
                ),
                span: self.toks_mut().span_from(start),
                body: &[],
            }))));
        }

        if self.scan_char(':') {
            name_buffer.add_string(mid_buffer);
            name_buffer.add_char(':');
            return Ok(DeclarationOrBuffer::Buffer(name_buffer));
        } else if self.is_indented() && self.looking_at_interpolated_identifier() {
            // In the indented syntax, `foo:bar` is always considered a selector
            // rather than a property.
            name_buffer.add_string(mid_buffer);
            return Ok(DeclarationOrBuffer::Buffer(name_buffer));
        }

        // Whitespace consumption is intentionally not materialized into a
        // `String` here (unlike the `raw_text`-based sites above) since it's
        // only ever needed as either an emptiness check (cursor delta, below)
        // or appended into `mid_buffer` -- both of which `raw_chars` serves
        // directly without an intermediate allocation.
        let post_colon_whitespace_start = self.toks().cursor();
        let _ = self.whitespace();
        if self.looking_at_children()? {
            if self.is_plain_css() {
                return Err((
                    "Nested declarations aren't allowed in plain CSS.",
                    self.toks().current_span(),
                )
                    .into());
            }
            let body = self.with_children(Self::parse_declaration_child)?.node;
            return Ok(DeclarationOrBuffer::Stmt(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name_buffer.finish(self.arena()),
                value: None,
                span: self.toks_mut().span_from(start),
                body: self.alloc_stmts(body),

            }))));
        }

        let could_be_selector = self.toks().cursor() == post_colon_whitespace_start
            && self.looking_at_interpolated_identifier();
        mid_buffer.extend(self.toks().raw_chars(post_colon_whitespace_start));

        let before_decl = self.toks().cursor();

        let mut calculate_value = || {
            let value = self.parse_expression(None, None, None)?;

            if self.looking_at_children()? {
                if could_be_selector {
                    self.expect_statement_separator(None)?;
                }
            } else if !self.at_end_of_statement() {
                self.expect_statement_separator(None)?;
            }

            Ok(value)
        };

        let value = match calculate_value() {
            Ok(v) => v,
            Err(e) => {
                if !could_be_selector {
                    return Err(e);
                }

                self.toks_mut().set_cursor(before_decl);
                let additional = self.almost_any_value(false)?;
                if !self.is_indented() && self.toks_mut().next_char_is(';') {
                    return Err(e);
                }

                name_buffer.add_string(mid_buffer);
                name_buffer.add_interpolation(additional);
                return Ok(DeclarationOrBuffer::Buffer(name_buffer));
            }
        };

        if self.looking_at_children()? {
            if self.is_plain_css() {
                return Err((
                    "Nested declarations aren't allowed in plain CSS.",
                    self.toks().current_span(),
                )
                    .into());
            }
            let body = self.with_children(Self::parse_declaration_child)?.node;
            Ok(DeclarationOrBuffer::Stmt(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name_buffer.finish(self.arena()),
                value: Some(value),
                span: self.toks_mut().span_from(start),
                body: self.alloc_stmts(body),

            }))))
        } else {
            self.expect_statement_separator(None)?;
            Ok(DeclarationOrBuffer::Stmt(AstStmt::Style(self.arena().alloc(AstStyle {
                name: name_buffer.finish(self.arena()),
                value: Some(value),
                span: self.toks_mut().span_from(start),
                body: &[],

            }))))
        }
    }

    fn parse_declaration_child(&mut self) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();

        if self.toks_mut().next_char_is('@') {
            self.parse_declaration_at_rule(start)
        } else {
            self.parse_property_or_variable_declaration(false)
        }
    }

    fn parse_plain_at_rule_name(&mut self) -> SassResult<String> {
        self.expect_char('@')?;
        let name = self.parse_identifier(false, false)?;
        self.whitespace()?;
        Ok(name)
    }

    fn parse_declaration_at_rule(&mut self, start: usize) -> SassResult<AstStmt<'a>> {
        let name = self.parse_plain_at_rule_name()?;

        match name.as_str() {
            "content" => self.parse_content_rule(start),
            "debug" => self.parse_debug_rule(),
            "each" => self.parse_each_rule(Self::parse_declaration_child),
            "else" => self.parse_disallowed_at_rule(start),
            "error" => self.parse_error_rule(),
            "for" => self.parse_for_rule(Self::parse_declaration_child),
            "if" => self.parse_if_rule(Self::parse_declaration_child),
            "include" => self.parse_include_rule(),
            "warn" => self.parse_warn_rule(),
            "while" => self.parse_while_rule(Self::parse_declaration_child),
            _ => self.parse_disallowed_at_rule(start),
        }
    }

    fn parse_variable_declaration_or_style_rule(&mut self) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();

        if self.is_plain_css() {
            return self.parse_style_rule(None, None);
        }

        // The indented syntax allows a single backslash to distinguish a style rule
        // from old-style property syntax. We don't support old property syntax, but
        // we do support the backslash because it's easy to do.
        if self.is_indented() && self.scan_char('\\') {
            return self.parse_style_rule(None, None);
        };

        if !self.looking_at_identifier() {
            return self.parse_style_rule(None, None);
        }

        match self.parse_variable_declaration_or_interpolation()? {
            VariableDeclOrInterpolation::VariableDecl(var) => Ok(AstStmt::VariableDecl(self.arena().alloc(var))),
            VariableDeclOrInterpolation::Interpolation(int) => {
                self.parse_style_rule(Some(int), Some(start))
            }
        }
    }

    fn parse_style_rule(
        &mut self,
        existing_buffer: Option<InterpolationBuilder<'a>>,
        start: Option<usize>,
    ) -> SassResult<AstStmt<'a>> {
        let start = start.unwrap_or_else(|| self.toks().cursor());

        self.flags_mut().set(ContextFlags::IS_USE_ALLOWED, false);
        let mut interpolation = self.parse_style_rule_selector()?;

        if let Some(mut existing_buffer) = existing_buffer {
            existing_buffer.add_interpolation(interpolation);
            interpolation = existing_buffer;
        }

        if interpolation.contents.is_empty() {
            return Err(("expected \"}\".", self.toks().current_span()).into());
        }

        let was_in_style_rule = self.flags().in_style_rule();
        *self.flags_mut() |= ContextFlags::IN_STYLE_RULE;

        let selector_span = self.toks_mut().span_from(start);

        let children = self.with_children(Self::parse_statement)?;

        self.flags_mut()
            .set(ContextFlags::IN_STYLE_RULE, was_in_style_rule);

        let span = selector_span.merge(children.span);

        Ok(AstStmt::RuleSet(AstRuleSet {
            selector: interpolation.finish(self.arena()),
            body: self.alloc_stmts(children.node),
            selector_span,
            span,
        }))
    }

    fn parse_silent_comment(&mut self) -> SassResult<AstStmt<'a>> {
        let start = self.toks().cursor();
        debug_assert!(self.next_matches("//"));
        self.toks_mut().next();
        self.toks_mut().next();

        let mut buffer = String::new();

        while let Some(tok) = self.toks_mut().next() {
            if tok.kind == '\n' {
                self.whitespace_without_comments();
                if self.next_matches("//") {
                    self.toks_mut().next();
                    self.toks_mut().next();
                    buffer.clear();
                    continue;
                }
                break;
            }

            buffer.push(tok.kind);
        }

        if self.is_plain_css() {
            return Err((
                "Silent comments aren't allowed in plain CSS.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        self.whitespace_without_comments();

        Ok(AstStmt::SilentComment(AstSilentComment {
            text: buffer,
            span: self.toks_mut().span_from(start),
        }))
    }

    fn next_is_hex(&self) -> bool {
        match self.toks().peek() {
            Some(Token { kind, .. }) => kind.is_ascii_hexdigit(),
            None => false,
        }
    }

    fn assert_public(ident: &str, span: Span) -> SassResult<()> {
        if !ScssParser::is_private(ident) {
            return Ok(());
        }

        Err((
            "Private members can't be accessed from outside their modules.",
            span,
        )
            .into())
    }

    fn is_private(ident: &str) -> bool {
        ident.starts_with('-') || ident.starts_with('_')
    }

    fn parse_variable_declaration_without_namespace(
        &mut self,
        namespace: Option<Spanned<Identifier>>,
        start: Option<usize>,
    ) -> SassResult<AstVariableDecl<'a>> {
        let start = start.unwrap_or_else(|| self.toks().cursor());

        let name = self.parse_variable_name()?;

        if namespace.is_some() {
            Self::assert_public(&name, self.toks_mut().span_from(start))?;
        }

        if self.is_plain_css() {
            return Err((
                "Sass variables aren't allowed in plain CSS.",
                self.toks_mut().span_from(start),
            )
                .into());
        }

        // In indented syntax, allow newlines around ':'
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;
        self.expect_char(':')?;
        self.whitespace()?;
        self.set_consume_newlines(was_consuming_newlines);

        let value = self.parse_expression(None, None, None)?.node;

        let mut is_guarded = false;
        let mut is_global = false;

        loop {
            let flag_start = self.toks().cursor();
            if !self.scan_char('!') {
                break;
            }
            let flag = self.parse_identifier(false, false)?;

            match flag.as_str() {
                "default" => {
                    if is_guarded {
                        let span = self.toks_mut().span_from(flag_start);
                        self.parse_time_warnings_mut().push((
                            Deprecation::DuplicateVarFlags,
                            span,
                            "!default should only be written once for each variable.\nThis \
                             will be an error in Dart Sass 2.0.0."
                                .to_string(),
                        ));
                    }
                    is_guarded = true;
                }
                "global" => {
                    if namespace.is_some() {
                        return Err((
                            "!global isn't allowed for variables in other modules.",
                            self.toks_mut().span_from(flag_start),
                        )
                            .into());
                    }

                    if is_global {
                        let span = self.toks_mut().span_from(flag_start);
                        self.parse_time_warnings_mut().push((
                            Deprecation::DuplicateVarFlags,
                            span,
                            "!global should only be written once for each variable.\nThis \
                             will be an error in Dart Sass 2.0.0."
                                .to_string(),
                        ));
                    }

                    is_global = true;
                }
                _ => {
                    return Err(
                        ("Invalid flag name.", self.toks_mut().span_from(flag_start)).into(),
                    )
                }
            }

            self.whitespace()?;
        }

        self.expect_statement_separator(Some("variable declaration"))?;

        let declaration = AstVariableDecl {
            namespace,
            name: Identifier::from(name),
            value,
            is_guarded,
            is_global,
            span: self.toks_mut().span_from(start),
        };

        // Note: global variable pre-declaration is handled by
        // StyleSheet::collect_pre_declared_global_variables() after parsing.

        Ok(declaration)
    }

    fn almost_any_value(
        &mut self,
        // default=false
        omit_comments: bool,
    ) -> SassResult<InterpolationBuilder<'a>> {
        let mut buffer = InterpolationBuilder::new();
        let mut brackets: Vec<char> = Vec::new();
        let mut ident_buf = String::new();

        while let Some(tok) = self.toks().peek() {
            match tok.kind {
                '\\' => {
                    // Write a literal backslash because this text will be re-parsed.
                    buffer.add_char(tok.kind);
                    self.toks_mut().next();
                    match self.toks_mut().next() {
                        Some(tok) => buffer.add_char(tok.kind),
                        None => {
                            return Err(("expected more input.", self.toks().current_span()).into())
                        }
                    }
                }
                '"' | '\'' => {
                    let original_quote = tok.kind;
                    buffer.add_interpolation(InterpolationBuilder::from_interpolation(
                        self.parse_interpolated_string()?.node.as_interpolation(
                            false,
                            Some(original_quote),
                            self.arena(),
                        ),
                    ));
                }
                '/' => {
                    let comment_start = self.toks().cursor();
                    match self.toks().peek_n(1) {
                        Some(Token { kind: '/', .. }) if brackets.is_empty() => {
                            // Silent comments are always stripped, but only at the
                            // top level — inside parens (e.g. url-prefix(http://...))
                            // `//` is literal text, not a comment.
                            self.skip_silent_comment()?;
                        }
                        Some(Token { kind: '*', .. }) => {
                            self.skip_loud_comment()?;
                            if !omit_comments {
                                buffer
                                    .trailing_string_mut()
                                    .extend(self.toks().raw_chars(comment_start));
                            }
                        }
                        _ => {
                            buffer.add_char(self.toks_mut().next().unwrap().kind);
                        }
                    }
                }
                '#' => {
                    if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) {
                        // Add a full interpolated identifier to handle cases like
                        // "#{...}--1", since "--1" isn't a valid identifier on its own.
                        buffer.add_interpolation(self.parse_interpolated_identifier()?);
                    } else {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                    }
                }
                '\r' | '\n' => {
                    if self.is_indented()
                        && brackets.is_empty()
                        && !self.is_consuming_newlines()
                    {
                        break;
                    }
                    buffer.add_char(self.toks_mut().next().unwrap().kind);
                }
                '(' | '[' => {
                    let bracket = self.toks_mut().next().unwrap().kind;
                    buffer.add_char(bracket);
                    brackets.push(opposite_bracket(bracket));
                }
                ')' | ']' => {
                    if brackets.is_empty() {
                        break;
                    }
                    let expected = brackets.pop().unwrap();
                    self.expect_char(expected)?;
                    buffer.add_char(expected);
                }
                '!' | ';' | '{' | '}' => break,
                'u' | 'U' => {
                    let before_url = self.toks().cursor();
                    if !self.scan_identifier("url", false)? {
                        self.toks_mut().next();
                        buffer.add_char(tok.kind);
                        continue;
                    }

                    match self.try_url_contents(None)? {
                        Some(contents) => buffer.add_interpolation(contents),
                        None => {
                            self.toks_mut().set_cursor(before_url);
                            self.toks_mut().next();
                            buffer.add_char(tok.kind);
                        }
                    }
                }
                _ => {
                    if self.looking_at_identifier() {
                        ident_buf.clear();
                        self.parse_identifier_into(&mut ident_buf, false, false)?;
                        buffer.add_str(&ident_buf);
                    } else {
                        buffer.add_char(self.toks_mut().next().unwrap().kind);
                    }
                }
            }
        }

        Ok(buffer)
    }

    fn parse_variable_declaration_or_interpolation(
        &mut self,
    ) -> SassResult<VariableDeclOrInterpolation<'a>> {
        if !self.looking_at_identifier() {
            return Ok(VariableDeclOrInterpolation::Interpolation(
                self.parse_interpolated_identifier()?,
            ));
        }

        let start = self.toks().cursor();

        let ident = self.parse_identifier(false, false)?;
        if self.next_matches(".$") {
            let namespace_span = self.toks_mut().span_from(start);
            self.expect_char('.')?;
            Ok(VariableDeclOrInterpolation::VariableDecl(
                self.parse_variable_declaration_without_namespace(
                    Some(Spanned {
                        node: Identifier::from(ident),
                        span: namespace_span,
                    }),
                    Some(start),
                )?,
            ))
        } else {
            let mut buffer = InterpolationBuilder::new_plain(ident);

            if self.looking_at_interpolated_identifier_body() {
                buffer.add_interpolation(self.parse_interpolated_identifier()?);
            }

            Ok(VariableDeclOrInterpolation::Interpolation(buffer))
        }
    }

    fn looking_at_interpolated_identifier_body(&mut self) -> bool {
        match self.toks().peek() {
            Some(Token { kind: '\\', .. }) => true,
            Some(Token { kind: '#', .. })
                if matches!(self.toks().peek_n(1), Some(Token { kind: '{', .. })) =>
            {
                true
            }
            Some(Token { kind, .. }) if is_name(kind) => true,
            Some(..) | None => false,
        }
    }

    fn expression_until_comparison(&mut self) -> SassResult<Spanned<AstExpr<'a>>> {
        let value = self.parse_expression(
            Some(&|parser| {
                Ok(match parser.toks().peek() {
                    Some(Token { kind: '>', .. }) | Some(Token { kind: '<', .. }) => true,
                    Some(Token { kind: '=', .. }) => {
                        !matches!(parser.toks().peek_n(1), Some(Token { kind: '=', .. }))
                    }
                    _ => false,
                })
            }),
            None,
            None,
        )?;
        Ok(value)
    }

    fn parse_media_query_list(&mut self) -> SassResult<InterpolationBuilder<'a>> {
        let mut buf = InterpolationBuilder::new();
        loop {
            self.whitespace()?;
            self.parse_media_query(&mut buf)?;
            self.whitespace()?;
            if !self.scan_char(',') {
                break;
            }
            buf.add_char(',');
            buf.add_char(' ');
        }
        Ok(buf)
    }

    fn parse_media_in_parens(&mut self, buf: &mut InterpolationBuilder<'a>) -> SassResult<()> {
        self.expect_char_with_message('(', "media condition in parentheses")?;
        buf.add_char('(');
        // In indented syntax, allow newlines inside media query parens
        let was_consuming_newlines = self.is_consuming_newlines();
        self.set_consume_newlines(true);
        self.whitespace()?;

        if matches!(self.toks().peek(), Some(Token { kind: '(', .. })) {
            self.parse_media_in_parens(buf)?;
            self.whitespace()?;

            if self.scan_identifier("and", false)? {
                buf.add_string(" and ".to_owned());
                self.expect_whitespace()?;
                self.parse_media_logic_sequence(buf, "and")?;
            } else if self.scan_identifier("or", false)? {
                buf.add_string(" or ".to_owned());
                self.expect_whitespace()?;
                self.parse_media_logic_sequence(buf, "or")?;
            }
        } else if self.scan_identifier("not", false)? {
            buf.add_string("not ".to_owned());
            self.expect_whitespace()?;
            self.parse_media_or_interpolation(buf)?;
        } else {
            buf.add_expr(self.expression_until_comparison()?);

            if self.scan_char(':') {
                self.whitespace()?;
                buf.add_char(':');
                buf.add_char(' ');
                buf.add_expr(self.parse_expression(None, None, None)?);
            } else {
                let next = self.toks().peek();
                if matches!(
                    next,
                    Some(Token {
                        kind: '<' | '>' | '=',
                        ..
                    })
                ) {
                    let next = next.unwrap().kind;
                    buf.add_char(' ');
                    buf.add_char(self.toks_mut().next().unwrap().kind);

                    if (next == '<' || next == '>') && self.scan_char('=') {
                        buf.add_char('=');
                    }

                    buf.add_char(' ');

                    self.whitespace()?;

                    buf.add_expr(self.expression_until_comparison()?);

                    if (next == '<' || next == '>') && self.scan_char(next) {
                        buf.add_char(' ');
                        buf.add_char(next);

                        if self.scan_char('=') {
                            buf.add_char('=');
                        }

                        buf.add_char(' ');

                        self.whitespace()?;
                        buf.add_expr(self.expression_until_comparison()?);
                    }
                }
            }
        }

        self.set_consume_newlines(was_consuming_newlines);
        self.expect_char(')')?;
        self.whitespace()?;
        buf.add_char(')');

        Ok(())
    }

    fn parse_media_logic_sequence(
        &mut self,
        buf: &mut InterpolationBuilder<'a>,
        operator: &'static str,
    ) -> SassResult<()> {
        loop {
            self.parse_media_or_interpolation(buf)?;
            self.whitespace()?;

            if !self.scan_identifier(operator, false)? {
                return Ok(());
            }

            self.expect_whitespace()?;

            buf.add_char(' ');
            buf.add_string(operator.to_owned());
            buf.add_char(' ');
        }
    }

    fn parse_media_or_interpolation(&mut self, buf: &mut InterpolationBuilder<'a>) -> SassResult<()> {
        if self.toks_mut().next_char_is('#') {
            buf.add_interpolation(self.parse_single_interpolation()?);
        } else {
            self.parse_media_in_parens(buf)?;
        }

        Ok(())
    }

    fn parse_media_query(&mut self, buf: &mut InterpolationBuilder<'a>) -> SassResult<()> {
        if matches!(self.toks().peek(), Some(Token { kind: '(', .. })) {
            self.parse_media_in_parens(buf)?;
            self.whitespace()?;

            if self.scan_identifier("and", false)? {
                buf.add_string(" and ".to_owned());
                self.expect_whitespace()?;
                self.parse_media_logic_sequence(buf, "and")?;
            } else if self.scan_identifier("or", false)? {
                buf.add_string(" or ".to_owned());
                self.expect_whitespace()?;
                self.parse_media_logic_sequence(buf, "or")?;
            }

            return Ok(());
        }

        let ident1 = self.parse_interpolated_identifier()?;

        if ident1.as_plain().unwrap_or("").eq_ignore_ascii_case("not") {
            // For example, "@media not (...) {"
            self.expect_whitespace()?;
            if !self.looking_at_interpolated_identifier() {
                buf.add_string("not ".to_owned());
                self.parse_media_or_interpolation(buf)?;
                return Ok(());
            }
        }

        self.whitespace()?;
        buf.add_interpolation(ident1);
        if !self.looking_at_interpolated_identifier() {
            // For example, "@media screen {".
            return Ok(());
        }

        buf.add_char(' ');

        let ident2 = self.parse_interpolated_identifier()?;

        if ident2.as_plain().unwrap_or("").eq_ignore_ascii_case("and") {
            self.expect_whitespace()?;
            // For example, "@media screen and ..."
            buf.add_string(" and ".to_owned());
        } else {
            self.whitespace()?;
            buf.add_interpolation(ident2);

            if self.scan_identifier("and", false)? {
                // For example, "@media only screen and ..."
                self.expect_whitespace()?;
                buf.add_string(" and ".to_owned());
            } else {
                // For example, "@media only screen {"
                return Ok(());
            }
        }

        // We've consumed either `IDENTIFIER "and"` or
        // `IDENTIFIER IDENTIFIER "and"`.

        if self.scan_identifier("not", false)? {
            // For example, "@media screen and not (...) {"
            self.expect_whitespace()?;
            buf.add_string("not ".to_owned());
            self.parse_media_or_interpolation(buf)?;
            return Ok(());
        }

        self.parse_media_logic_sequence(buf, "and")?;

        Ok(())
    }
}

/// Approximates dart-sass's per-function `needsDeprecationWarning` check for
/// `@-moz-document` preludes (mirrors `mozDocumentRule` in
/// `lib/src/parse/stylesheet.dart`): warns unless every top-level
/// comma-separated term is a bare `url-prefix()` call with no argument or an
/// empty string argument (Gecko's still-supported "select everything" form).
///
/// grass parses the whole prelude as raw interpolated text rather than
/// dart's structured function-call grammar, so this operates on the
/// flattened plain text instead of matching per-function; any dynamic
/// `#{...}` interpolation trivially fails the check, matching dart (which
/// always warns when the prelude contains interpolation).
fn moz_document_prelude_needs_deprecation_warning(value: &Interpolation) -> bool {
    // Unlike `InterpolationBuilder::as_plain`, this concatenates every `String`
    // part instead of requiring exactly one — quoted string arguments (e.g.
    // `url-prefix("")`) are parsed as their own interpolation chunk(s) even
    // when they contain no actual `#{...}`, so a multi-chunk-but-still-fully-
    // static prelude must not be treated as dynamic.
    let mut text = String::new();
    for part in value.contents {
        match part {
            InterpolationPart::String(s) => text.push_str(s),
            InterpolationPart::Expr(..) => return true,
        }
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    !split_top_level_commas(trimmed).iter().all(|term| {
        matches!(term.trim(), "url-prefix()" | "url-prefix(\"\")" | "url-prefix('')")
    })
}

/// Splits `s` on commas that aren't nested inside parentheses.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;

    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    parts.push(&s[start..]);
    parts
}
