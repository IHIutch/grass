// Regression tests for the recursion-depth guards (stack-overflow DoS).
//
// Rust stack overflow always aborts the process, so `catch_unwind` cannot
// catch it (see Plan 004). These inputs used to abort the process; they must
// now return a clean `Err` instead.
//
// grass has two separate recursion guards with very different stack costs
// per level (see MAX_PARSER_RECURSION_DEPTH and MAX_CALLABLE_RECURSION_DEPTH
// in the compiler crate for the full measurements):
//
// - Parser recursion (nested blocks/parens/brackets) is cheap enough that
//   MAX_PARSER_RECURSION_DEPTH (128) survives comfortably on cargo test's
//   own default thread stack (~2 MiB in debug builds) — those tests below
//   run with no special stack handling.
// - Evaluator callable recursion (function/mixin/content-block calls) is
//   expensive enough that MAX_CALLABLE_RECURSION_DEPTH (110) — sized to
//   support dart-sass-compatible bounded recursion like `sum(100)` — only
//   survives in release builds or on a stack larger than cargo test's debug
//   default. Those tests explicitly spawn an 8 MiB-stack thread (verified
//   safe for both directions: refuses unbounded recursion cleanly, and lets
//   sum(100) complete) so `cargo test` itself doesn't crash in debug mode.

#[test]
fn deeply_nested_rules_error_cleanly() {
    let input = format!("{}b:c;{}", "a{".repeat(50_000), "}".repeat(50_000));
    assert!(grass::from_string(input, &grass::Options::default()).is_err());
}

#[test]
fn deeply_nested_parens_error_cleanly() {
    let input = format!("a{{b: {}1{};}}", "(".repeat(100_000), ")".repeat(100_000));
    assert!(grass::from_string(input, &grass::Options::default()).is_err());
}

#[test]
fn deeply_nested_brackets_error_cleanly() {
    let input = format!("a{{b: {}1{};}}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(grass::from_string(input, &grass::Options::default()).is_err());
}

#[test]
fn reasonable_nesting_depth_still_compiles() {
    let input = format!("{}b:c;{}", "a{".repeat(20), "}".repeat(20));
    assert!(grass::from_string(input, &grass::Options::default()).is_ok());
}

/// non_conformant/scss/huge.hrx (sass-spec) nests 59 levels of plain style
/// rules and must compile — this is the concrete regression a too-low
/// MAX_PARSER_RECURSION_DEPTH previously caused.
#[test]
fn huge_hrx_scale_nesting_still_compiles() {
    let input = format!("{}b:c;{}", "a{".repeat(59), "}".repeat(59));
    assert!(grass::from_string(input, &grass::Options::default()).is_ok());
}

fn is_ok_on_8mib_stack(input: String) -> bool {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || grass::from_string(input, &grass::Options::default()).is_ok())
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn unbounded_recursive_function_errors_cleanly() {
    let input = "@function f($n) {\n  @return f($n + 1);\n}\na { b: f(1); }\n".to_string();
    assert!(!is_ok_on_8mib_stack(input));
}

#[test]
fn unbounded_recursive_mixin_errors_cleanly() {
    let input = "@mixin m($n) {\n  @include m($n + 1);\n}\na { @include m(1); }\n".to_string();
    assert!(!is_ok_on_8mib_stack(input));
}

/// Every other Sass implementation compiles bounded recursive helpers like
/// this; grass previously rejected sum(40) and sum(100) (a confirmed
/// dart-sass-compat regression) because the recursion guard was sized too
/// low. See MAX_CALLABLE_RECURSION_DEPTH's doc comment for the measurements
/// behind the current value.
#[test]
fn sum_bounded_recursion_still_compiles() {
    let input = "@function sum($n) {\n  @if $n <= 0 {\n    @return 0;\n  }\n  @return $n + sum($n - 1);\n}\na { b: sum(100); }\n".to_string();
    assert!(is_ok_on_8mib_stack(input));
}
