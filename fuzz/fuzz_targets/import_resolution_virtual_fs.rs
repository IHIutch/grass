#![no_main]

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use libfuzzer_sys::fuzz_target;

#[derive(Debug)]
struct VirtualFs {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl grass::Fs for VirtualFs {
    fn is_dir(&self, path: &Path) -> bool {
        path == Path::new("/virtual")
            || self
                .files
                .keys()
                .any(|file| file.parent().is_some_and(|parent| parent == path))
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "virtual fuzz file is missing"))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

fuzz_target!(|data: &[u8]| {
    let value = data.first().copied().unwrap_or_default() % 32;
    let use_rule = data.get(1).is_none_or(|byte| byte & 1 == 0);
    let partial = data.get(2).is_some_and(|byte| byte & 1 == 0);
    let file_name = if partial { "_dep.scss" } else { "dep.scss" };

    let source = if use_rule {
        r#"@use "dep" as dep;
a { value: dep.$value; }"#
            .to_owned()
    } else {
        r#"@import "dep";
a { value: $value; }"#
            .to_owned()
    };
    let dependency = format!("$value: {value}px;");

    let mut files = BTreeMap::new();
    files.insert(
        PathBuf::from(format!("/virtual/{file_name}")),
        dependency.into_bytes(),
    );
    let fs = VirtualFs { files };
    let options = grass::Options::default().fs(&fs).load_path("/virtual");

    // Both successful compilation and ordinary Sass resolution/parser errors
    // are valid outcomes. The oracle is that arbitrary virtual filesystem and
    // import inputs never panic or escape the bounded fuzz invocation.
    let _ = grass::from_string(source, &options);
});
