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
    assert!(map.contains(&expected_source), "got: {map}\nwant substring: {expected_source}");
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
        .args(["--no-source-map", "--embed-source-map", "in.scss", "out.css"])
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
    assert!(stdout.contains("file:"), "expected an absolute file: URL, got: {stdout}");
    assert!(!stdout.contains("%22file%22"), "file key must be omitted, got: {stdout}");
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
    assert!(
        map.contains("\"mappings\":\"AAAA;EACE\""),
        "got: {map}"
    );
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
    assert!(map.contains("\"mappings\":\";AAAA;AACE;EACA\""), "got: {map}");
}
