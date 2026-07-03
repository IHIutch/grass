use grass::Deprecation;
use macros::TestLogger;

#[macro_use]
mod macros;

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
            "Using / for division outside of calc() is deprecated and will be removed in Dart \
             Sass 2.0.0."
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
            "Using / for division is deprecated and will be removed in Dart Sass 2.0.0."
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
