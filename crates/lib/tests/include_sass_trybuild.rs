#![cfg(feature = "macro")]

// Snapshot the compile error for invalid Sass through the `include_sass!`
// proc macro (see `tests/trybuild/invalid.rs` + its `.stderr` snapshot).
//
// Exercises the `FileTracker` include_str! path for a multi-file `@import`
// too: `tests/trybuild/multi/main.scss` imports `tests/trybuild/multi/
// _partial.scss`. The importing `.rs` fixture is generated at test-run time
// (not checked in) with an *absolute* path baked in, rather than checking in
// a fixture using a relative literal — trybuild compiles fixtures from a
// synthetic project under `target/tests/trybuild/<crate>/`, so a relative
// literal would need several leading `../..` to reach back to this crate's
// `tests/` dir, which trips a real (separately filed) bug in the compiler's
// `normalize_path` where consecutive leading `..` components pairwise-cancel
// instead of accumulating. `invalid.rs` avoids this because compiling it
// only ever *reads* one file directly (no `@import` resolution, so
// `normalize_path` is never invoked); `multi_file.rs` would hit it, so its
// fixture uses an absolute path instead, generated here.
#[test]
fn trybuild() {
    let multi_main = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/trybuild/multi/main.scss");
    let generated_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/trybuild/multi_file_generated.rs");
    std::fs::write(
        generated_path,
        format!(
            "fn main() {{\n    let css: &str = grass::include!({multi_main:?});\n    assert_eq!(css, \"a{{color:red}}\");\n}}\n"
        ),
    )
    .unwrap();

    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/invalid.rs");
    t.pass("tests/trybuild/multi_file_generated.rs");
}
