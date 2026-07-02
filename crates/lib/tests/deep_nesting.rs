// Regression tests for the recursion-depth guard (stack-overflow DoS).
//
// Rust stack overflow always aborts the process, so `catch_unwind` cannot
// catch it (see Plan 004). These inputs used to abort the process; they must
// now return a clean `Err` instead.
//
// Each case is run on a thread with an explicit 1 MiB stack — smaller than
// both cargo's default test-thread stack (2 MiB) and the main thread, and
// matching napi's worker-thread default (~1 MiB). This is the smallest
// stack grass runs on, so it's the environment the guard must survive.

fn is_ok_on_small_stack(input: String) -> bool {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || grass::from_string(input, &grass::Options::default()).is_ok())
        .unwrap()
        .join()
        .unwrap()
}

#[test]
#[ignore = "crashes the process (stack overflow abort) until the parser recursion guard lands"]
fn deeply_nested_rules_error_cleanly() {
    let input = format!("{}b:c;{}", "a{".repeat(50_000), "}".repeat(50_000));
    assert!(!is_ok_on_small_stack(input));
}

#[test]
#[ignore = "crashes the process (stack overflow abort) until the parser recursion guard lands"]
fn deeply_nested_parens_error_cleanly() {
    let input = format!("a{{b: {}1{};}}", "(".repeat(100_000), ")".repeat(100_000));
    assert!(!is_ok_on_small_stack(input));
}

#[test]
#[ignore = "crashes the process (stack overflow abort) until the parser recursion guard lands"]
fn deeply_nested_brackets_error_cleanly() {
    let input = format!("a{{b: {}1{};}}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(!is_ok_on_small_stack(input));
}

#[test]
#[ignore = "crashes the process (stack overflow abort) until the evaluator recursion guard lands"]
fn unbounded_recursive_function_errors_cleanly() {
    let input = "@function f($n) {\n  @return f($n + 1);\n}\na { b: f(1); }\n".to_string();
    assert!(!is_ok_on_small_stack(input));
}

#[test]
#[ignore = "crashes the process (stack overflow abort) until the evaluator recursion guard lands"]
fn unbounded_recursive_mixin_errors_cleanly() {
    let input = "@mixin m($n) {\n  @include m($n + 1);\n}\na { @include m(1); }\n".to_string();
    assert!(!is_ok_on_small_stack(input));
}

#[test]
fn reasonable_nesting_depth_still_compiles() {
    let input = format!("{}b:c;{}", "a{".repeat(20), "}".repeat(20));
    assert!(is_ok_on_small_stack(input));
}
