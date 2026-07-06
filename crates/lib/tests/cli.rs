use std::io::Write;
use std::process::{Command, Stdio};

fn grass_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_grass"))
}

fn run_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = grass_cmd()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn grass");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

// Ground truth verified with dart-sass 1.97.3:
//   printf 'a { b: c }' | npx sass@1.97.3 --stdin --style=expanded
//   -> "a {\n  b: c;\n}\n"
#[test]
fn stdin_expanded() {
    let output = run_with_stdin(&["--stdin"], "a { b: c }");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a {\n  b: c;\n}\n");
}

// Ground truth verified with dart-sass 1.97.3:
//   printf 'a { b: c }' | npx sass@1.97.3 --stdin --style=compressed
//   -> "a{b:c}\n" (dart-sass emits a trailing newline even in compressed mode)
#[test]
fn stdin_compressed() {
    let output = run_with_stdin(&["--stdin", "-s", "compressed"], "a { b: c }");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a{b:c}\n");
}

// Ground truth verified with dart-sass 1.97.3:
//   printf 'a { b: ' | npx sass@1.97.3 --stdin --style=expanded
//   -> exit 65, stderr "Error: Expected expression." (+ span), empty stdout
// grass's main.rs hard-codes `std::process::exit(1)` on compile error
// (crates/lib/src/main.rs:267-270), so we assert grass's own coded exit
// status (1) rather than dart-sass's (65) -- only the "fails with non-empty
// stderr and empty stdout" shape is being compared here.
#[test]
fn error_exit_code() {
    let output = run_with_stdin(&["--stdin"], "a { b: ");
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
    assert!(output.stdout.is_empty());
}

#[test]
fn load_path() {
    let dir = tempfile::tempdir().unwrap();
    let dep_path = dir.path().join("_dep.scss");
    std::fs::write(&dep_path, "a { b: c }").unwrap();

    let dir2 = tempfile::tempdir().unwrap();
    let in_path = dir2.path().join("in.scss");
    std::fs::write(&in_path, "@use \"dep\";").unwrap();

    let output = grass_cmd()
        .args(["-I", dir.path().to_str().unwrap(), in_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("a {"));
}

#[test]
fn output_file() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    std::fs::write(&in_path, "a { b: c }").unwrap();

    let baseline = grass_cmd()
        .arg(in_path.to_str().unwrap())
        .output()
        .expect("failed to spawn grass");
    assert!(baseline.status.success());

    let out_path = dir.path().join("out.css");
    let output = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(output.status.success());

    let file_contents = std::fs::read(&out_path).unwrap();
    assert_eq!(file_contents, baseline.stdout);
}

// Ground truth verified with dart-sass 1.97.3:
//   printf 'a { b: "ü" }' | npx sass@1.97.3 --stdin --style=expanded
//   -> starts with `@charset "UTF-8";\n`
//   printf 'a { b: "ü" }' | npx sass@1.97.3 --stdin --style=expanded --no-charset
//   -> does not emit a @charset line, starts directly with "a {"
#[test]
fn no_charset() {
    let with_charset = run_with_stdin(&["--stdin"], "a { b: \"\u{fc}\" }");
    assert!(with_charset.status.success());
    assert!(with_charset.stdout.starts_with(b"@charset \"UTF-8\";\n"));

    let without_charset = run_with_stdin(&["--stdin", "--no-charset"], "a { b: \"\u{fc}\" }");
    assert!(without_charset.status.success());
    assert!(without_charset.stdout.starts_with(b"a {"));
}

#[test]
fn input_file_missing() {
    let output = grass_cmd()
        .args(["/nonexistent/x.scss"])
        .output()
        .expect("failed to spawn grass");

    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
}

// Ground truth verified with dart-sass 1.97.3:
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --style=expanded
//   -> stderr contains "DEPRECATION WARNING [slash-div]: Using / for division..."
#[test]
fn deprecation_warning_by_default() {
    let output = run_with_stdin(&["--stdin"], "$a: 1;\nb { c: $a/2; }");
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("slash-div"), "stderr: {stderr}");
}

// Ground truth verified with dart-sass 1.97.3:
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --silence-deprecation=slash-div --style=expanded
//   -> compiles cleanly, no warning on stderr, exit 0
#[test]
fn silence_deprecation_removes_warning() {
    let output = run_with_stdin(
        &["--stdin", "--silence-deprecation=slash-div"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("slash-div"), "stderr: {stderr}");
    assert_eq!(output.stdout, b"b {\n  c: 0.5;\n}\n");
}

// Comma-separated and repeated-flag forms both compose, matching dart-sass's
// `addMultiOption` behavior (verified via npx sass@1.97.3).
#[test]
fn silence_deprecation_comma_and_repeat_forms() {
    let comma = run_with_stdin(
        &["--stdin", "--silence-deprecation=slash-div,import"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert!(comma.status.success());
    assert!(!String::from_utf8(comma.stderr).unwrap().contains("slash-div"));

    let repeated = run_with_stdin(
        &[
            "--stdin",
            "--silence-deprecation=slash-div",
            "--silence-deprecation=import",
        ],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert!(repeated.status.success());
    assert!(!String::from_utf8(repeated.stderr)
        .unwrap()
        .contains("slash-div"));
}

// Deprecation::from_id needs an arm for every seeded ID, including the two
// most recently seeded (function-units, duplicate-var-flags) — without one,
// `--silence-deprecation` hits the "Invalid deprecation" hard-failure path
// (see `unknown_deprecation_id_is_a_hard_failure` below) instead of actually
// silencing the warning.
//
// Ground truth verified with dart-sass 1.97.3:
//   printf '@use "sass:list";\na { b: list.nth(1px 2px 3px, 1px); }' | \
//     npx sass@1.97.3 --stdin --silence-deprecation=function-units --style=expanded
//   -> "a {\n  b: 1px;\n}\n", exit 0, no warning
//   printf '$a: 1 !default !default;\na { b: $a; }' | \
//     npx sass@1.97.3 --stdin --silence-deprecation=duplicate-var-flags --style=expanded
//   -> "a {\n  b: 1;\n}\n", exit 0, no warning
#[test]
fn silence_deprecation_accepts_function_units_and_duplicate_var_flags() {
    let function_units = run_with_stdin(
        &["--stdin", "--silence-deprecation=function-units"],
        "@use \"sass:list\";\na { b: list.nth(1px 2px 3px, 1px); }",
    );
    assert!(function_units.status.success());
    let stderr = String::from_utf8(function_units.stderr).unwrap();
    assert!(!stderr.contains("function-units"), "stderr: {stderr}");
    assert_eq!(function_units.stdout, b"a {\n  b: 1px;\n}\n");

    let duplicate_var_flags = run_with_stdin(
        &["--stdin", "--silence-deprecation=duplicate-var-flags"],
        "$a: 1 !default !default;\na { b: $a; }",
    );
    assert!(duplicate_var_flags.status.success());
    let stderr = String::from_utf8(duplicate_var_flags.stderr).unwrap();
    assert!(!stderr.contains("duplicate-var-flags"), "stderr: {stderr}");
    assert_eq!(duplicate_var_flags.stdout, b"a {\n  b: 1;\n}\n");
}

// Ground truth verified with dart-sass 1.97.3:
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --fatal-deprecation=slash-div --style=expanded
//   -> exit 65, stderr "Error: Using / for division outside of calc() is
//      deprecated..." + "This is only an error because you've set the
//      slash-div deprecation to be fatal.", empty stdout
// grass exits 1 (its own convention for CLI-level compile failures; see
// `error_exit_code` above) rather than dart's 65.
#[test]
fn fatal_deprecation_turns_warning_into_error() {
    let output = run_with_stdin(
        &["--stdin", "--fatal-deprecation=slash-div"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("slash-div"), "stderr: {stderr}");
}

// Ground truth verified with dart-sass 1.97.3:
//   echo "a{b:c}" | npx sass@1.97.3 --stdin --silence-deprecation=bogus-id --style=expanded
//   -> "Invalid deprecation "bogus-id"." + usage text, exit 64, empty stdout
// grass exits 1 (see `error_exit_code`) but matches the "hard failure before
// compilation, with the same message text" behavior.
#[test]
fn unknown_deprecation_id_is_a_hard_failure() {
    for flag in [
        "--silence-deprecation=bogus-id",
        "--fatal-deprecation=bogus-id",
        "--future-deprecation=bogus-id",
    ] {
        let output = run_with_stdin(&["--stdin", flag], "a { b: c }");
        assert_eq!(output.status.code(), Some(1), "flag: {flag}");
        assert!(output.stdout.is_empty(), "flag: {flag}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("Invalid deprecation \"bogus-id\"."),
            "flag: {flag}, stderr: {stderr}"
        );
    }
}

// `--future-deprecation` accepts a known ID without erroring, even though no
// currently-seeded deprecation is future-gated (`Deprecation::is_future` is
// `false` for all 16 IDs), so this only exercises flag acceptance.
#[test]
fn future_deprecation_flag_is_accepted() {
    let output = run_with_stdin(
        &["--stdin", "--future-deprecation=slash-div"],
        "a { b: c }",
    );
    assert!(output.status.success());
}

// Ground truth verified with dart-sass 1.97.3:
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --fatal-deprecation=1.23.0 --style=expanded
//   -> exit 0, warning still printed (1.23.0 predates slash-div's introduction
//      in 1.33.0, so the version-range expansion doesn't include it)
// grass's `Deprecation::for_version` mirrors dart's `Deprecation.forVersion`
// (introduced_in <= version) -- 1.23.0 only reaches up through
// color-module-compat, so slash-div (1.33.0) stays a plain warning.
#[test]
fn fatal_deprecation_version_range_excludes_later_id() {
    let output = run_with_stdin(
        &["--stdin", "--fatal-deprecation=1.23.0"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("slash-div"), "stderr: {stderr}");
}

// Ground truth verified with dart-sass 1.97.3 (boundary probe for #188):
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --fatal-deprecation=1.33.0 --style=expanded
//   -> exit 65, fatal error (slash-div's introduced_in, 1.33.0, is included:
//      the range is INCLUSIVE of the given version)
//   printf '$a: 1;\nb { c: $a/2; }' | npx sass@1.97.3 --stdin --fatal-deprecation=1.32.9 --style=expanded
//   -> exit 0, plain warning (1.32.9 < 1.33.0, so slash-div is excluded)
#[test]
fn fatal_deprecation_version_range_boundary_is_inclusive() {
    let at_boundary = run_with_stdin(
        &["--stdin", "--fatal-deprecation=1.33.0"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert_eq!(at_boundary.status.code(), Some(1));
    let stderr = String::from_utf8(at_boundary.stderr).unwrap();
    assert!(stderr.contains("slash-div"), "stderr: {stderr}");

    let below_boundary = run_with_stdin(
        &["--stdin", "--fatal-deprecation=1.32.9"],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert!(below_boundary.status.success());
}

// Ground truth verified with dart-sass 1.97.3 (probe for #188):
//   printf 'a{b:c}' | npx sass@1.97.3 --stdin --fatal-deprecation=1.2 --style=expanded
//   -> "Invalid deprecation "1.2"." + usage text, exit 64 (same for "1.2.3.4")
// A version-shaped string with the wrong number of parts is not treated as a
// version at all -- it's rejected the same as any other unrecognized ID.
#[test]
fn fatal_deprecation_malformed_version_is_an_invalid_id() {
    for flag in ["--fatal-deprecation=1.2", "--fatal-deprecation=1.2.3.4"] {
        let output = run_with_stdin(&["--stdin", flag], "a { b: c }");
        assert_eq!(output.status.code(), Some(1), "flag: {flag}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let expected_id = flag.split('=').nth(1).unwrap();
        assert!(
            stderr.contains(&format!("Invalid deprecation \"{expected_id}\".")),
            "flag: {flag}, stderr: {stderr}"
        );
    }
}

// dart-sass's `--fatal-deprecation` wins over `--silence-deprecation` for the
// same ID (verified via npx sass@1.97.3: emits a
// "WARNING: Ignoring setting to silence ... since it has also been made
// fatal." notice, then still errors). grass's evaluator already checks
// `fatal_deprecations` before `silence_deprecations`
// (crates/compiler/src/evaluate/visitor.rs) -- this test only confirms the
// CLI wiring doesn't disturb that precedence.
#[test]
fn fatal_wins_over_silence_for_same_id() {
    let output = run_with_stdin(
        &[
            "--stdin",
            "--fatal-deprecation=slash-div",
            "--silence-deprecation=slash-div",
        ],
        "$a: 1;\nb { c: $a/2; }",
    );
    assert_eq!(output.status.code(), Some(1));
}
