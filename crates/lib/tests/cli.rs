use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn grass_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_grass"))
}

/// Kills (and reaps) a spawned `--watch` child on drop, so a failing
/// assertion partway through a watch test can't leak a background process.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Polls `cond` (e.g. "does the output file now contain the new content?")
/// until it's true or `timeout` elapses. Used instead of a fixed sleep for
/// the `--watch` tests below, since recompile latency depends on the host's
/// filesystem-event delivery.
fn wait_for<F: Fn() -> bool>(timeout: Duration, cond: F) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn spawn_watch(input: &Path, output: &Path) -> (KillOnDrop, std::sync::mpsc::Receiver<String>) {
    let mut child = grass_cmd()
        .args([
            "--watch",
            "--no-source-map",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn grass --watch");

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(stdout).lines().flatten() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    (KillOnDrop(child), rx)
}

fn wait_for_watch_ready(output: &std::sync::mpsc::Receiver<String>) -> bool {
    wait_for(Duration::from_secs(10), || {
        output
            .try_iter()
            .any(|line| line == "Sass is watching for changes. Press Ctrl-C to stop.")
    })
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

// Regression test for todo #200: `grass --stdin <file>` must read the
// stylesheet from stdin and WRITE the compiled CSS to <file> (dart-sass
// behavior: `printf 'a{b:c}' | npx sass@1.97.3 --stdin out.css` writes
// "a {\n  b: c;\n}\n" to out.css). Before the fix, the lone positional was
// bound to INPUT and grass tried to READ it, failing with
// "No such file or directory". --no-source-map keeps the file contents equal
// to the plain expanded output (file output otherwise defaults source maps on
// and would append a sourceMappingURL comment). The CSS content itself is the
// same ground-truth-verified output as `stdin_expanded` above.
#[test]
fn stdin_writes_to_output_file() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("out.css");

    let output = run_with_stdin(
        &["--stdin", "--no-source-map", out_path.to_str().unwrap()],
        "a { b: c }",
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // With an output file given, nothing is written to stdout.
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);

    let file_contents = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(file_contents, "a {\n  b: c;\n}\n");
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

    // Source maps default to on for file output but off for stdout (matching
    // dart-sass — see cli_source_map.rs), so --no-source-map here keeps this
    // test's comparison isolated to "does writing to a file produce the same
    // CSS as stdout", not source-map behavior.
    let out_path = dir.path().join("out.css");
    let output = grass_cmd()
        .args([
            "--no-source-map",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn grass");
    assert!(output.status.success());

    let file_contents = std::fs::read(&out_path).unwrap();
    assert_eq!(file_contents, baseline.stdout);
}

// Regression test for todo #216, superseded by #226: a broken recompile
// must never silently truncate a previously-written good output file to 0
// bytes (the original #216 bug). #216's interim fix left the file completely
// untouched on failure; #226 replaces that with dart-sass's actual
// behavior (verified via npx sass@1.97.3, see error_css.rs and the probe
// transcripts on solo todo #226): by default (`--error-css`, on when writing
// to a file) the target is OVERWRITTEN with a synthesized "error CSS"
// stylesheet -- a valid, non-empty CSS file embedding the error message in a
// `body::before` rule (for live-reload UX) -- and under `--no-error-css` the
// file is DELETED outright. Both of these are real, intentional writes/
// deletes, not the silent-truncation bug #216 was about.
#[test]
fn broken_recompile_overwrites_existing_output_file_with_error_css() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let good = grass_cmd()
        .args(["--no-source-map", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(good.status.success());
    let good_contents = std::fs::read(&out_path).unwrap();
    assert!(!good_contents.is_empty());

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args(["--no-source-map", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));
    assert!(!broken.stderr.is_empty());

    let contents_after_failure = std::fs::read_to_string(&out_path).unwrap();
    assert_ne!(
        contents_after_failure, String::from_utf8(good_contents).unwrap(),
        "output file must be overwritten (not preserved) by a failed recompile"
    );
    // dart-sass's exact error-CSS template (verified via npx): a `/* ... */`
    // comment followed by a `body::before` rule with a `content:` property
    // embedding the error text.
    assert!(contents_after_failure.starts_with("/* Error:"));
    assert!(contents_after_failure.contains("body::before {"));
    assert!(contents_after_failure.contains("content: \"Error:"));
}

// Companion: `--no-error-css` deletes the output file outright on a failed
// recompile, rather than overwriting it with error CSS (verified via npx
// sass@1.97.3).
#[test]
fn broken_recompile_deletes_output_file_under_no_error_css() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let good = grass_cmd()
        .args(["--no-source-map", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(good.status.success());
    assert!(out_path.exists());

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args([
            "--no-source-map",
            "--no-error-css",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));
    assert!(!broken.stderr.is_empty());

    assert!(
        !out_path.exists(),
        "output file must be deleted by a failed recompile under --no-error-css"
    );
}

// Companion to the overwrite test above: the `.map` sibling written
// alongside `-o` output is left completely untouched by a failed recompile
// (verified via npx sass@1.97.3 -- dart never regenerates or deletes the
// `.map` file on error, even though it writes/removes the `.css` file
// itself).
#[test]
fn broken_recompile_preserves_existing_map_file() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    let map_path = dir.path().join("out.css.map");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let good = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(good.status.success());
    assert!(map_path.exists());
    let good_map_contents = std::fs::read(&map_path).unwrap();
    assert!(!good_map_contents.is_empty());

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));

    let map_contents_after_failure = std::fs::read(&map_path).unwrap();
    assert_eq!(
        map_contents_after_failure, good_map_contents,
        "map file must be untouched by a failed recompile"
    );
}

// Companion: the `.map` sibling is also left untouched under
// `--no-error-css`, even though the primary `.css` output is deleted
// (verified via npx sass@1.97.3).
#[test]
fn broken_recompile_preserves_existing_map_file_under_no_error_css() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    let map_path = dir.path().join("out.css.map");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let good = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(good.status.success());
    assert!(map_path.exists());
    let good_map_contents = std::fs::read(&map_path).unwrap();

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args(["--no-error-css", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));

    assert!(!out_path.exists(), "css output must be deleted under --no-error-css");
    let map_contents_after_failure = std::fs::read(&map_path).unwrap();
    assert_eq!(
        map_contents_after_failure, good_map_contents,
        "map file must be untouched even though the css output was deleted"
    );
}

// `--error-css` explicitly passed after an earlier `--no-error-css` wins
// (clap's `overrides_with`, last flag wins -- matches this CLI's existing
// convention for other `--[no-]foo` pairs).
#[test]
fn error_css_flag_overrides_earlier_no_error_css() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let good = grass_cmd()
        .args(["--no-source-map", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(good.status.success());

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args([
            "--no-source-map",
            "--no-error-css",
            "--error-css",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));

    let contents = std::fs::read_to_string(&out_path).unwrap();
    assert!(contents.starts_with("/* Error:"), "--error-css should win: {contents}");
}

// A failed compile on a target that never previously existed still gets an
// error-CSS file created for it by default (verified via npx sass@1.97.3).
#[test]
fn broken_compile_creates_error_css_for_nonexistent_target() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");

    std::fs::write(&in_path, "a { b: ").unwrap();
    let broken = grass_cmd()
        .args(["--no-source-map", in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(broken.status.code(), Some(1));

    let contents = std::fs::read_to_string(&out_path).unwrap();
    assert!(contents.starts_with("/* Error:"));
}

// Companion regression test: a successful recompile must still overwrite
// both the output file and its `.map` sibling with the new content (i.e.
// the fix for #216 doesn't accidentally make grass stop writing on success).
#[test]
fn successful_recompile_overwrites_output_and_map_file() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    let map_path = dir.path().join("out.css.map");

    std::fs::write(&in_path, "a { b: c }").unwrap();
    let first = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(first.status.success());
    let first_contents = std::fs::read(&out_path).unwrap();
    let first_map_contents = std::fs::read(&map_path).unwrap();

    std::fs::write(&in_path, "a { b: d }").unwrap();
    let second = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert!(second.status.success());

    let second_contents = std::fs::read(&out_path).unwrap();
    let second_map_contents = std::fs::read(&map_path).unwrap();
    assert_ne!(second_contents, first_contents);
    assert_ne!(second_map_contents, first_map_contents);
}

// Companion regression test: a broken compile targeting stdout must have no
// output-file side effects at all (there is no `-o` path to protect, but
// this locks in that the stdout branch is unaffected by the reordering).
#[test]
fn broken_compile_to_stdout_has_no_file_side_effects() {
    let output = run_with_stdin(&["--stdin"], "a { b: ");
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
    assert!(output.stdout.is_empty());
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

// Ground truth verified with dart-sass 1.97.3:
//   printf 'a{b:c}' | npx sass@1.97.3 --watch --stdin
//   -> "--watch is not allowed with --stdin.", exit 64
// grass exits 1 (see `error_exit_code`) but matches the message text and the
// "hard failure before compilation" behavior.
#[test]
fn watch_rejects_stdin() {
    let output = run_with_stdin(&["--watch", "--stdin"], "a { b: c }");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("--watch is not allowed with --stdin."),
        "stderr: {stderr}"
    );
}

// Ground truth verified with dart-sass 1.97.3:
//   npx sass@1.97.3 --watch in.scss
//   -> "--watch is not allowed when printing to stdout.", exit 64
#[test]
fn watch_rejects_stdout_output() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    std::fs::write(&in_path, "a { b: c }").unwrap();

    let output = grass_cmd()
        .args(["--watch", in_path.to_str().unwrap()])
        .output()
        .expect("failed to spawn grass");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("--watch is not allowed when printing to stdout."),
        "stderr: {stderr}"
    );
}

// Ground truth verified with dart-sass 1.97.3 (`npx sass@1.97.3 --watch
// in.scss out.css`): the first line of output is `[timestamp] Compiled
// in.scss to out.css.`, immediately followed by the "Sass is watching..."
// banner (no blank line between them; see watch.rs's module doc comment for
// the full transcript). Timestamps are UTC in grass rather than dart's local
// wall-clock time (see watch.rs), so this only checks the message shape.
#[test]
fn watch_prints_compiled_message_then_banner() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    std::fs::write(&in_path, "a { b: c }").unwrap();

    let mut child = grass_cmd()
        .args([
            "--watch",
            "--no-source-map",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn grass --watch");

    let stdout = child.stdout.take().unwrap();
    let _guard = KillOnDrop(child);

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(stdout).lines().flatten() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut lines = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while lines.len() < 2 && Instant::now() < deadline {
        if let Ok(line) = rx.recv_timeout(Duration::from_secs(1)) {
            lines.push(line);
        }
    }

    assert!(
        lines.len() >= 2,
        "expected at least 2 lines of watch output within 10s, got: {lines:?}"
    );
    assert!(
        lines[0].contains("Compiled") && lines[0].contains(" to ") && lines[0].ends_with('.'),
        "line0: {}",
        lines[0]
    );
    assert_eq!(lines[1], "Sass is watching for changes. Press Ctrl-C to stop.");
}

// End-to-end: editing the watched input file triggers a recompile with the
// new content, without the process exiting. Manually cross-checked against
// `npx sass@1.97.3 --watch` (see watch.rs's module doc comment).
#[test]
fn watch_recompiles_on_input_change() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    std::fs::write(&in_path, "a { b: c }").unwrap();

    let (_guard, watch_output) = spawn_watch(&in_path, &out_path);

    assert!(
        wait_for_watch_ready(&watch_output),
        "watcher did not become ready within 10s"
    );

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: c"))
        }),
        "initial compile did not appear within 10s"
    );

    std::fs::write(&in_path, "a { b: d }").unwrap();

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: d"))
        }),
        "recompile on input change did not appear within 10s"
    );
}

// End-to-end: editing a `@use`d partial (not the entry point itself) also
// triggers a recompile -- dependency tracking, not just entry-file watching.
// Manually cross-checked against `npx sass@1.97.3 --watch`.
#[test]
fn watch_recompiles_on_dependency_change() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let dep_path = dir.path().join("_dep.scss");
    let out_path = dir.path().join("out.css");
    std::fs::write(&dep_path, "$x: 1;\n").unwrap();
    std::fs::write(&in_path, "@use \"dep\";\na { b: dep.$x; }\n").unwrap();

    let (_guard, watch_output) = spawn_watch(&in_path, &out_path);

    assert!(
        wait_for_watch_ready(&watch_output),
        "watcher did not become ready within 10s"
    );

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: 1"))
        }),
        "initial compile did not appear within 10s"
    );

    std::fs::write(&dep_path, "$x: 999;\n").unwrap();

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: 999"))
        }),
        "recompile on dependency change did not appear within 10s"
    );
}

// End-to-end: an error mid-watch overwrites the output with error CSS (same
// format as the non-watch path -- `write_compile_result` is shared) and
// keeps watching; fixing the file recompiles normally. Manually
// cross-checked against `npx sass@1.97.3 --watch`.
#[test]
fn watch_recovers_from_error() {
    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("in.scss");
    let out_path = dir.path().join("out.css");
    std::fs::write(&in_path, "a { b: c }").unwrap();

    let (_guard, watch_output) = spawn_watch(&in_path, &out_path);

    assert!(
        wait_for_watch_ready(&watch_output),
        "watcher did not become ready within 10s"
    );

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: c"))
        }),
        "initial compile did not appear within 10s"
    );

    std::fs::write(&in_path, "a { b: ").unwrap();

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.starts_with("/* Error:"))
        }),
        "error CSS did not appear within 10s"
    );

    std::fs::write(&in_path, "a { b: e }").unwrap();

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: e"))
        }),
        "recovery recompile did not appear within 10s"
    );
}

// End-to-end, todo #274's critical case: a `@use`d partial containing only
// variables (no selectors, so it never emits a CSS mapping of its own) must
// still trigger a recompile. The partial also lives in a *sibling*
// directory outside the entry file's own directory tree and outside any
// `-I` load path, so this can only pass via `SourceMapData::loaded_files`
// (`Visitor::modules`) driving the watch set -- the old directory-recursive
// fallback (still exercised by the other watch tests, since their deps sit
// next to the entry file) would never see this directory at all.
#[test]
fn watch_recompiles_on_variable_only_partial_in_sibling_dir() {
    let root = tempfile::tempdir().unwrap();
    let main_dir = root.path().join("main");
    let shared_dir = root.path().join("shared");
    std::fs::create_dir_all(&main_dir).unwrap();
    std::fs::create_dir_all(&shared_dir).unwrap();

    let in_path = main_dir.join("in.scss");
    let out_path = main_dir.join("out.css");
    let vars_path = shared_dir.join("_vars.scss");

    // No selector/rule at all -- purely a variable declaration, so this
    // file contributes zero emitted CSS and would be invisible to a
    // `sources`-based (mapping-emission-scoped) watch set.
    std::fs::write(&vars_path, "$x: 1;\n").unwrap();
    std::fs::write(&in_path, "@use \"../shared/vars\" as vars;\na { b: vars.$x; }\n").unwrap();

    let (_guard, watch_output) = spawn_watch(&in_path, &out_path);

    assert!(
        wait_for_watch_ready(&watch_output),
        "watcher did not become ready within 10s"
    );

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: 1"))
        }),
        "initial compile did not appear within 10s"
    );

    std::fs::write(&vars_path, "$x: 999;\n").unwrap();

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: 999"))
        }),
        "recompile on variable-only sibling-dir partial change did not appear within 10s"
    );
}

// Negative counterpart to the test above: a `.scss` file that sits in a
// directory that's neither the entry file's own directory tree, an `-I`
// load path, nor a loaded file's directory must NOT trigger a recompile --
// confirming the watch set is actually scoped down, not just widened.
// Finishes by editing the real dependency to prove the watcher process is
// still alive and responsive (so a silently-hung watcher can't masquerade
// as "correctly ignored").
#[test]
fn watch_ignores_unrelated_sibling_dir() {
    let root = tempfile::tempdir().unwrap();
    let main_dir = root.path().join("main");
    let unrelated_dir = root.path().join("unrelated");
    std::fs::create_dir_all(&main_dir).unwrap();
    std::fs::create_dir_all(&unrelated_dir).unwrap();

    let in_path = main_dir.join("in.scss");
    let out_path = main_dir.join("out.css");
    let noise_path = unrelated_dir.join("_noise.scss");

    std::fs::write(&noise_path, "$noise: 1;\n").unwrap();
    std::fs::write(&in_path, "a { b: c; }\n").unwrap();

    let (_guard, watch_output) = spawn_watch(&in_path, &out_path);

    assert!(
        wait_for_watch_ready(&watch_output),
        "watcher did not become ready within 10s"
    );

    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: c"))
        }),
        "initial compile did not appear within 10s"
    );

    let compiled_at = std::fs::metadata(&out_path).unwrap().modified().unwrap();

    std::fs::write(&noise_path, "$noise: 2;\n").unwrap();
    assert!(
        !wait_for(Duration::from_millis(750), || {
            std::fs::metadata(&out_path)
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified != compiled_at)
        }),
        "editing an unrelated sibling directory should not have triggered a recompile"
    );

    std::fs::write(&in_path, "a { b: d; }\n").unwrap();
    assert!(
        wait_for(Duration::from_secs(10), || {
            std::fs::read_to_string(&out_path).is_ok_and(|c| c.contains("b: d"))
        }),
        "watcher appears hung: recompile on a real input change did not appear within 10s"
    );
}
