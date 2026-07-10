#![cfg(feature = "macro")]

use std::path::Path;

// Snapshot the compile error for invalid Sass through the `include_sass!`
// proc macro (see `tests/trybuild/invalid.rs` + its `.stderr` snapshot).
//
// Exercises the `FileTracker` include_str! path for a multi-file `@import`
// too: `tests/trybuild/multi/main.scss` imports `tests/trybuild/multi/
// _partial.scss`. The importing `.rs` fixture is generated at test-run time
// (not checked in). trybuild compiles fixtures from a synthetic project
// under `<workspace>/target/tests/trybuild/<crate>/`, so a relative literal
// pointing back at this crate's `tests/` dir needs several leading `../..`
// components — which used to trip a real bug in the compiler's
// `normalize_path` where consecutive leading `..` components pairwise-cancel
// instead of accumulating (todo #270, fixed in
// `crates/compiler/src/evaluate/visitor.rs`). We generate *two* variants to
// cover both the previously-working-around case and the actual bug:
// - `multi_file_generated.rs` bakes in an absolute path (regression coverage
//   for the generator itself; also what `invalid.rs` effectively has, since
//   compiling it only ever *reads* one file directly with no `@import`
//   resolution, so `normalize_path` is never invoked there).
// - `multi_file_generated_relative.rs` uses the natural relative `../../../../`
//   literal that previously failed to resolve, proving the fix end-to-end.
#[test]
fn trybuild() {
    let multi_main = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/trybuild/multi/main.scss"
    );
    let generated_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/trybuild/multi_file_generated.rs"
    );
    std::fs::write(
        generated_path,
        format!(
            "fn main() {{\n    let css: &str = grass::include!({multi_main:?});\n    assert_eq!(css, \"a{{color:red}}\");\n}}\n"
        ),
    )
    .unwrap();

    // `<workspace_root>` == `CARGO_MANIFEST_DIR/../..` (crates/lib -> crates -> workspace root).
    // trybuild's synthetic project lives at `<workspace_root>/target/tests/trybuild/grass/`,
    // 4 directories below `workspace_root`, so climbing back out takes 4 leading `..`.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rel_from_workspace = Path::new("crates/lib/tests/trybuild/multi/main.scss");
    assert_eq!(
        workspace_root.join(rel_from_workspace),
        Path::new(multi_main),
        "rel_from_workspace must match multi_main's actual location"
    );
    let relative_main = format!("../../../../{}", rel_from_workspace.display());
    let generated_relative_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/trybuild/multi_file_generated_relative.rs"
    );
    std::fs::write(
        generated_relative_path,
        format!(
            "fn main() {{\n    let css: &str = grass::include!({relative_main:?});\n    assert_eq!(css, \"a{{color:red}}\");\n}}\n"
        ),
    )
    .unwrap();

    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/invalid.rs");
    t.pass("tests/trybuild/multi_file_generated.rs");
    t.pass("tests/trybuild/multi_file_generated_relative.rs");
}
