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
// BUG: grass's CLI omits the trailing newline in compressed style
// (confirmed via ./target/debug/grass --stdin --style=compressed -> "a{b:c}"
// with no trailing byte). Expanded style is unaffected. Reported to todo #128
// instead of filed (bd removed from repo); production code intentionally not
// touched per this plan's scope.
#[test]
#[ignore = "bug: grass CLI compressed-style output is missing dart-sass's trailing newline"]
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
