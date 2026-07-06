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
// - Parser recursion (nested blocks/parens/brackets) grows the stack on
//   demand via `stacker::maybe_grow` (todo #148) when the default-on
//   `stacker` feature is enabled. This let MAX_PARSER_RECURSION_DEPTH double
//   (128 -> 256) — NOT the ~40x originally hoped for, because evaluating
//   plain nested style rules is a *second*, separate recursion in
//   evaluate/visitor.rs with no guard and no stack growth at all (only
//   *callable* function/mixin/content-block recursion is guarded there, via
//   MAX_CALLABLE_RECURSION_DEPTH). That unguarded evaluator recursion is now
//   the binding constraint on the full parse+evaluate+serialize pipeline —
//   see MAX_PARSER_RECURSION_DEPTH's doc comment for the measured crash
//   boundaries and the todo #148 follow-up this implies. With the feature
//   off (wasm32, where `stacker` isn't supported), the limit drops back to
//   128.
// - Evaluator callable recursion (function/mixin/content-block calls) is
//   expensive enough that MAX_CALLABLE_RECURSION_DEPTH (110) — sized to
//   support dart-sass-compatible bounded recursion like `sum(100)` — only
//   survives in release builds or on a stack larger than cargo test's debug
//   default. Those tests explicitly spawn an 8 MiB-stack thread (verified
//   safe for both directions: refuses unbounded recursion cleanly, and lets
//   sum(100) complete) so `cargo test` itself doesn't crash in debug mode.
//   This chokepoint is out of scope for todo #148 (owned separately).
//
// The plain-nesting tests near MAX_PARSER_RECURSION_DEPTH below use the same
// spawned-8-MiB-stack pattern, for the same reason: cargo test's own debug
// default thread (2 MiB) is close enough to the *evaluator's* unguarded
// crash boundary (measured ~270 levels) that tests near the 256 limit could
// otherwise flake in debug builds.

#[test]
fn deeply_nested_rules_error_cleanly() {
    let input = format!("{}b:c;{}", "a{".repeat(50_000), "}".repeat(50_000));
    let err = grass::from_string(input, &grass::Options::default()).unwrap_err();
    assert!(err.to_string().contains("Too much nesting."));
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

/// 200 levels sits comfortably within MAX_PARSER_RECURSION_DEPTH (256) and
/// was verified byte-identical to `npx sass@1.97.3 --style=compressed`
/// output (todo #148). Note this is well *under* dart-sass's own ~450-500
/// level crash boundary — see MAX_PARSER_RECURSION_DEPTH's doc comment for
/// why grass's real ceiling is currently lower than dart-sass's.
#[test]
fn nesting_200_levels_matches_dart_sass() {
    let input = format!("{}b:c;{}", "a{".repeat(200), "}".repeat(200));
    assert!(is_ok_on_8mib_stack(input));
}

/// Just under MAX_PARSER_RECURSION_DEPTH (256) must still compile...
#[test]
fn nesting_at_parser_recursion_limit_boundary_still_compiles() {
    let input = format!("{}b:c;{}", "a{".repeat(250), "}".repeat(250));
    assert!(is_ok_on_8mib_stack(input));
}

/// ...while comfortably beyond it must still error cleanly (not crash). The
/// parser guard rejects this before recursing anywhere near the evaluator's
/// own unguarded crash boundary, so this is safe to call directly (no
/// spawned big-stack thread needed) even in debug builds.
#[test]
fn nesting_beyond_parser_recursion_limit_errors_cleanly() {
    let input = format!("{}b:c;{}", "a{".repeat(300), "}".repeat(300));
    let err = grass::from_string(input, &grass::Options::default()).unwrap_err();
    assert!(err.to_string().contains("Too much nesting."));
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
