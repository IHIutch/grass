//! Synthesizes the "error CSS" stylesheet dart-sass writes to a file-output
//! target on a failed compile (`--error-css`, on by default when writing to
//! a file; see `--no-error-css`).
//!
//! Format and escaping rules verified byte-for-byte against
//! `npx sass@1.97.3` (crates/lib/tests/cli.rs has the probe transcripts) and
//! cross-checked against dart-sass's own implementation:
//! `lib/src/exception.dart` (`SassException.toCssString`) and
//! `lib/src/visitor/serialize.dart` (`_visitQuotedString` / `_writeEscape`).

/// Builds the full error-CSS stylesheet body for `display_text` (the
/// `Display` text of a failed compile's error, e.g. `e.to_string()`, with any
/// trailing newline still attached -- it is trimmed here).
///
/// dart-sass always renders the `/* ... */` comment header in ASCII (forcing
/// `term_glyph.ascii = true` for just that part -- see `toCssString`),
/// regardless of the terminal's Unicode setting, while the `content:`
/// property mirrors whatever was actually printed to the terminal. grass's
/// own error `Display` bakes its Unicode setting in at construction time
/// (`Options::unicode_error_messages`), so instead of re-rendering the error
/// twice, `unicode` is used to substitute the three box-drawing characters
/// grass's Unicode `Display` output can contain back to their ASCII
/// equivalents for the comment only. This does not change grass's own
/// (already dart-divergent -- see crates/compiler/src/error.rs) choice of
/// final location-line format; only the box-drawing bar characters are
/// normalized, matching dart's intent of keeping the comment readable in a
/// non-UTF-8 terminal without requiring a second compile.
pub fn synthesize(display_text: &str, unicode: bool) -> String {
    let base = display_text.trim_end_matches('\n');

    let mut comment_message = if unicode {
        base.replace('╷', ",").replace('│', "|").replace('╵', "'")
    } else {
        base.to_owned()
    };
    // Prevent the error text from prematurely closing the `/* ... */`
    // comment, and normalize CRLF to LF (dart-sass does both; see
    // `toCssString`).
    comment_message = comment_message.replace("*/", "*\u{2215}").replace("\r\n", "\n");

    let comment = format!("/* {} */", comment_message.replace('\n', "\n * "));
    let content = css_string_literal(base);

    let mut out = String::new();
    out.push_str(&comment);
    out.push_str("\n\nbody::before {\n");
    out.push_str("  font-family: \"Source Code Pro\", \"SF Mono\", Monaco, Inconsolata, \"Fira Mono\",\n");
    out.push_str("      \"Droid Sans Mono\", monospace, monospace;\n");
    out.push_str("  white-space: pre;\n");
    out.push_str("  display: block;\n");
    out.push_str("  padding: 1em;\n");
    out.push_str("  margin-bottom: 1em;\n");
    out.push_str("  border-bottom: 2px solid black;\n");
    out.push_str("  content: ");
    out.push_str(&content);
    out.push_str(";\n");
    out.push_str("}\n");
    out
}

/// Whether `c` is one of the C0 control characters (or DEL) that dart-sass's
/// CSS string serializer always escapes -- every C0 control character
/// *except* tab, which is passed through literally (see the `case $nul ||
/// ... || $del:` arm of `_visitQuotedString` in `serialize.dart`, which
/// conspicuously omits `$tab`).
fn is_css_control_char(c: char) -> bool {
    let code = c as u32;
    (code <= 0x1F && c != '\t') || code == 0x7F
}

/// Writes a single-character CSS escape (`\{hex}`), followed by a
/// disambiguating trailing space if `next` would otherwise be read as part
/// of the hex sequence (a hex digit, space, or tab) -- matching dart-sass's
/// `_writeEscape`.
fn write_escape(buf: &mut String, codepoint: u32, next: Option<char>) {
    buf.push('\\');
    buf.push_str(&format!("{codepoint:x}"));
    if let Some(next) = next {
        if next.is_ascii_hexdigit() || next == ' ' || next == '\t' {
            buf.push(' ');
        }
    }
}

/// Quotes `text` as a CSS string literal, matching dart-sass's
/// `_visitQuotedString`: prefers double quotes, falls back to single quotes
/// if the text contains a double quote but no single quote, and escapes
/// backslashes, whichever quote character is in use, and C0/DEL control
/// characters (other than tab).
fn visit_quoted_string(text: &str, force_double_quote: bool) -> String {
    let chars: Vec<char> = text.chars().collect();

    let mut includes_single_quote = false;
    let mut includes_double_quote = false;
    let mut buf = String::new();
    if force_double_quote {
        buf.push('"');
    }

    for i in 0..chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\'' if force_double_quote => buf.push('\''),
            '\'' if includes_double_quote => return visit_quoted_string(text, true),
            '\'' => {
                includes_single_quote = true;
                buf.push('\'');
            }
            '"' if force_double_quote => {
                buf.push('\\');
                buf.push('"');
            }
            '"' if includes_single_quote => return visit_quoted_string(text, true),
            '"' => {
                includes_double_quote = true;
                buf.push('"');
            }
            '\\' => {
                buf.push('\\');
                buf.push('\\');
            }
            c if is_css_control_char(c) => write_escape(&mut buf, c as u32, next),
            c => buf.push(c),
        }
    }

    if force_double_quote {
        buf.push('"');
        buf
    } else {
        let quote = if includes_double_quote { '\'' } else { '"' };
        format!("{quote}{buf}{quote}")
    }
}

/// Full two-stage CSS string-literal encoding dart-sass's `toCssString` uses
/// for the `content:` property: first quote-and-escape as an ordinary CSS
/// string (`visit_quoted_string`), then re-escape every remaining non-ASCII
/// rune (anything above U+007F -- i.e. whatever survived stage one
/// unescaped, such as literal accented letters or grass's Unicode
/// box-drawing bar characters) as `\{hex} ` with an unconditional trailing
/// space, so the file is safe to interpret as any encoding.
fn css_string_literal(text: &str) -> String {
    let quoted = visit_quoted_string(text, false);
    let mut out = String::new();
    for c in quoted.chars() {
        if (c as u32) > 0x7F {
            out.push('\\');
            out.push_str(&format!("{:x}", c as u32));
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::synthesize;

    // Ground truth verified with dart-sass 1.97.3 (probe captured with
    // Unicode terminal output on):
    //   printf 'a { b: ' > in.scss; npx sass@1.97.3 in.scss out.css
    #[test]
    fn simple_parse_error_unicode() {
        let display = "Error: Expected expression.\n  ╷\n1 │ a { b: \n  │        ^\n  ╵\n  in.scss 1:8  root stylesheet\n";
        let got = synthesize(display, true);
        let expected = "/* Error: Expected expression.\n *   ,\n * 1 | a { b: \n *   |        ^\n *   '\n *   in.scss 1:8  root stylesheet */\n\nbody::before {\n  font-family: \"Source Code Pro\", \"SF Mono\", Monaco, Inconsolata, \"Fira Mono\",\n      \"Droid Sans Mono\", monospace, monospace;\n  white-space: pre;\n  display: block;\n  padding: 1em;\n  margin-bottom: 1em;\n  border-bottom: 2px solid black;\n  content: \"Error: Expected expression.\\a   \\2577 \\a 1 \\2502  a { b: \\a   \\2502         ^\\a   \\2575 \\a   in.scss 1:8  root stylesheet\";\n}\n";
        assert_eq!(got, expected);
    }

    // Same message rendered in grass's ASCII (`--no-unicode`) form: the
    // comment and the content should both carry the plain ASCII bars, since
    // there are no non-ASCII runes left for stage two to escape.
    #[test]
    fn simple_parse_error_ascii() {
        let display = "Error: Expected expression.\n  ,\n1 | a { b: \n  |        ^\n  '\n  in.scss 1:8  root stylesheet\n";
        let got = synthesize(display, false);
        let expected = "/* Error: Expected expression.\n *   ,\n * 1 | a { b: \n *   |        ^\n *   '\n *   in.scss 1:8  root stylesheet */\n\nbody::before {\n  font-family: \"Source Code Pro\", \"SF Mono\", Monaco, Inconsolata, \"Fira Mono\",\n      \"Droid Sans Mono\", monospace, monospace;\n  white-space: pre;\n  display: block;\n  padding: 1em;\n  margin-bottom: 1em;\n  border-bottom: 2px solid black;\n  content: \"Error: Expected expression.\\a   ,\\a 1 | a { b: \\a   |        ^\\a   '\\a   in.scss 1:8  root stylesheet\";\n}\n";
        assert_eq!(got, expected);
    }

    // Ground truth verified with dart-sass 1.97.3 with a message containing
    // both quote types, a backslash, and a non-ASCII character (ü):
    //   printf '@error "contains \\"quotes\\" and \\\\ backslash and a \xc3\xbc char";' > q.scss
    //   npx sass@1.97.3 q.scss out3.css
    #[test]
    fn quotes_backslash_and_unicode_in_message() {
        let display = "Error: 'contains \"quotes\" and \\\\ backslash and a \u{fc} char'\n  ,\n1 | @error \"contains \\\"quotes\\\" and \\\\ backslash and a \u{fc} char\";\n  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n  '\n  q.scss 1:1  root stylesheet\n";
        let got = synthesize(display, false);
        assert!(got.contains(
            "content: \"Error: 'contains \\\"quotes\\\" and \\\\\\\\ backslash and a \\fc  char'\\a"
        ));
        assert!(got.starts_with(
            "/* Error: 'contains \"quotes\" and \\\\ backslash and a \u{fc} char'\n *   ,"
        ));
    }

    #[test]
    fn unicode_bars_normalized_to_ascii_in_comment_only() {
        let display = "Error: oops.\n  ╷\n1 │ a\n  │ ^\n  ╵\n  in.scss 1:1  root stylesheet\n";
        let got = synthesize(display, true);
        assert!(got.starts_with(
            "/* Error: oops.\n *   ,\n * 1 | a\n *   | ^\n *   '\n *   in.scss 1:1  root stylesheet */\n\nbody::before"
        ));
        // Content still carries the Unicode bars (escaped), matching dart's
        // "content reflects the actual terminal setting" behavior.
        assert!(got.contains("\\2577 "));
        assert!(got.contains("\\2502 "));
        assert!(got.contains("\\2575 "));
    }

    #[test]
    fn comment_close_sequence_is_escaped() {
        let display = "Error: a*/b.\n  ,\n1 | x\n  | ^\n  '\n  in.scss 1:1  root stylesheet\n";
        let got = synthesize(display, false);
        assert!(got.starts_with("/* Error: a*\u{2215}b."));
    }
}
