use grass::Deprecation;
use macros::{TestFs, TestLogger};

#[macro_use]
mod macros;

#[test]
fn elseif_warns() {
    let input = "a {\n  @if false { b: c; }\n  @elseif true { b: d; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: d;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [elseif]: @elseif is deprecated and will not be supported in \
             future Sass versions."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("Recommendation: @else if"));
}

#[test]
fn elseif_dedupes_repeated_call_site() {
    let input = "@mixin m {\n  @if false { a: 1; }\n  @elseif true { a: 2; }\n}\n\
                 @each $n in 1, 2, 3 {\n  .c-#{$n} { @include m; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn import_warns() {
    let mut fs = TestFs::new();
    fs.add_file("_a.scss", "a { b: c; }");

    let input = "@import \"a\";";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: c;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [import]: Sass @import rules are deprecated and will be \
             removed in Dart Sass 3.0.0."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("https://sass-lang.com/d/import"));
}

#[test]
fn import_does_not_warn_for_css_passthrough() {
    let inputs = [
        "@import url(foo);",
        "@import \"plain.css\";",
        "@import \"foo\" screen;",
    ];

    for input in inputs {
        let logger = TestLogger::default();
        let options = grass::Options::default().logger(&logger);
        grass::from_string(input.to_string(), &options).expect(input);
        assert_eq!(
            &[] as &[String],
            logger.warning_messages().as_slice(),
            "unexpected warning for {}",
            input
        );
    }
}

#[test]
fn import_warns_once_per_occurrence() {
    let mut fs = TestFs::new();
    fs.add_file("_a.scss", "a { b: c; }");

    let input = "@import \"a\", \"a\";";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 2);
}

#[test]
fn new_global_warns_at_root() {
    let input = "$existing: 1;\na {\n  b: $existing;\n}\n$new-var: 2 !global;\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [new-global]: As of Dart Sass 2.0.0, !global assignments won't \
             be able to declare new variables."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("unnecessary and can safely be removed"));
}

#[test]
fn new_global_warns_when_nested() {
    let input = "a {\n  $new-var: 2 !global;\n  b: $new-var;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 2;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Recommendation: add `$new-var: null` at the stylesheet root."));
}

#[test]
fn new_global_does_not_warn_for_existing_global() {
    let input = "$g: 1;\na {\n  $g: 2 !global;\n  b: $g;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

fn assert_global_builtin_warning(input: &str, expected_module_dot_name: &str) {
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    // The warning is emitted before argument validation, so a case that
    // otherwise errors (e.g. an unrecognized function name, or a type
    // mismatch on a later argument) can still be used to test the warning.
    let _ = grass::from_string(input.to_string(), &options);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1, "expected exactly one warning for {input}");
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [global-builtin]: Global built-in functions are deprecated and \
             will be removed in Dart Sass 3.0.0."
        ),
        "unexpected warning for {input}: {}",
        warnings[0]
    );
    assert!(
        warnings[0].contains(&format!("Use {expected_module_dot_name} instead.")),
        "expected warning for {input} to recommend {expected_module_dot_name}, got: {}",
        warnings[0]
    );
}

/// Like `assert_global_builtin_warning`, but for call sites that also trip a
/// second, unrelated deprecation (e.g. `lighten()`/`saturate()` additionally
/// warn under `color-functions` for their `_suggestScaleAndAdjust` message) —
/// only checks that the global-builtin warning is present as the first one,
/// without asserting the total warning count.
fn assert_first_warning_is_global_builtin(input: &str, expected_module_dot_name: &str) {
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let _ = grass::from_string(input.to_string(), &options);
    let warnings = logger.warning_messages();
    assert!(!warnings.is_empty(), "expected at least one warning for {input}");
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [global-builtin]: Global built-in functions are deprecated and \
             will be removed in Dart Sass 3.0.0."
        ),
        "unexpected warning for {input}: {}",
        warnings[0]
    );
    assert!(
        warnings[0].contains(&format!("Use {expected_module_dot_name} instead.")),
        "expected warning for {input} to recommend {expected_module_dot_name}, got: {}",
        warnings[0]
    );
}

fn assert_no_global_builtin_warning(input: &str) {
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(
        &[] as &[String],
        logger.warning_messages().as_slice(),
        "unexpected warning for {input}"
    );
}

#[test]
fn global_builtin_warns_for_list_map_selector_string_math_functions() {
    assert_global_builtin_warning("a { b: nth(1px 2px, 1); }", "list.nth");
    assert_global_builtin_warning("a { b: list-separator(1px 2px); }", "list.separator");
    assert_global_builtin_warning(
        "$m: (a: 1);\na { b: map-get($m, a); }",
        "map.get",
    );
    assert_global_builtin_warning("a { b: selector-parse(\".a .b\"); }", "selector.parse");
    assert_global_builtin_warning("a { b: str-slice(\"hello\", 1, 2); }", "string.slice");
    assert_global_builtin_warning("a { b: comparable(1px, 2px); }", "math.compatible");
    assert_global_builtin_warning("a { b: unitless(1px); }", "math.is-unitless");
    assert_global_builtin_warning("a { b: max(1px, \"x\"); }", "math.max");
}

#[test]
fn global_builtin_warns_for_meta_functions() {
    assert_global_builtin_warning("a { b: variable-exists(foo); }", "meta.variable-exists");
    assert_global_builtin_warning("a { b: get-function(\"foo\"); }", "meta.get-function");
    assert_global_builtin_warning("a { b: inspect(1); }", "meta.inspect");
}

#[test]
fn global_builtin_does_not_warn_for_if() {
    // `if()` has no `sass:*` module equivalent in dart-sass. Uses the modern
    // if() syntax so this doesn't also trip the (separate) if-function
    // deprecation for the legacy call form.
    assert_no_global_builtin_warning("a { b: if(sass(true): 1; else: 2); }");
}

#[test]
fn global_builtin_warns_for_unconditional_color_functions() {
    // lighten() also trips color-functions (its own _suggestScaleAndAdjust
    // warning) — see global_builtin_warns_for_unconditional_color_functions's
    // sibling coverage in color_functions_scale_and_adjust_* below.
    assert_first_warning_is_global_builtin("a { b: lighten(red, 10%); }", "color.adjust");
    assert_global_builtin_warning("a { b: adjust-color(red, $red: 5); }", "color.adjust");
    assert_global_builtin_warning("a { b: scale-color(red, $red: 5%); }", "color.scale");
    assert_global_builtin_warning("a { b: change-color(red, $red: 5); }", "color.change");
    assert_global_builtin_warning("a { b: complement(red); }", "color.complement");
    assert_global_builtin_warning("a { b: mix(red, blue); }", "color.mix");
}

#[test]
fn global_builtin_grayscale_warns_only_for_color_arg() {
    assert_no_global_builtin_warning("a { b: grayscale(50%); }");
    assert_global_builtin_warning("a { b: grayscale(red); }", "color.grayscale");
}

#[test]
fn global_builtin_invert_warns_only_for_color_arg() {
    assert_no_global_builtin_warning("a { b: invert(50%); }");
    assert_global_builtin_warning("a { b: invert(red); }", "color.invert");
}

#[test]
fn global_builtin_opacity_warns_only_for_color_arg() {
    assert_no_global_builtin_warning("a { b: opacity(50%); }");
    assert_global_builtin_warning("a { b: opacity(red); }", "color.opacity");
}

#[test]
fn global_builtin_saturate_warns_only_for_two_arg_form() {
    assert_no_global_builtin_warning("a { b: saturate(50%); }");
    // The 2-arg form also trips color-functions (its own
    // _suggestScaleAndAdjust warning) — see color_functions_scale_and_adjust_*
    // below.
    assert_first_warning_is_global_builtin("a { b: saturate(red, 10%); }", "color.adjust");
}

#[test]
fn global_builtin_alpha_warns_unless_ms_filter() {
    assert_no_global_builtin_warning("a { b: alpha(opacity=50); }");
    assert_global_builtin_warning("a { b: alpha(red); }", "color.alpha");
}

#[test]
fn global_builtin_does_not_warn_for_css_color_constructors() {
    assert_no_global_builtin_warning("a { b: rgb(1, 2, 3); }");
    assert_no_global_builtin_warning("a { b: rgba(1, 2, 3, 0.5); }");
    assert_no_global_builtin_warning("a { b: hsl(1deg, 2%, 3%); }");
    assert_no_global_builtin_warning("a { b: hwb(1deg 2% 3%); }");
    assert_no_global_builtin_warning("a { b: ie-hex-str(red); }");
}

#[test]
fn global_builtin_does_not_warn_for_calc_safe_math_calls() {
    // Calc-safe positional-number calls resolve via the calculation
    // fallback, never reaching the global function table.
    assert_no_global_builtin_warning("a { b: max(1px, 2px); }");
    assert_no_global_builtin_warning("a { b: min(1px, 2px); }");
    assert_no_global_builtin_warning("a { b: abs(-5px); }");
    assert_no_global_builtin_warning("a { b: clamp(1px, 2px, 3px); }");
}

#[test]
fn global_builtin_module_call_does_not_warn() {
    assert_no_global_builtin_warning("@use \"sass:color\";\na { b: color.invert(red); }");
    assert_no_global_builtin_warning("@use \"sass:list\";\na { b: list.nth(1px 2px, 1); }");
    assert_no_global_builtin_warning("@use \"sass:color\";\na { b: color.alpha(red); }");
    assert_no_global_builtin_warning("@use \"sass:color\";\na { b: color.opacity(red); }");
}

#[test]
fn slash_div_warns_outside_calc() {
    let input = "a { b: (1/2) }";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 0.5;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [slash-div]: Using / for division outside of calc() is \
             deprecated and will be removed in Dart Sass 2.0.0."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("Recommendation: math.div(1, 2) or calc(1 / 2)"));
    assert!(warnings[0].contains("https://sass-lang.com/d/slash-div"));
}

#[test]
fn slash_div_warns_when_slash_tagged_value_is_used() {
    // `$a: 1/2` is a top-level assignment, so the division itself is tagged
    // with the operands rather than warning immediately; the warning instead
    // fires when the value's slash is later stripped (here, on use in a
    // declaration).
    let input = "$a: 1/2; a { b: $a }";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 0.5;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [slash-div]: Using / for division is deprecated and will be \
             removed in Dart Sass 2.0.0."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(!warnings[0].contains("outside of calc()"));
    assert!(warnings[0].contains("Recommendation: math.div(1, 2)"));
}

#[test]
fn slash_div_silenced() {
    let input = "a { b: (1/2) }";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::SlashDiv);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 0.5;\n}\n");
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn slash_div_fatal() {
    let input = "a { b: (1/2) }";
    let options = grass::Options::default().fatal_deprecation(Deprecation::SlashDiv);
    match grass::from_string(input.to_string(), &options) {
        Ok(..) => panic!("did not fail"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(
                    "Using / for division outside of calc() is deprecated and will be removed \
                     in Dart Sass 2.0.0."
                ),
                "unexpected error: {msg}"
            );
            assert!(msg.contains(
                "This is only an error because you've set the slash-div deprecation to be fatal."
            ));
        }
    }
}

#[test]
fn slash_div_dedupes_repeated_call_site() {
    // The same division, evaluated many times via a loop, should only warn
    // once per call site (matches dart-sass's per-(message, span) dedup).
    let input = "@each $n in 1, 2, 3, 4, 5 {\n  .a-#{$n} { b: 12 / $n; }\n}";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn no_warning_inside_calc() {
    let input = "a { b: calc(1 / 2) }";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn strict_unary_warns() {
    let input = "$a: 1;\n$b: 2;\na {\n  b: $a -$b;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: -1;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [strict-unary]: This operation is parsed as:\n\n    $a - $b"
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("but you may have intended it to mean:\n\n    $a (-$b)"));
    assert!(warnings[0].contains("https://sass-lang.com/d/strict-unary"));
}

#[test]
fn strict_unary_warns_for_plus() {
    let input = "$a: 1;\n$b: 2;\na {\n  b: $a +$b;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("This operation is parsed as:\n\n    $a + $b"));
    assert!(warnings[0].contains("but you may have intended it to mean:\n\n    $a (+$b)"));
}

#[test]
fn strict_unary_reconstructs_chained_left_operand() {
    // The "left" side of the ambiguous operator can itself be a composite
    // binary expression; the message should show the full chain, not just
    // the immediately-preceding term.
    let input = "$a: 1;\n$b: 2;\n$c: 3;\na {\n  b: $a - $b -$c;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("This operation is parsed as:\n\n    $a - $b - $c"));
    assert!(warnings[0].contains("but you may have intended it to mean:\n\n    $a - $b (-$c)"));
}

#[test]
fn strict_unary_does_not_warn_when_spaced_both_sides() {
    let input = "$a: 1;\n$b: 2;\na {\n  b: $a - $b;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn strict_unary_does_not_warn_for_negative_number_literal() {
    // `$a -1` attaches the `-` to the number literal (a space-separated
    // list), which is unambiguous and never reaches the binary-operator path.
    let input = "$a: 1;\na {\n  b: $a -1;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn strict_unary_dedupes_repeated_call_site() {
    let input = "$a: 1;\n$b: 2;\n@each $n in 1, 2, 3 {\n  .c-#{$n} { d: $a -$b; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn strict_unary_silenced() {
    let input = "$a: 1;\n$b: 2;\na {\n  b: $a -$b;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::StrictUnary);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn if_function_warns_with_suggestion() {
    let input = "a {\n  b: if(true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 1;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [if-function]: The Sass if() syntax is deprecated in favor \
             of the modern CSS syntax."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("Suggestion: if(sass(true): 1; else: 2)"));
    assert!(warnings[0].contains("More info: https://sass-lang.com/d/if-function"));
}

#[test]
fn if_function_suggestion_uses_not_sass_when_if_true_is_null() {
    let input = "a {\n  b: if(true, null, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Suggestion: if(not sass(true): 2)"));
}

#[test]
fn if_function_suggestion_omits_else_when_if_false_is_null() {
    let input = "a {\n  b: if(true, 1, null);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Suggestion: if(sass(true): 1)"));
    assert!(!warnings[0].contains("else"));
}

#[test]
fn if_function_reconstructs_nested_call_argument() {
    let input = "$list: 1, 2, 3;\n@use \"sass:list\";\na {\n  b: if(list.length($list) > 2, list.nth($list, 3), list.nth($list, 1));\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(
        "Suggestion: if(sass(list.length($list) > 2): list.nth($list, 3); else: list.nth($list, 1))"
    ));
}

#[test]
fn if_function_suggestion_adds_leading_zero_to_bare_decimals() {
    // dart-sass reconstructs numeric-literal arguments via SassNumber's
    // canonical (always-leading-zero) formatting, not raw source text.
    let input = "$v: -1;\na {\n  b: if($v < 0, -.5, .5);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Suggestion: if(sass($v < 0): -0.5; else: 0.5)"));
}

#[test]
fn if_function_omits_suggestion_for_non_three_arg_shape() {
    // 4 positional args isn't a shape `if()`'s declaration accepts, so this
    // still warns (parsing succeeds and the warning fires before argument
    // validation) but then errors during evaluation — matching dart-sass.
    let input = "a {\n  b: if(true, 1, 2, 3);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    assert!(grass::from_string(input.to_string(), &options).is_err());
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(!warnings[0].contains("Suggestion"));
    assert!(warnings[0].ends_with("More info: https://sass-lang.com/d/if-function"));
}

#[test]
fn if_function_does_not_warn_for_modern_syntax() {
    let input = "a {\n  b: if(sass(true): 1; else: 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn if_function_warns_even_when_call_is_unreached() {
    let input = "@function f() {\n  @if false {\n    @return if(true, 1, 2);\n  }\n  @return 0;\n}\na {\n  b: f();\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 0;\n}\n");
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn if_function_dedupes_repeated_call_site() {
    let input =
        "@each $n in 1, 2, 3 {\n  .c-#{$n} {\n    d: if(true, 1, 2);\n  }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn if_function_silenced() {
    let input = "a {\n  b: if(true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::IfFunction);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn bogus_combinators_warns_for_leading_combinator_and_keeps_selector() {
    let input = "+ .a {\n  b: c;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "+ .a {\n  b: c;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with(
            "DEPRECATION WARNING [bogus-combinators]: The selector \"+ .a\" is invalid CSS.\n\
             This will be an error in Dart Sass 2.0.0."
        ),
        "unexpected warning: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("https://sass-lang.com/d/bogus-combinators"));
}

#[test]
fn bogus_combinators_warns_for_trailing_combinator_with_declaration_and_omits_rule() {
    let input = ".a > {\n  b: c;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(
        "The selector \".a >\" is only valid for nesting and shouldn't\nhave children other \
         than style rules. It will be omitted from the generated CSS."
    ));
}

#[test]
fn bogus_combinators_does_not_warn_when_trailing_combinator_only_nests_style_rules() {
    let input = ".a > {\n  .b {\n    c: d;\n  }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, ".a > .b {\n  c: d;\n}\n");
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn bogus_combinators_warns_for_doubled_combinator_and_omits_rule() {
    let input = ".a + + .b {\n  c: d;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0]
        .contains("The selector \".a + + .b\" is invalid CSS. It will be omitted from the generated CSS."));
}

#[test]
fn bogus_combinators_does_not_warn_for_valid_selectors() {
    let input = ".a > .b {\n  c: d;\n}\na + .b {\n  c: d;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn bogus_combinators_warns_for_bogus_extender() {
    let input = "+ .a {\n  @extend .b;\n  c: d;\n}\n.b {\n  e: f;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    // The extender's own style-rule warning, plus the @extend-specific one —
    // not a third warning for `.b`'s selector list gaining "+ .a" via
    // extension (that complex selector isn't original to `.b`'s own rule).
    assert_eq!(warnings.len(), 2);
    assert!(warnings
        .iter()
        .any(|w| w.contains("is invalid CSS and shouldn't be an extender")));
    assert!(warnings
        .iter()
        .any(|w| w.starts_with("DEPRECATION WARNING [bogus-combinators]: The selector \"+ .a\" is invalid CSS.\n")));
}

#[test]
fn bogus_combinators_dedupes_repeated_call_site() {
    // NOTE: dart-sass's own dedup key is `(message, span)` (evaluate.dart's
    // `_warningsEmitted`), so it does NOT dedupe here (the selector text —
    // and thus the message — differs each iteration: ".c-1", ".c-2", ...) —
    // dart shows 3 warnings for this input. grass's `emit_deprecation`
    // dedups by `(Deprecation, Span)` only (a pre-existing choice from the
    // elseif/import/slash-div tranche, where the message text never varies
    // by loop iteration for those IDs), so it collapses to 1 here. Fixing
    // this would mean changing the shared dedup key for every deprecation,
    // out of scope for this tranche — documented as a known divergence.
    let input = "@each $n in 1, 2, 3 {\n  + .c-#{$n} {\n    d: e;\n  }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn bogus_combinators_silenced() {
    let input = "+ .a {\n  b: c;\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::BogusCombinators);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn color_functions_global_channel_getter_warns_without_prefix_and_alongside_global_builtin() {
    let input = "a {\n  b: red(#fff);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 255;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [global-builtin]:"));
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: red() is deprecated. Suggestion:\n\n\
         color.channel($color, \"red\", $space: rgb)"
    ));
}

#[test]
fn color_functions_module_channel_getter_warns_with_color_prefix_only() {
    let input = "@use \"sass:color\";\na {\n  b: color.red(#fff);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [color-functions]: color.red() is deprecated. Suggestion:\n\n\
         color.channel($color, \"red\", $space: rgb)"
    ));
}

#[test]
fn color_functions_warns_for_all_six_channel_getters() {
    for (name, space) in [
        ("red", "rgb"),
        ("green", "rgb"),
        ("blue", "rgb"),
        ("hue", "hsl"),
        ("saturation", "hsl"),
        ("lightness", "hsl"),
    ] {
        let input = format!("@use \"sass:color\";\na {{\n  b: color.{name}(#abc);\n}}\n");
        let logger = TestLogger::default();
        let options = grass::Options::default().logger(&logger);
        grass::from_string(input.clone(), &options).expect(&input);
        let warnings = logger.warning_messages();
        assert_eq!(warnings.len(), 1, "unexpected warnings for {name}: {warnings:?}");
        assert!(
            warnings[0].contains(&format!("color.channel($color, \"{name}\", $space: {space})")),
            "unexpected warning for {name}: {}",
            warnings[0]
        );
    }
}

#[test]
fn color_functions_adjust_hue_warns_with_suggestion() {
    let input = "a {\n  b: adjust-hue(red, 30deg);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: adjust-hue() is deprecated. Suggestion:\n\n\
         color.adjust($color, $hue: 30deg)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_lighten_warns_with_both_suggestions() {
    // #036 has HSL lightness 20%; +20 stays in bounds, so both the scale and
    // adjust suggestions are shown (dart-verified: 25%/20%).
    let input = "a {\n  b: lighten(#036, 20%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: lighten() is deprecated. Suggestions:\n\n\
         color.scale($color, $lightness: 25%)\n\
         color.adjust($color, $lightness: 20%)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_darken_clamps_factor_at_negative_boundary() {
    // #036's lightness (20%) minus 20 hits the channel's lower bound exactly,
    // so the scale factor clamps to -100% (dart-verified).
    let input = "a {\n  b: darken(#036, 20%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: darken() is deprecated. Suggestions:\n\n\
         color.scale($color, $lightness: -100%)\n\
         color.adjust($color, $lightness: -20%)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_saturate_two_arg_warns() {
    // #036 is fully saturated (100%); +20 overflows the channel max, so the
    // scale factor clamps to 100% (dart-verified).
    let input = "a {\n  b: saturate(#036, 20%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: saturate() is deprecated. Suggestions:\n\n\
         color.scale($color, $saturation: 100%)\n\
         color.adjust($color, $saturation: 20%)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_desaturate_warns() {
    let input = "a {\n  b: desaturate(#036, 20%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: desaturate() is deprecated. Suggestions:\n\n\
         color.scale($color, $saturation: -20%)\n\
         color.adjust($color, $saturation: -20%)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_omits_scale_suggestion_when_adjustment_is_zero() {
    let input = "a {\n  b: lighten(#036, 0%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: lighten() is deprecated. Suggestion:\n\n\
         color.adjust($color, $lightness: 0%)"
    ));
    assert!(!warnings[1].contains("color.scale"));
}

#[test]
fn color_functions_scale_and_adjust_opacify_and_fade_in_use_own_names() {
    let input_opacify = "a {\n  b: opacify(rgba(0, 0, 0, 0.5), 0.2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_opacify.to_string(), &options).expect(input_opacify);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: opacify() is deprecated. Suggestions:\n\n\
         color.scale($color, $alpha: 40%)\n\
         color.adjust($color, $alpha: 0.2)"
    ));

    let input_fade_in = "a {\n  b: fade-in(rgba(0, 0, 0, 0.5), 0.2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_fade_in.to_string(), &options).expect(input_fade_in);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: fade-in() is deprecated. Suggestions:\n\n\
         color.scale($color, $alpha: 40%)\n\
         color.adjust($color, $alpha: 0.2)"
    ));
}

#[test]
fn color_functions_scale_and_adjust_transparentize_and_fade_out_use_own_names() {
    let input_transparentize = "a {\n  b: transparentize(rgba(0, 0, 0, 0.5), 0.2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_transparentize.to_string(), &options).expect(input_transparentize);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: transparentize() is deprecated. Suggestions:\n\n\
         color.scale($color, $alpha: -40%)\n\
         color.adjust($color, $alpha: -0.2)"
    ));

    let input_fade_out = "a {\n  b: fade-out(rgba(0, 0, 0, 0.5), 0.2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_fade_out.to_string(), &options).expect(input_fade_out);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [color-functions]: fade-out() is deprecated. Suggestions:\n\n\
         color.scale($color, $alpha: -40%)\n\
         color.adjust($color, $alpha: -0.2)"
    ));
}

#[test]
fn color_functions_does_not_warn_for_channel_function() {
    let input = "@use \"sass:color\";\na {\n  b: color.channel(#fff, \"red\");\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn color_functions_silenced() {
    let input = "a {\n  b: red(#fff);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::ColorFunctions);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [global-builtin]:"));
}

#[test]
fn call_string_warns_with_quoted_reconstruction() {
    let input = "a {\n  b: call(\"if\", true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [call-string]: Passing a string to call() is deprecated and will \
         be illegal in Dart Sass 2.0.0.\n\nRecommendation: call(get-function(\"if\"))"
    ));
}

#[test]
fn call_string_reconstruction_preserves_unquoted_string() {
    let input = "a {\n  b: call(unquote(\"if\"), true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert!(warnings.iter().any(|w| w.starts_with(
        "DEPRECATION WARNING [call-string]: Passing a string to call() is deprecated and will \
         be illegal in Dart Sass 2.0.0.\n\nRecommendation: call(get-function(if))"
    )));
}

#[test]
fn call_string_does_not_warn_for_function_reference() {
    let input = "@use \"sass:meta\";\na {\n  b: meta.call(meta.get-function(\"if\"), true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert!(
        !logger.warning_messages().iter().any(|w| w.contains("[call-string]")),
        "unexpected call-string warning: {:?}",
        logger.warning_messages()
    );
}

#[test]
fn call_string_dedupes_repeated_call_site() {
    let input = "@each $n in 1, 2, 3 {\n  a { b: call(\"if\", true, 1, 2); }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.iter().filter(|w| w.contains("[call-string]")).count(), 1);
}

#[test]
fn call_string_silenced() {
    let input = "a {\n  b: call(\"if\", true, 1, 2);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::CallString);
    grass::from_string(input.to_string(), &options).expect(input);
    assert!(
        !logger.warning_messages().iter().any(|w| w.contains("[call-string]")),
        "unexpected call-string warning: {:?}",
        logger.warning_messages()
    );
}

#[test]
fn feature_exists_warns_for_global_and_module_forms() {
    let input_global = "a {\n  b: feature-exists(\"at-error\");\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_global.to_string(), &options).expect(input_global);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [global-builtin]:"));
    assert!(warnings[1].starts_with(
        "DEPRECATION WARNING [feature-exists]: The feature-exists() function is deprecated.\n\n\
         More info: https://sass-lang.com/d/feature-exists"
    ));

    let input_module =
        "@use \"sass:meta\";\na {\n  b: meta.feature-exists(\"at-error\");\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input_module.to_string(), &options).expect(input_module);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [feature-exists]:"));
}

#[test]
fn feature_exists_silenced() {
    let input = "@use \"sass:meta\";\na {\n  b: meta.feature-exists(\"at-error\");\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::FeatureExists);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn abs_percent_warns_for_bare_call() {
    let input = "a {\n  b: abs(-50%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: 50%;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [abs-percent]: Passing percentage units to the global abs() \
         function is deprecated.\nIn the future, this will emit a CSS abs() function to be \
         resolved by the browser.\nTo preserve current behavior: math.abs(-50%)\nTo emit a CSS \
         abs() now: abs(#{-50%})"
    ));
}

#[test]
fn abs_percent_warns_inside_explicit_calc() {
    let input = "a {\n  b: calc(abs(50%));\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [abs-percent]:"));
}

#[test]
fn abs_percent_warns_for_named_arg_form() {
    // The named-arg (non-calc-safe) form also trips global-builtin.
    let input = "a {\n  b: abs($number: -50%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [abs-percent]:"));
}

#[test]
fn abs_does_not_warn_for_non_percent_units() {
    let input = "a {\n  b: abs(-5px);\n  c: abs($number: -5px);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert!(
        !logger.warning_messages().iter().any(|w| w.contains("[abs-percent]")),
        "unexpected abs-percent warning: {:?}",
        logger.warning_messages()
    );
}

#[test]
fn abs_does_not_warn_for_module_form() {
    let input = "@use \"sass:math\";\na {\n  b: math.abs(-50%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn abs_percent_warns_once_per_distinct_call_site() {
    // AstExpr::Calculation now carries its own span (#187), so two `abs(-X%)`
    // calls at different source locations no longer collapse into one
    // warning via the (Deprecation, Span) dedup key.
    let input = "a {\n  b: abs(-50%);\n}\nc {\n  d: abs(-25%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 2, "expected 2 distinct warnings, got {:?}", warnings);
    assert!(warnings[0].contains("math.abs(-50%)"));
    assert!(warnings[1].contains("math.abs(-25%)"));
}

#[test]
fn abs_percent_silenced() {
    let input = "a {\n  b: abs(-50%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::AbsPercent);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn misplaced_rest_warns_for_positional_after_rest() {
    let input = "@mixin a($b, $args...) {}\na { @include a([1, 2]..., 3); }\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [misplaced-rest]: Positional arguments must come before rest \
         arguments.\nThis will be an error in Dart Sass 2.0.0."
    ));
}

#[test]
fn misplaced_rest_warns_for_named_after_rest() {
    let input = "@mixin a($a, $b, $c) {}\na { @include a([1, 2]..., $c: 3); }\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [misplaced-rest]: Named arguments must come before rest \
         arguments.\nThis will be an error in Dart Sass 2.0.0."
    ));
}

#[test]
fn misplaced_rest_does_not_warn_for_correctly_ordered_args() {
    let input = "@mixin a($a, $args...) {}\na { @include a(1, 2, 3...); }\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn misplaced_rest_dedupes_multiple_misplaced_args_at_one_call_site() {
    let input = "@mixin a($b, $c, $args...) {}\na { @include a([1, 2]..., 3, 4); }\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn misplaced_rest_silenced() {
    let input = "@mixin a($b, $args...) {}\na { @include a([1, 2]..., 3); }\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::MisplacedRest);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn moz_document_warns_for_url_prefix_with_argument() {
    let input = "@-moz-document url-prefix(a) {\n  a { b: c; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [moz-document]: @-moz-document is deprecated and support will be \
         removed in Dart Sass 2.0.0.\n\nFor details, see https://sass-lang.com/d/moz-document."
    ));
}

#[test]
fn moz_document_does_not_warn_for_empty_url_prefix() {
    let input = "@-moz-document url-prefix() {\n  a { b: c; }\n}\n\
                 @-moz-document url-prefix(\"\") {\n  a { b: c; }\n}\n\
                 @-moz-document url-prefix('') {\n  a { b: c; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn moz_document_warns_once_for_multiple_functions() {
    let input = "@-moz-document url(http://www.w3.org/), url-prefix(http://www.w3.org/Style/), \
                 domain(mozilla.org), regexp(\"https:.*\") {\n  a { b: c; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn moz_document_warns_for_interpolation() {
    let input = "@-moz-document url(#{\"sass-lang.com\"}) {\n  a { b: c; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(logger.warning_messages().len(), 1);
}

#[test]
fn moz_document_silenced() {
    let input = "@-moz-document url-prefix(a) {\n  a { b: c; }\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::MozDocument);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn with_private_warns_for_use() {
    let mut fs = TestFs::new();
    fs.add_file("_mod.scss", "$-private: red !default;\na { b: $-private; }");

    let input = "@use \"mod\" with ($-private: green);";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: green;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [with-private]: Configuring private variables is \
         deprecated.\nThis will be an error in Dart Sass 2.0.0."
    ));
}

#[test]
fn with_private_warns_for_forward() {
    let mut fs = TestFs::new();
    fs.add_file("_mod.scss", "$-private: red !default;\na { b: $-private; }");

    let input = "@forward \"mod\" with ($-private: green !default);";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("DEPRECATION WARNING [with-private]:"));
}

#[test]
fn with_private_warns_for_load_css() {
    let mut fs = TestFs::new();
    fs.add_file("_mod.scss", "$-private: red !default;\na { b: $-private; }");

    let input = "@use \"sass:meta\";\n@include meta.load-css(\"mod\", $with: (\"-private\": green));";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    let output = grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "a {\n  b: green;\n}\n");
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with(
        "DEPRECATION WARNING [with-private]: Configuring private variables (such as \
         $-private) is deprecated.\nThis will be an error in Dart Sass 2.0.0."
    ));
}

#[test]
fn with_private_does_not_warn_for_public_variables() {
    let mut fs = TestFs::new();
    fs.add_file("_mod.scss", "$public: red !default;\na { b: $public; }");

    let input = "@use \"mod\" with ($public: green);";
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger).fs(&fs);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

#[test]
fn with_private_silenced() {
    let mut fs = TestFs::new();
    fs.add_file("_mod.scss", "$-private: red !default;\na { b: $-private; }");

    let input = "@use \"mod\" with ($-private: green);";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .fs(&fs)
        .silence_deprecation(Deprecation::WithPrivate);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}

fn assert_color_module_compat_warning(input: &str, expected_prefix: &str) {
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    let warnings = logger.warning_messages();
    assert_eq!(warnings.len(), 1, "expected exactly one warning for {input}: {warnings:?}");
    assert!(
        warnings[0].starts_with(expected_prefix),
        "unexpected warning for {input}: {}",
        warnings[0]
    );
}

fn assert_no_color_module_compat_warning(input: &str) {
    let logger = TestLogger::default();
    let options = grass::Options::default().logger(&logger);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(
        &[] as &[String],
        logger.warning_messages().as_slice(),
        "unexpected warning for {input}"
    );
}

#[test]
fn color_module_compat_invert_warns_for_number() {
    assert_color_module_compat_warning(
        "@use \"sass:color\";\na {\n  b: color.invert(50%);\n}\n",
        "DEPRECATION WARNING [color-module-compat]: Passing a number (50%) to color.invert() \
         is deprecated.\n\nRecommendation: invert(50%)",
    );
}

#[test]
fn color_module_compat_grayscale_warns_for_number() {
    assert_color_module_compat_warning(
        "@use \"sass:color\";\na {\n  b: color.grayscale(50%);\n}\n",
        "DEPRECATION WARNING [color-module-compat]: Passing a number (50%) to \
         color.grayscale() is deprecated.\n\nRecommendation: grayscale(50%)",
    );
}

#[test]
fn color_module_compat_opacity_warns_for_number_with_dart_typo() {
    // Reproduces dart-sass's message verbatim, including its missing
    // closing paren after the number ("(0.5 to" not "(0.5) to").
    assert_color_module_compat_warning(
        "@use \"sass:color\";\na {\n  b: color.opacity(0.5);\n}\n",
        "DEPRECATION WARNING [color-module-compat]: Passing a number (0.5 to color.opacity() \
         is deprecated.\n\nRecommendation: opacity(0.5)",
    );
}

#[test]
fn color_module_compat_alpha_warns_for_ms_filter() {
    assert_color_module_compat_warning(
        "@use \"sass:color\";\na {\n  b: color.alpha(alpha=50);\n}\n",
        "DEPRECATION WARNING [color-module-compat]: Using color.alpha() for a Microsoft filter \
         is deprecated.\n\nRecommendation: alpha(alpha=50)",
    );
}

#[test]
fn color_module_compat_does_not_warn_for_actual_colors() {
    for input in [
        "@use \"sass:color\";\na {\n  b: color.invert(red);\n}\n",
        "@use \"sass:color\";\na {\n  b: color.grayscale(red);\n}\n",
        "@use \"sass:color\";\na {\n  b: color.opacity(red);\n}\n",
        "@use \"sass:color\";\na {\n  b: color.alpha(red);\n}\n",
    ] {
        assert_no_color_module_compat_warning(input);
    }
}

#[test]
fn color_module_compat_does_not_warn_for_global_forms() {
    for input in [
        "a {\n  b: invert(50%);\n}\n",
        "a {\n  b: grayscale(50%);\n}\n",
        "a {\n  b: opacity(0.5);\n}\n",
        "a {\n  b: alpha(alpha=50);\n}\n",
    ] {
        let logger = TestLogger::default();
        let options = grass::Options::default().logger(&logger);
        grass::from_string(input.to_string(), &options).expect(input);
        assert!(
            !logger.warning_messages().iter().any(|w| w.contains("[color-module-compat]")),
            "unexpected color-module-compat warning for {input}: {:?}",
            logger.warning_messages()
        );
    }
}

#[test]
fn color_module_compat_silenced() {
    let input = "@use \"sass:color\";\na {\n  b: color.invert(50%);\n}\n";
    let logger = TestLogger::default();
    let options = grass::Options::default()
        .logger(&logger)
        .silence_deprecation(Deprecation::ColorModuleCompat);
    grass::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}
