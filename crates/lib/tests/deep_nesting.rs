// Regression tests for the recursion-depth guards (stack-overflow DoS).
//
// Rust stack overflow always aborts the process, so `catch_unwind` cannot
// catch it (see Plan 004). These inputs used to abort the process; they must
// now return a clean `Err` instead.
//
// grass has three separate recursion guards with very different stack costs
// per level (see MAX_PARSER_RECURSION_DEPTH, MAX_STYLE_RULE_RECURSION_DEPTH,
// and MAX_CALLABLE_RECURSION_DEPTH in the compiler crate for the full
// measurements):
//
// - Parser recursion (nested blocks/parens/brackets) grows the stack on
//   demand via `stacker::maybe_grow` (todo #148) when the default-on
//   `stacker` feature is enabled.
// - Evaluator plain-nesting recursion (`a { b { c { ... } } }`, no
//   callables) is a *second*, separate recursion in evaluate/visitor.rs
//   (`Visitor::visit_ruleset`), guarded by its own
//   `MAX_STYLE_RULE_RECURSION_DEPTH` and also wrapped in `maybe_grow` (todo
//   #196). Before todo #196, this chokepoint had no guard and no stack
//   growth at all, so it was the real, unguarded ceiling for the full
//   parse+evaluate+serialize pipeline even after the parser guard alone was
//   raised (todo #148) — see MAX_PARSER_RECURSION_DEPTH's doc comment for
//   the historical crash boundaries this caused. Both limits are now kept in
//   sync at 1024, since they gate the same nesting from different layers.
//   With the `stacker` feature off (wasm32, where `stacker` isn't
//   supported), the parser limit drops back to 128 and stack growth never
//   happens for either chokepoint.
// - Evaluator callable recursion (function/mixin/content-block calls) is
//   expensive enough that MAX_CALLABLE_RECURSION_DEPTH (110) — sized to
//   support dart-sass-compatible bounded recursion like `sum(100)` — only
//   survives in release builds or on a stack larger than cargo test's debug
//   default. Those tests explicitly spawn an 8 MiB-stack thread (verified
//   safe for both directions: refuses unbounded recursion cleanly, and lets
//   sum(100) complete) so `cargo test` itself doesn't crash in debug mode.
//   This chokepoint is unaffected by todo #196 (separate recursion source).
//
// The plain-nesting tests near MAX_PARSER_RECURSION_DEPTH /
// MAX_STYLE_RULE_RECURSION_DEPTH below use the same spawned-larger-stack
// pattern where needed, though todo #196's fix means cargo test's own debug
// default thread (2 MiB) now comfortably survives depth 1024 directly (see
// that constant's doc comment for the measured boundary).

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

/// dart-sass 1.97.3 itself stack-overflows on plain brace nesting somewhere
/// between 450 (survives) and 500 (crashes) — confirmed via `npx sass@1.97.3
/// --stdin` (todo #196). 450 is therefore the natural dart-sass parity
/// point: grass's output at this depth was verified byte-identical to
/// dart-sass's (`--style=expanded`) as part of todo #196. This sits well
/// within both MAX_PARSER_RECURSION_DEPTH and MAX_STYLE_RULE_RECURSION_DEPTH
/// (1024), so grass now matches or exceeds dart-sass's own ceiling
/// end-to-end, not just at the parser layer (see todo #148, which only
/// achieved this for the parser in isolation).
#[test]
fn nesting_450_levels_matches_dart_sass() {
    let input = format!("{}b:c;{}", "a{".repeat(450), "}".repeat(450));
    assert!(is_ok_on_8mib_stack(input));
}

/// Just under MAX_PARSER_RECURSION_DEPTH / MAX_STYLE_RULE_RECURSION_DEPTH
/// (1024) must still compile...
#[test]
fn nesting_at_recursion_limit_boundary_still_compiles() {
    let input = format!("{}b:c;{}", "a{".repeat(1000), "}".repeat(1000));
    assert!(is_ok_on_8mib_stack(input));
}

/// ...while comfortably beyond it must still error cleanly (not crash). The
/// parser guard rejects this before recursing anywhere near either guarded
/// chokepoint's real crash boundary, so this is safe to call directly (no
/// spawned big-stack thread needed) even in debug builds.
#[test]
fn nesting_beyond_recursion_limit_errors_cleanly() {
    let input = format!("{}b:c;{}", "a{".repeat(1200), "}".repeat(1200));
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
