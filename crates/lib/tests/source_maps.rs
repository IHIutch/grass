//! Tests for the Plan 013 design-spike source-map prototype
//! (`grass::from_string_with_source_map`). See `docs/design/source-maps.md`.

const INPUT: &str = "a {\n  b: c;\n}\n";

#[test]
fn option_off_is_byte_identical_to_from_string() {
    let plain = grass::from_string(INPUT.to_string(), &grass::Options::default())
        .expect("from_string should succeed");

    let (with_map_api, map) =
        grass::from_string_with_source_map(INPUT.to_string(), &grass::Options::default())
            .expect("from_string_with_source_map should succeed");

    assert_eq!(plain, with_map_api);
    assert!(
        map.is_none(),
        "map must be None when source_map option is off"
    );
}

#[test]
fn option_on_emits_valid_v3_json() {
    let options = grass::Options::default().source_map(true);
    let (_css, map) = grass::from_string_with_source_map(INPUT.to_string(), &options)
        .expect("from_string_with_source_map should succeed");

    let map = map.expect("map must be Some when source_map option is on");
    let map = map.to_json(None, false);

    // Hand-parse the handful of top-level fields rather than pulling in a
    // JSON dependency for a single test file.
    assert!(map.starts_with('{') && map.ends_with('}'));
    assert!(map.contains("\"version\":3"));
    // `from_string_with_source_map` has no real input path, so — matching
    // the JS API's `compileString` without a `url` option — the sole
    // `sources` entry is a `data:` URL of the input, not a literal "stdin".
    assert!(
        map.contains("\"sources\":[\"data:;charset=utf-8,"),
        "got: {map}"
    );
    assert!(map.contains("\"names\":[]"));
    assert!(
        !map.contains("\"mappings\":\"\""),
        "mappings must be non-empty"
    );
    assert!(
        !map.contains("\"file\""),
        "file key must be omitted, got: {map}"
    );
}

#[test]
fn first_mapping_matches_hand_computed_dart_sass_output() {
    // INPUT is `a {\n  b: c;\n}\n`. dart-sass 1.97.3 (observed via
    // `npx sass@1.97.3 --stdin` is data-URL sourced, so we instead reproduce
    // the file-based fixture from `npx sass@1.97.3 in.scss out.css` with the
    // same `a {\n  b: c;\n}\n` contents) produces `"mappings":"AAAA;EACE"`:
    //
    //   - group 1 ("AAAA"): line 0 (`a {`) maps output col 0 -> source
    //     file 0, line 0, col 0 (the `a` of the selector).
    //   - group 2 ("EACE"): line 1 (`  b: c;`) maps output col 2 -> source
    //     line 1, col 2 (the `b` of the declaration, after 2-space indent).
    //
    // Our serializer only maps selectors and declarations (this spike's
    // scope), so it should produce the identical mappings string byte for
    // byte for this single-rule fixture.
    let options = grass::Options::default().source_map(true);
    let (_css, map) = grass::from_string_with_source_map(INPUT.to_string(), &options)
        .expect("from_string_with_source_map should succeed");
    let map = map.expect("map must be Some when source_map option is on");
    let map = map.to_json(None, false);

    assert!(
        map.contains("\"mappings\":\"AAAA;EACE\""),
        "expected mappings \"AAAA;EACE\" (verified against dart-sass 1.97.3), got: {map}"
    );
}

#[test]
fn dependency_tracking_collects_variable_only_imports_without_mappings() {
    let root = tempfile::tempdir().unwrap();
    let entry = root.path().join("in.scss");
    let partial = root.path().join("_vars.scss");
    std::fs::write(&partial, "$value: 1;\n").unwrap();
    std::fs::write(&entry, "@use \"vars\";\na { b: vars.$value; }\n").unwrap();

    let options = grass::Options::default()
        .load_path(root.path())
        .dependency_tracking(true);
    let (css, loaded_files) = grass::from_path_with_loaded_files(&entry, &options)
        .expect("dependency-only compilation should succeed");
    let plain = grass::from_path(&entry, &grass::Options::default().load_path(root.path()))
        .expect("plain compilation should succeed");

    assert_eq!(css, plain);
    assert!(loaded_files.iter().any(|path| path.ends_with("_vars.scss")));

    let (_, map) = grass::from_path_with_source_map(&entry, &options)
        .expect("source-map wrapper should still compile");
    assert!(
        map.is_none(),
        "dependency tracking must not enable mappings"
    );
}
