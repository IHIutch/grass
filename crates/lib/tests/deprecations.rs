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
    // `if()` has no `sass:*` module equivalent in dart-sass.
    assert_no_global_builtin_warning("a { b: if(true, 1, 2); }");
}

#[test]
fn global_builtin_warns_for_unconditional_color_functions() {
    assert_global_builtin_warning("a { b: lighten(red, 10%); }", "color.adjust");
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
    assert_global_builtin_warning("a { b: saturate(red, 10%); }", "color.adjust");
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
