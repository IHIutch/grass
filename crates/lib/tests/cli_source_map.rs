//! End-to-end CLI tests for source-map generation (todo #162). Ground truth
//! for every assertion here was captured by running the equivalent
//! `npx sass@1.97.3` invocation against the same fixture and comparing the
//! `.map` JSON / trailer comment byte-for-byte (see docs/design/source-maps.md
//! and the todo #162 comment thread for the exact commands run) — the
//! expected strings below are copied from that output, not derived from
//! grass's own implementation.

use std::process::Command;

const INPUT: &str = "a {\n  b: c;\n}\n";

fn grass_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_grass"))
}

// Ground truth: `cd <tmp> && npx sass@1.97.3 in.scss out.css`
//   -> out.css.map: {"version":3,"sourceRoot":"","sources":["in.scss"],
//      "names":[],"mappings":"AAAA;EACE","file":"out.css"}
//   -> out.css trailer: "...}\n\n/*# sourceMappingURL=out.css.map */\n"
#[test]
fn default_file_output_writes_relative_map_matching_dart_sass() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let css = std::fs::read(tmp.path().join("out.css")).unwrap();
    assert_eq!(
        css,
        b"a {\n  b: c;\n}\n\n/*# sourceMappingURL=out.css.map */\n".to_vec()
    );

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert_eq!(
        map,
        "{\"version\":3,\"sourceRoot\":\"\",\"sources\":[\"in.scss\"],\"names\":[],\"mappings\":\"AAAA;EACE\",\"file\":\"out.css\"}"
    );
}

// Ground truth: `npx sass@1.101.0 <absolute>/in.scss <absolute>/out.css`
// in a macOS tempfile whose `/var` spelling is symlinked -> sources:
// ["in.scss"] on the first compile.
#[test]
fn first_compile_into_absolute_tempdir_uses_relative_source() {
    let tmp = tempfile::tempdir().unwrap();
    let in_path = tmp.path().join("in.scss");
    let out_path = tmp.path().join("out.css");
    std::fs::write(&in_path, INPUT).unwrap();

    let output = grass_cmd()
        .args([in_path.to_str().unwrap(), out_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(map.contains("\"sources\":[\"in.scss\"]"), "got: {map}");
}

// Ground truth: `npx sass@1.97.3 src/in.scss build/out.css` (run from a
// directory containing both `src/` and `build/`) -> sources: ["../src/in.scss"]
#[test]
fn nested_output_dir_computes_dot_dot_relative_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("build")).unwrap();
    std::fs::write(tmp.path().join("src/in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["src/in.scss", "build/out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("build/out.css.map")).unwrap();
    assert!(
        map.contains("\"sources\":[\"../src/in.scss\"]"),
        "got: {map}"
    );
}

// Ground truth: `npx sass@1.97.3 sub/../in.scss sub/../out.css` ->
// sources: ["in.scss"]. The output directory exists, but the output file
// does not yet exist when grass constructs the source map.
#[test]
fn dotted_output_path_normalizes_before_relative_source_calculation() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["sub/../in.scss", "sub/../out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(map.contains("\"sources\":[\"in.scss\"]"), "got: {map}");
}

// Ground truth: `npx sass@1.97.3 actual/in.scss linked/out.css` where
// `linked` points to `actual` -> sources: ["../actual/in.scss"]. A missing
// output file must retain the symlinked directory in the fallback path.
#[cfg(unix)]
#[test]
fn missing_output_in_symlinked_dir_preserves_relative_source_behavior() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("actual")).unwrap();
    std::os::unix::fs::symlink("actual", tmp.path().join("linked")).unwrap();
    std::fs::write(tmp.path().join("actual/in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["actual/in.scss", "linked/out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("actual/out.css.map")).unwrap();
    assert!(
        map.contains("\"sources\":[\"../actual/in.scss\"]"),
        "got: {map}"
    );
}

// Ground truth: `npx sass@1.101.0 actual/in.scss linked/out.css` with an
// existing linked/out.css -> sources: ["../actual/in.scss"].
#[cfg(unix)]
#[test]
fn existing_output_in_symlinked_dir_preserves_relative_source_behavior() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("actual")).unwrap();
    std::os::unix::fs::symlink("actual", tmp.path().join("linked")).unwrap();
    std::fs::write(tmp.path().join("actual/in.scss"), INPUT).unwrap();
    std::fs::write(tmp.path().join("actual/out.css"), "existing\n").unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["actual/in.scss", "linked/out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("actual/out.css.map")).unwrap();
    assert!(
        map.contains("\"sources\":[\"../actual/in.scss\"]"),
        "got: {map}"
    );
}

// Ground truth: `npx sass@1.97.3 --source-map-urls=absolute in.scss out.css`
// -> sources: ["file:///<absolute path>/in.scss"]
#[test]
fn absolute_urls_produce_file_url() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--source-map-urls=absolute", "in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    let canonical = std::fs::canonicalize(tmp.path().join("in.scss")).unwrap();
    let expected_source = format!("\"sources\":[\"file://{}\"]", canonical.to_string_lossy());
    assert!(
        map.contains(&expected_source),
        "got: {map}\nwant substring: {expected_source}"
    );
}

// Ground truth: `npx sass@1.97.3 --embed-source-map in.scss out.css` -> no
// out.css.map file written; trailer is a `data:application/json` URL of the
// same JSON shape, still including "file":"out.css".
#[test]
fn embed_source_map_writes_no_map_file_and_embeds_data_url() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--embed-source-map", "in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    assert!(!tmp.path().join("out.css.map").exists());

    let css = std::fs::read_to_string(tmp.path().join("out.css")).unwrap();
    assert!(
        css.contains("/*# sourceMappingURL=data:application/json;charset=utf-8,"),
        "got: {css}"
    );
    assert!(css.contains("%22file%22:%22out.css%22"), "got: {css}");
}

// Ground truth: `npx sass@1.97.3 --embed-sources in.scss out.css` -> map file
// gains a "sourcesContent" array with the verbatim input text.
#[test]
fn embed_sources_adds_sources_content() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--embed-sources", "in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"sourcesContent\":[\"a {\\n  b: c;\\n}\\n\"]"),
        "got: {map}"
    );
}

// Ground truth: `npx sass@1.97.3 --no-source-map in.scss out.css` -> no map
// file, no trailer.
#[test]
fn no_source_map_disables_everything() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--no-source-map", "in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    assert!(!tmp.path().join("out.css.map").exists());
    let css = std::fs::read_to_string(tmp.path().join("out.css")).unwrap();
    assert_eq!(css, INPUT);
}

// Ground truth: `npx sass@1.97.3 in.scss` (no output arg, i.e. stdout) with
// no source-map flags at all -> plain CSS on stdout, no trailer, no error —
// dart-sass's default is source maps ON, but only when there's a file to
// write one to.
#[test]
fn stdout_default_produces_no_trailer() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, INPUT.as_bytes());
}

// Ground truth: `npx sass@1.97.3 --no-source-map --embed-source-map in.scss
// out.css` -> exit 64, stderr "--embed-source-map isn't allowed with
// --no-source-map.\n\n<usage>". grass uses its own exit-1 CLI-error
// convention (see error_exit_code in cli.rs) rather than dart's 64, but the
// message text itself is copied verbatim.
#[test]
fn no_source_map_conflicts_with_embed_source_map() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args([
            "--no-source-map",
            "--embed-source-map",
            "in.scss",
            "out.css",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .starts_with("--embed-source-map isn't allowed with --no-source-map."),
        "got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Ground truth: `npx sass@1.97.3 --source-map-urls=relative in.scss` (no
// output arg) -> exit 64, stderr "--source-map-urls=relative isn't allowed
// when printing to stdout.\n\n<usage>".
#[test]
fn relative_urls_conflict_with_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--source-map-urls=relative", "in.scss"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .starts_with("--source-map-urls=relative isn't allowed when printing to stdout."),
        "got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Ground truth: `npx sass@1.97.3 --embed-sources --no-source-map in.scss
// out.css` -> exit 64, stderr "--embed-sources isn't allowed with
// --no-source-map.\n\n<usage>" (message text copied verbatim; grass exits 1
// per its own CLI-error convention rather than dart's 64).
#[test]
fn embed_sources_conflicts_with_no_source_map() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--embed-sources", "--no-source-map", "in.scss", "out.css"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "--embed-sources isn't allowed with --no-source-map.\n"
    );
}

// Ground truth: `npx sass@1.97.3 --source-map-urls=absolute --no-source-map
// in.scss out.css` -> exit 64, stderr "--source-map-urls isn't allowed with
// --no-source-map.\n\n<usage>" (message text copied verbatim).
#[test]
fn source_map_urls_conflicts_with_no_source_map() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args([
            "--source-map-urls=absolute",
            "--no-source-map",
            "in.scss",
            "out.css",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "--source-map-urls isn't allowed with --no-source-map.\n"
    );
}

// Ground truth: `npx sass@1.97.3 --embed-sources in.scss` (no output arg,
// no --embed-source-map) -> exit 64, stderr "When printing to stdout,
// --embed-sources requires --embed-source-map.\n\n<usage>" (message text
// copied verbatim).
#[test]
fn embed_sources_without_embed_source_map_conflicts_with_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--embed-sources", "in.scss"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "When printing to stdout, --embed-sources requires --embed-source-map.\n"
    );
}

// Ground truth: `npx sass@1.97.3 --embed-source-map in.scss` (no output arg)
// -> works (unlike --source-map-urls/--embed-sources alone), falling back to
// an absolute file:// URL since there's no output directory for a relative
// path, and omitting the "file" key entirely.
#[test]
fn embed_source_map_alone_works_on_stdout_with_absolute_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), INPUT).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["--embed-source-map", "in.scss"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("sourceMappingURL=data:application/json;charset=utf-8,"),
        "got: {stdout}"
    );
    assert!(
        stdout.contains("file:"),
        "expected an absolute file: URL, got: {stdout}"
    );
    assert!(
        !stdout.contains("%22file%22"),
        "file key must be omitted, got: {stdout}"
    );
}

// `--stdin` with no OUTPUT arg (stdout) must remain byte-identical to plain
// stdin compilation (see `stdin_expanded` in cli.rs) now that stdin goes
// through `from_string_with_source_map` internally — the source-map wiring
// must not perturb the plain-stdin path at all.
//
// Note: `--stdin <file>` (an OUTPUT positional alongside --stdin) is not
// covered here — clap currently binds a single trailing positional to INPUT
// rather than OUTPUT even when --stdin is set, a pre-existing ambiguity
// unrelated to source maps (filed as follow-up work; see todo #162's closing
// comment). `from_string_with_source_map`'s data:-URL-sources behavior is
// covered directly at the library level in tests/source_maps.rs instead.
#[test]
fn stdin_without_output_arg_is_unaffected() {
    use std::io::Write as _;
    use std::process::Stdio;

    let tmp = tempfile::tempdir().unwrap();

    let mut child = grass_cmd()
        .current_dir(tmp.path())
        .args(["--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"a { b: c; }\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"a {\n  b: c;\n}\n".to_vec());
}

// ---- Slice 5/6 follow-on (todo #203): comments and UTF-16 columns ----
//
// Ground truth for every test below was captured the same way as the rest of
// this file: run the equivalent `npx sass@1.97.3` invocation and decode its
// `mappings` VLQ string by hand (see the small Python decoder used during
// development; not checked in — the decoded, human-readable positions are
// transcribed into each test's comment).

// Ground truth: a standalone comment on its own indented line is mapped to
// the START of its generated line (column 0), before indentation is
// written -- unlike declarations/selectors, which map after indentation.
// Verified via:
//   .a {\n  /* c1 */\n  color: red;\n  /* c2 */\n}\n
// -> dart mappings "AAAA;AACE;EACA;AACA": dst(1,0)->src(1,2) i.e. the
// comment text starts at source col 2 but is mapped to generated col 0.
#[test]
fn standalone_comment_maps_to_column_zero_before_indentation() {
    let input = ".a {\n  /* c1 */\n  color: red;\n  /* c2 */\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;AACE;EACA;AACA\""),
        "got: {map}"
    );
}

// Ground truth: a comment squeezed onto the same generated line as an
// opening `{` (dart-sass's "comment-only body renders inline" rule, and the
// general opening-brace/declaration-trailing-comment squeeze) gets NO
// mapping at all -- only the selector itself is mapped. Verified against
// `.b { /* inline-body */ }` (single statement) -> mappings "AAAA" (one
// segment only, for the selector; nothing for the comment).
#[test]
fn comment_only_body_on_brace_line_is_not_mapped() {
    let input = ".b { /* inline-body */ }\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(map.contains("\"mappings\":\"AAAA\""), "got: {map}");
}

// Ground truth: a comment trailing a declaration on the same source line
// (`color: blue; /* trailing */`) is NOT mapped -- dart emits only the
// declaration's own segment for that generated line.
#[test]
fn comment_trailing_declaration_is_not_mapped() {
    let input = ".baz {\n  color: blue; /* trailing */\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    // Only 2 segments total: the selector and the declaration -- the
    // trailing comment contributes no third segment.
    assert!(map.contains("\"mappings\":\"AAAA;EACE\""), "got: {map}");
}

// Ground truth: a comment trailing a `}` on the same source line (dart-sass
// "Sub-problem C") DOES get its own mapping, at the column right after `} `
// -- unlike the declaration/opening-brace trailing-comment cases above.
// Verified via:
//   .q {\n  color: blue;\n} /* trailing after close */\n.r { color: green; }\n
// -> dart mappings "AAAA;EACE;EACA;AACF;EAAK".
#[test]
fn comment_trailing_closing_brace_is_mapped() {
    let input = ".q {\n  color: blue;\n} /* trailing after close */\n.r { color: green; }\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;EACA;AACF;EAAK\""),
        "got: {map}"
    );
}

// ---- Slice 6 (todo #203): UTF-16 column semantics ----

// Ground truth: Source Map v3 columns are UTF-16 code units. An emoji
// (supplementary-plane, encodes as a UTF-16 surrogate pair = 2 units) placed
// before another mapped token on the same source line shifts that token's
// mapped column by 2, not 1 (which is what a Unicode-scalar-value count would
// give). Verified via:
//   a {\n  content: "\u{1F600}"; color: red;\n}\n
// -> dart mappings ";AAAA;EACE;EAAe" (the leading `;` is the `@charset`
// line dart-sass prepends for non-ASCII output; decodes to dst(2,2)->src
// col17, i.e. UTF-16 units, not the 16 a scalar count would give).
#[test]
fn emoji_before_mapped_token_uses_utf16_column() {
    let input = "a {\n  content: \"\u{1F600}\"; color: red;\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\";AAAA;EACE;EAAe\""),
        "got: {map}"
    );
}

// Ground truth: `Serializer::finish` prepends `@charset "UTF-8";\n` for
// non-ASCII output *after* mappings are already collected relative to the
// pre-prepend buffer. Every mapping's generated line must be shifted by +1
// to stay correct -- dart-sass's own mappings for any non-ASCII fixture
// start with a leading empty group (`;...`) for exactly this reason. This
// is a plain-ASCII-content regression check: a non-ASCII *comment* (no
// emoji-column interaction) still needs the whole-mappings-string line
// shift.
#[test]
fn charset_prepend_shifts_all_mapping_lines() {
    let input = "a {\n  /* \u{00e9} */\n  b: c;\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let css = std::fs::read(tmp.path().join("out.css")).unwrap();
    assert!(
        css.starts_with(b"@charset \"UTF-8\";\n"),
        "expected @charset prefix, got: {}",
        String::from_utf8_lossy(&css)
    );

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    // First mapping group is now empty (dst line 0 = the @charset line,
    // unmapped); the selector (line 1), comment (line 2), and declaration
    // (line 3) mappings follow with an extra +1 line shift baked in.
    assert!(
        map.contains("\"mappings\":\";AAAA;AACE;EACA\""),
        "got: {map}"
    );
}

// `@supports` at-rule mapping (todo #269, follow-up to #225 part 1).
// `AstSupportsRule.span` is the *body* span (starts at `{`), unlike
// Media/UnknownAtRule whose span already starts at `@` -- so this needed a
// separate `at_rule_span` threaded from the parser's dispatch-arm `start`
// position. Ground truth: `npx sass@1.97.3 in.scss out.css` on
// `@supports (display: grid) {\n  a { b: c; }\n}\n` -> mappings
// "AAAA;EACE;IAAI" (maps the `@supports` keyword to source 0:0, not the `{`
// at 0:26).
#[test]
fn supports_at_rule_maps_to_keyword_not_body() {
    let input = "@supports (display: grid) {\n  a { b: c; }\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;IAAI\""),
        "got: {map}"
    );
}

// Nested `@supports` -- ground truth: `npx sass@1.97.3` on
// `@supports (display: grid) {\n  @supports (color: red) {\n    a { b: c; }\n  }\n}\n`
// -> mappings "AAAA;EACE;IACE;MAAI" (each `@supports` keyword maps to its own
// source line, not the `{`).
#[test]
fn nested_supports_at_rule_maps_to_keyword() {
    let input =
        "@supports (display: grid) {\n  @supports (color: red) {\n    a { b: c; }\n  }\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;IACE;MAAI\""),
        "got: {map}"
    );
}

// `@supports` nested inside `@media` -- ground truth: `npx sass@1.97.3` on
// `@media (min-width: 100px) {\n  @supports (display: grid) {\n    a { b: c; }\n  }\n}\n`
// -> mappings "AAAA;EACE;IACE;MAAI" (both at-rule keywords map correctly
// regardless of which at-rule kind wraps which).
#[test]
fn supports_inside_media_maps_both_keywords() {
    let input =
        "@media (min-width: 100px) {\n  @supports (display: grid) {\n    a { b: c; }\n  }\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;IACE;MAAI\""),
        "got: {map}"
    );
}

// `@media` at-rule mapping (todo #225 part 2, landed alongside @font-face
// and @keyframes in Plan 074/06ac94e). `AstMediaRule`'s `at_rule_span`
// already starts at `@`, unlike `@supports` above. Ground truth:
// `npx sass@1.97.3 --source-map` on
// `@media (min-width: 100px) {\n  a { b: c; }\n}\n` -> mappings
// "AAAA;EACE;IAAI" (the `@media` keyword maps to source 0:0, not the `{`).
#[test]
fn media_at_rule_maps_to_keyword() {
    let input = "@media (min-width: 100px) {\n  a { b: c; }\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;IAAI\""),
        "got: {map}"
    );
}

// `@font-face` at-rule mapping -- unlike `@media`/`@supports`, `@font-face`
// has no selector/prelude, only declarations directly in its body. Ground
// truth: `npx sass@1.97.3 --source-map` on
// `@font-face {\n  font-family: "Foo";\n}\n` -> mappings "AAAA;EACE" (the
// `@font-face` keyword maps to source 0:0; the declaration line maps
// normally).
#[test]
fn font_face_at_rule_maps_to_keyword() {
    let input = "@font-face {\n  font-family: \"Foo\";\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(map.contains("\"mappings\":\"AAAA;EACE\""), "got: {map}");
}

// `@keyframes` at-rule mapping, including `from`/`to` selectors. Ground
// truth: `npx sass@1.97.3 --source-map` on
// `@keyframes foo {\n  from { a: b; }\n  to { c: d; }\n}\n` -> mappings
// "AAAA;EACE;IAAO;;EACP;IAAK" (the `@keyframes` keyword maps to source 0:0,
// and each of `from`/`to` maps as its own selector line, same as a regular
// ruleset selector).
#[test]
fn keyframes_at_rule_maps_from_to_selectors() {
    let input = "@keyframes foo {\n  from { a: b; }\n  to { c: d; }\n}\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE;IAAO;;EACP;IAAK\""),
        "got: {map}"
    );
}

// `@import` passthrough mapping (a plain `url(...)` import that dart-sass
// does not resolve as a partial, so it survives untranslated in the CSS
// output). Ground truth: `npx sass@1.97.3 --source-map` on
// `@import url(theme.css);\n` -> mappings "AAAQ" (dart maps the URL token
// position within the statement, not the `@import` keyword itself).
#[test]
fn import_passthrough_maps_to_url_token() {
    let input = "@import url(theme.css);\n";
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("in.scss"), input).unwrap();

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(["in.scss", "out.css"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let map = std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap();
    assert!(map.contains("\"mappings\":\"AAAQ\""), "got: {map}");
}

// ---- Declaration value provenance (todo #225 / Plan 113) ----
//
// dart-sass emits a SECOND mapping segment for every declaration value
// (`valueSpanForMap` in its serializer): for a bare `$var` value it points
// at the variable's stored declaration-value span (the environment keeps
// each variable's assigned expression node), for anything else at the value
// expression's own span. `SourceMapBuffer._addEntry`'s same-line dedup
// (drop an entry whose source line AND generated line both equal the
// previous entry's) is what keeps same-line literal/arithmetic values
// invisible. Ground truth for every test below: `npx sass@1.101.0 in.scss
// out.css` on the same fixture, mappings compared byte-for-byte (decoded
// positions transcribed into each test's comment).
//
// Known deferred gap (NOT tested here): dart also stores nodes for
// mixin/function ARGUMENT bindings, so `@mixin m($x) { b: $x; }` maps the
// value to the call-site argument expression ("@include m(c)" -> the `c`).
// grass has no per-argument span plumbing yet, so that fixture emits no
// second segment (the fallback `$x` span dedups away against the property).

/// Runs the CLI on `files` (written into one tempdir), compiling
/// `main.scss` -> `out.css`, and returns the raw `.map` JSON.
fn compile_to_map(files: &[(&str, &str)], extra_args: &[&str]) -> String {
    let tmp = tempfile::tempdir().unwrap();
    for (name, contents) in files {
        std::fs::write(tmp.path().join(name), contents).unwrap();
    }

    let mut args: Vec<&str> = extra_args.to_vec();
    args.push("main.scss");
    args.push("out.css");

    let output = grass_cmd()
        .current_dir(tmp.path())
        .args(&args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    std::fs::read_to_string(tmp.path().join("out.css.map")).unwrap()
}

// `$v: c;\na {\n  b: $v;\n}` -> "AACA;EACE,GAFE": the anchor fixture. The
// second segment maps generated 1:5 (after `b: `) to source 0:4 -- the `c`
// in `$v: c;`, i.e. the declaration VALUE's span, not the `$v` name.
#[test]
fn bare_variable_value_maps_to_declaration_site() {
    let map = compile_to_map(&[("main.scss", "$v: c;\na {\n  b: $v;\n}\n")], &[]);
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GAFE\""),
        "got: {map}"
    );
}

// Literal / arithmetic / function-call values emit no visible second
// segment: their value span is on the same source line as the property, so
// the same-line dedup drops it (mappings identical to pre-provenance).
#[test]
fn same_line_literal_values_emit_no_second_segment() {
    let map = compile_to_map(
        &[(
            "main.scss",
            "$v: c;\na {\n  b: c;\n  d: 1px + 2px;\n  e: min(1px, 2px);\n}\n",
        )],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EACE;EACA;EACA\""),
        "got: {map}"
    );
}

// A NON-variable value on its own source line does get a second segment
// (`a {\n  b:\n    c;\n}` -> value segment at source 2:4): dart maps every
// declaration value; the dedup only hides the same-line ones.
#[test]
fn value_on_own_line_gets_second_segment() {
    let map = compile_to_map(&[("main.scss", "a {\n  b:\n    c;\n}\n")], &[]);
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE,GACE\""),
        "got: {map}"
    );
}

// Reassignment: the LAST assignment's value span wins (dart stores the
// node at assignment time). `$v: c;\n$v: d;` -> segment points at the `d`.
#[test]
fn reassigned_variable_maps_to_last_assignment() {
    let map = compile_to_map(&[("main.scss", "$v: c;\n$v: d;\na {\n  b: $v;\n}\n")], &[]);
    assert!(
        map.contains("\"mappings\":\"AAEA;EACE,GAFE\""),
        "got: {map}"
    );
}

// `!default` on an already-set variable keeps the FIRST declaration's span
// (the guarded assignment never runs, so the stored node is untouched).
#[test]
fn guarded_reassignment_keeps_first_declaration_span() {
    let map = compile_to_map(
        &[("main.scss", "$v: c;\n$v: d !default;\na {\n  b: $v;\n}\n")],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AAEA;EACE,GAHE\""),
        "got: {map}"
    );
}

// `!default` on an unset variable stores its own value span like a plain
// declaration.
#[test]
fn guarded_first_declaration_stores_own_span() {
    let map = compile_to_map(&[("main.scss", "$v: d !default;\na {\n  b: $v;\n}\n")], &[]);
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GAFE\""),
        "got: {map}"
    );
}

// Nested scopes: a local shadow wins -- the segment points at the inner
// `$v: d;` (source 2:6), not the global declaration.
#[test]
fn shadowing_local_variable_maps_to_inner_declaration() {
    let map = compile_to_map(
        &[("main.scss", "$v: c;\na {\n  $v: d;\n  b: $v;\n}\n")],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EAEE,GADI\""),
        "got: {map}"
    );
}

// Variable chains collapse at assignment time: `$w: $v;` stores $v's
// already-stored node, so `b: $w` maps all the way back to the original
// `c` at source 0:4 (verified against dart, which does the same in
// `setVariable(..., _expressionNode(node.expression))`).
#[test]
fn variable_chain_collapses_to_original_declaration() {
    let map = compile_to_map(
        &[("main.scss", "$v: c;\na {\n  $w: $v;\n  b: $w;\n}\n")],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EAEE,GAHE\""),
        "got: {map}"
    );
}

// Multi-file `@use`: the provenance segment reaches into the used module
// (`_vars.scss` 0:4) -- and this is the ONLY way a no-CSS-output module
// enters the `sources` array at all, fixing that divergence too.
#[test]
fn use_module_variable_maps_into_module_and_adds_source() {
    let map = compile_to_map(
        &[
            ("main.scss", "@use \"vars\";\na {\n  b: vars.$v;\n}\n"),
            ("_vars.scss", "$v: c;\n"),
        ],
        &[],
    );
    assert!(
        map.contains("\"sources\":[\"main.scss\",\"_vars.scss\"]"),
        "got: {map}"
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GCFE\""),
        "got: {map}"
    );
}

// `@use ... as *` resolves through global modules the same way.
#[test]
fn use_as_star_variable_maps_into_module() {
    let map = compile_to_map(
        &[
            ("main.scss", "@use \"vars\" as *;\na {\n  b: $v;\n}\n"),
            ("_vars.scss", "$v: c;\n"),
        ],
        &[],
    );
    assert!(
        map.contains("\"sources\":[\"main.scss\",\"_vars.scss\"]"),
        "got: {map}"
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GCFE\""),
        "got: {map}"
    );
}

// Namespaced reassignment (`vars.$v: q;`) updates the stored span through
// the module, so the segment points at the `q` in the consumer file.
#[test]
fn namespaced_reassignment_updates_module_span() {
    let map = compile_to_map(
        &[
            (
                "main.scss",
                "@use \"vars\";\nvars.$v: q;\na {\n  b: vars.$v;\n}\n",
            ),
            ("_vars.scss", "$v: c;\n"),
        ],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AAEA;EACE,GAFO\""),
        "got: {map}"
    );
}

// `@use ... with ($v: x)` stores the configured expression's span (the `x`
// inside the with-clause, source 0:21), like dart's ConfiguredValue
// assignmentNode -- distinct from the error-reporting configuration span.
#[test]
fn configured_variable_maps_to_with_clause_expression() {
    let map = compile_to_map(
        &[
            (
                "main.scss",
                "@use \"cfg\" with ($v: x);\na {\n  b: cfg.$v;\n}\n",
            ),
            ("_cfg.scss", "$v: c !default;\n"),
        ],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GAFmB\""),
        "got: {map}"
    );
}

// Builtin module variables have no declaration site: no second segment
// (dart's builtin modules have no variableNodes; the fallback span dedups
// away against the property's own line).
#[test]
fn builtin_module_variable_emits_no_second_segment() {
    let map = compile_to_map(
        &[("main.scss", "@use \"sass:math\";\na {\n  b: math.$pi;\n}\n")],
        &[],
    );
    assert!(map.contains("\"mappings\":\"AACA;EACE\""), "got: {map}");
}

// Two uses of the same variable each get their own segment (different
// generated lines, so the dedup keeps both), both pointing at the same
// declaration site.
#[test]
fn repeated_variable_use_maps_each_declaration() {
    let map = compile_to_map(
        &[("main.scss", "$v: c;\na {\n  b: $v;\n  d: $v;\n}\n")],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACA;EACE,GAFE;EAGF,GAHE\""),
        "got: {map}"
    );
}

// Same-line declaration and use: `$v: c; a { b: $v; }` on ONE source line.
// The candidate second segment (source line 0) follows the property entry
// (also source line 0, same generated line 1) -> dropped by the dedup,
// exactly like dart.
#[test]
fn same_line_declaration_and_use_dedups_second_segment() {
    let map = compile_to_map(&[("main.scss", "$v: c; a { b: $v; }\n")], &[]);
    assert!(map.contains("\"mappings\":\"AAAO;EAAI\""), "got: {map}");
}

// `@each` binds its loop variables with the LIST expression as their node:
// `b: $i` maps to `c, d` at source 0:12 in both unrolled copies.
#[test]
fn each_variable_maps_to_list_expression() {
    let map = compile_to_map(
        &[(
            "main.scss",
            "@each $i in c, d {\n  a {\n    b: $i;\n  }\n}\n",
        )],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACE;EACE,GAFQ;;;AACV;EACE,GAFQ\""),
        "got: {map}"
    );
}

// `@for` binds its loop variable with the FROM expression as its node:
// `b: $i` maps to the `1` after "from" (source 0:13).
#[test]
fn for_variable_maps_to_from_expression() {
    let map = compile_to_map(
        &[(
            "main.scss",
            "@for $i from 1 through 1 {\n  a {\n    b: $i;\n  }\n}\n",
        )],
        &[],
    );
    assert!(
        map.contains("\"mappings\":\"AACE;EACE,GAFS\""),
        "got: {map}"
    );
}

// Compressed output exercises the dedup GLOBALLY (everything shares
// generated line 0): entries survive only while their source lines keep
// changing, byte-identical to dart's compressed map for the same fixture.
#[test]
fn compressed_output_dedups_by_line_like_dart() {
    let map = compile_to_map(
        &[(
            "main.scss",
            "$v: c;\na {\n  b: $v;\n  d: e;\n}\nf {\n  g: h;\n}\n",
        )],
        &["--style", "compressed"],
    );
    assert!(
        map.contains("\"mappings\":\"AACA,EACE,EAFE,EAGF,IAEF,EACE\""),
        "got: {map}"
    );
}
