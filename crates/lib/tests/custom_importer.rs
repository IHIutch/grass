use std::{cell::Cell, io::Write, path::PathBuf, rc::Rc};

use grass::{ImportResolution, Importer, InputSyntax, Options, Result as SassResult};
use grass_compiler::codemap::Span;

#[macro_use]
mod macros;

/// An importer that intercepts exactly one virtual URL, delegating it to a
/// real file on disk (proving the `FileImporter`-style `DelegateToPath`
/// path reuses the existing partial/extension-resolution machinery), and
/// declines everything else so normal resolution can take over.
#[derive(Debug)]
struct VirtualImporter {
    virtual_url: &'static str,
    target: PathBuf,
}

impl Importer for VirtualImporter {
    fn canonicalize(
        &self,
        url: &str,
        _from_import: bool,
        _containing_url: Option<&str>,
        _span: Span,
    ) -> SassResult<ImportResolution> {
        if url == self.virtual_url {
            Ok(ImportResolution::DelegateToPath(self.target.clone()))
        } else {
            Ok(ImportResolution::NotFound)
        }
    }
}

#[test]
fn custom_importer_delegates_to_path() {
    tempfile!(
        "custom_importer_delegates_to_path__target.scss",
        "$a: red;",
        dir = "dir-custom_importer_delegates_to_path"
    );

    let options = Options::default().add_importer(Rc::new(VirtualImporter {
        virtual_url: "virtual:thing",
        target: PathBuf::from(
            "dir-custom_importer_delegates_to_path/custom_importer_delegates_to_path__target",
        ),
    }));

    let css = grass::from_string(
        "@import \"virtual:thing\";\na {\n  color: $a;\n}".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  color: red;\n}\n");
}

#[test]
fn custom_importer_not_found_falls_through_to_default_resolution() {
    tempfile!(
        "custom_importer_not_found__real.scss",
        "$b: blue;",
        dir = "dir-custom_importer_not_found"
    );

    // Registered, but never matches the URL below — every call must return
    // `NotFound` so the normal load-path/filesystem resolution still runs
    // uninterrupted underneath it.
    let options = Options::default()
        .add_importer(Rc::new(VirtualImporter {
            virtual_url: "virtual:unrelated",
            target: PathBuf::from("does-not-matter"),
        }))
        .load_path(std::path::Path::new("dir-custom_importer_not_found"));

    let css = grass::from_string(
        "@import \"custom_importer_not_found__real\";\na {\n  color: $b;\n}".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  color: blue;\n}\n");
}

#[test]
fn custom_importer_takes_priority_over_filesystem() {
    // A real file exists at the relative path a normal `@import` would
    // resolve to, but the importer intercepts that exact URL first and
    // redirects to a different file — proving importers are checked ahead
    // of the default filesystem resolution, not just as a fallback.
    tempfile!(
        "custom_importer_priority__decoy.scss",
        "$c: green;",
        dir = "dir-custom_importer_priority"
    );
    tempfile!(
        "custom_importer_priority__real_target.scss",
        "$c: purple;",
        dir = "dir-custom_importer_priority"
    );

    let options = Options::default()
        .add_importer(Rc::new(VirtualImporter {
            virtual_url: "custom_importer_priority__decoy",
            target: PathBuf::from(
                "dir-custom_importer_priority/custom_importer_priority__real_target",
            ),
        }))
        .load_path(std::path::Path::new("dir-custom_importer_priority"));

    let css = grass::from_string(
        "@import \"custom_importer_priority__decoy\";\na {\n  color: $c;\n}".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  color: purple;\n}\n");
}

/// An importer backing a full JS `Importer` (`canonicalize`+`load`,
/// combined into one Rust call): resolves `virtual:colors` to inline
/// `contents` with no filesystem involvement at all, via
/// `ImportResolution::Resolved`. `calls` counts every `canonicalize`
/// invocation and is baked into the returned contents (`$primary: <n>px`),
/// which lets tests observe whether a *second* resolution's contents were
/// actually used (no caching) or discarded in favor of an already-cached
/// module (caching, per the JS API's "same canonical URL -> same cached
/// module" correctness requirement).
#[derive(Debug)]
struct ResolvedImporter {
    calls: Cell<u32>,
}

impl Importer for ResolvedImporter {
    fn canonicalize(
        &self,
        url: &str,
        _from_import: bool,
        _containing_url: Option<&str>,
        _span: Span,
    ) -> SassResult<ImportResolution> {
        if url == "virtual:colors" {
            let n = self.calls.get() + 1;
            self.calls.set(n);
            Ok(ImportResolution::Resolved {
                canonical_url: "virtual:colors".to_owned(),
                contents: format!("$primary: {n}px;"),
                syntax: InputSyntax::Scss,
            })
        } else {
            Ok(ImportResolution::NotFound)
        }
    }
}

#[test]
fn custom_importer_resolves_inline_contents_no_filesystem() {
    let importer = Rc::new(ResolvedImporter {
        calls: Cell::new(0),
    });

    let options = Options::default().add_importer(Rc::clone(&importer) as Rc<dyn Importer>);

    let css = grass::from_string(
        "@import \"virtual:colors\";\na {\n  color: $primary;\n}".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(css, "a {\n  color: 1px;\n}\n");
    assert_eq!(importer.calls.get(), 1);
}

#[test]
fn custom_importer_resolved_hits_cache_across_contexts() {
    tempfile!(
        "custom_importer_resolved_cache__other.scss",
        "@import \"virtual:colors\";\nb {\n  color: $primary;\n}",
        dir = "dir-custom_importer_resolved_cache"
    );

    let importer = Rc::new(ResolvedImporter {
        calls: Cell::new(0),
    });

    let options = Options::default()
        .add_importer(Rc::clone(&importer) as Rc<dyn Importer>)
        .load_path(std::path::Path::new("dir-custom_importer_resolved_cache"));

    // Two distinct resolution contexts (this file, and
    // `custom_importer_resolved_cache__other`'s own `@import`) both resolve
    // `virtual:colors` -- `canonicalize` legitimately runs twice (once per
    // context), but the *module* must only be parsed/instantiated once: if
    // caching were broken, `b`'s `$primary` would pick up the second,
    // incremented `canonicalize` call's contents (2px) instead of reusing
    // the first-resolved, cached module (1px).
    let css = grass::from_string(
        "@import \"virtual:colors\";\n@import \"custom_importer_resolved_cache__other\";\na {\n  color: $primary;\n}".to_owned(),
        &options,
    )
    .unwrap();

    assert_eq!(
        css,
        "b {\n  color: 1px;\n}\n\na {\n  color: 1px;\n}\n"
    );
    assert_eq!(importer.calls.get(), 2);
}
