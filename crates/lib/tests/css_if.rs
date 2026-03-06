#[macro_use]
mod macros;

// Pure sass evaluation
test!(
    css_if_sass_true,
    "a {b: if(sass(true): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_sass_false_else,
    "a {b: if(sass(false): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_sass_false_no_else,
    "a {b: if(sass(false): c) == null}",
    "a {\n  b: true;\n}\n"
);
test!(
    css_if_sass_expression,
    "$a: true;\nb {c: if(sass($a): d; else: e)}",
    "b {\n  c: d;\n}\n"
);

// else clause
test!(
    css_if_else_alone,
    "a {b: if(else: c)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_else_two,
    "a {b: if(else: c; else: d)}",
    "a {\n  b: c;\n}\n"
);

// not operator
test!(
    css_if_not_true,
    "a {b: if(not sass(true): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_not_false,
    "a {b: if(not sass(false): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_not_paren,
    "a {b: if(not (sass(true)): c; else: d)}",
    "a {\n  b: d;\n}\n"
);

// and operator
test!(
    css_if_and_true_true,
    "a {b: if(sass(true) and sass(true): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_and_true_false,
    "a {b: if(sass(true) and sass(false): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_and_false_true,
    "a {b: if(sass(false) and sass(true): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_and_false_false,
    "a {b: if(sass(false) and sass(false): c; else: d)}",
    "a {\n  b: d;\n}\n"
);

// or operator
test!(
    css_if_or_true_true,
    "a {b: if(sass(true) or sass(true): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_or_true_false,
    "a {b: if(sass(true) or sass(false): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_or_false_true,
    "a {b: if(sass(false) or sass(true): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_or_false_false,
    "a {b: if(sass(false) or sass(false): c; else: d)}",
    "a {\n  b: d;\n}\n"
);

// Pure CSS passthrough
test!(
    css_if_css_alone,
    "a {b: if(css(): c)}",
    "a {\n  b: if(css(): c);\n}\n"
);
test!(
    css_if_css_else,
    "a {b: if(css(): c; else: d)}",
    "a {\n  b: if(css(): c; else: d);\n}\n"
);
test!(
    css_if_not_css,
    "a {b: if(not css(): c)}",
    "a {\n  b: if(not css(): c);\n}\n"
);
test!(
    css_if_css_and,
    "a {b: if(css(1) and css(2): c)}",
    "a {\n  b: if(css(1) and css(2): c);\n}\n"
);
test!(
    css_if_css_or,
    "a {b: if(css(1) or css(2): c)}",
    "a {\n  b: if(css(1) or css(2): c);\n}\n"
);

// Mixed sass + css
test!(
    css_if_mixed_true_and_css,
    "a {b: if(sass(true) and css(): c; else: d)}",
    "a {\n  b: if(css(): c; else: d);\n}\n"
);
test!(
    css_if_mixed_false_and_css,
    "a {b: if(sass(false) and css(): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_mixed_css_and_true,
    "a {b: if(css() and sass(true): c; else: d)}",
    "a {\n  b: if(css(): c; else: d);\n}\n"
);
test!(
    css_if_mixed_css_and_false,
    "a {b: if(css() and sass(false): c; else: d)}",
    "a {\n  b: d;\n}\n"
);
test!(
    css_if_mixed_true_or_css,
    "a {b: if(sass(true) or css(): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_mixed_false_or_css,
    "a {b: if(sass(false) or css(): c; else: d)}",
    "a {\n  b: if(css(): c; else: d);\n}\n"
);

// Short-circuit tests
test!(
    css_if_short_circuit_clause,
    "a {b: if(sass(true): c; sass($undefined): d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_short_circuit_and,
    "a {b: if(sass(false) and sass($undefined): c)}",
    ""
);
test!(
    css_if_short_circuit_or,
    "a {b: if(sass(true) or sass($undefined): c)}",
    "a {\n  b: c;\n}\n"
);

// Whitespace variations
test!(
    css_if_whitespace_after_open,
    "a {b: if( else: c)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_whitespace_before_colon,
    "a {b: if(else : c)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_no_whitespace_after_colon,
    "a {b: if(else:c)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_trailing_semi,
    "a {b: if(else: c;)}",
    "a {\n  b: c;\n}\n"
);

// Paren grouping
test!(
    css_if_paren_true,
    "a {b: if((sass(true)): c; else: d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    css_if_paren_false,
    "a {b: if((sass(false)): c; else: d)}",
    "a {\n  b: d;\n}\n"
);

// CSS paren
test!(
    css_if_paren_css,
    "a {b: if((css()): c)}",
    "a {\n  b: if((css()): c);\n}\n"
);

// Legacy if() still works
test!(
    legacy_if_true,
    "a {b: if(true, c, d)}",
    "a {\n  b: c;\n}\n"
);
test!(
    legacy_if_false,
    "a {b: if(false, c, d)}",
    "a {\n  b: d;\n}\n"
);

// Errors
error!(
    css_if_not_empty,
    "a {b: if(not: c)}", "Error: Expected identifier."
);

error!(
    css_if_not_without_space,
    "a {b: if(not(css()): d)}", "Error: Whitespace is required between \"not\" and \"(\""
);
error!(
    css_if_and_empty,
    "a {b: if(css(1) and: c)}", "Error: Expected identifier."
);
error!(
    css_if_or_empty,
    "a {b: if(css(1) or: c)}", "Error: Expected identifier."
);
