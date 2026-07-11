use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicIsize, Ordering},
};

struct CountingAllocator;

static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE_BYTES.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            LIVE_BYTES.fetch_add(new_size as isize - layout.size() as isize, Ordering::Relaxed);
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

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
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "virtual probe file is missing"))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

const USE_SOURCE: &str = "@use \"dep\" as dep;\na { value: dep.$value; }";
const IMPORT_SOURCE: &str = "@import \"dep\";\na { value: $value; }";
const PLAIN_SOURCE: &str = "a { value: 1px; }";

fn virtual_fs(file_name: &str) -> VirtualFs {
    let mut files = BTreeMap::new();
    files.insert(
        PathBuf::from(format!("/virtual/{file_name}")),
        b"$value: 1px;".to_vec(),
    );
    VirtualFs { files }
}

fn compile_original(iteration: usize, use_rule: bool, partial: bool, unique_name: bool) {
    let file_name = if partial { "_dep.scss" } else { "dep.scss" };
    let source = if use_rule { USE_SOURCE } else { IMPORT_SOURCE };
    let fs = virtual_fs(file_name);
    let options = grass::Options::default().fs(&fs).load_path("/virtual");
    let entry = if unique_name {
        format!("/virtual/entry-{iteration}.scss")
    } else {
        "/virtual/entry.scss".to_owned()
    };
    if unique_name {
        let _ = grass::from_string_with_url_and_source_map(source.to_owned(), &entry, &options);
    } else {
        let _ = grass::from_string(source.to_owned(), &options);
    }
}

fn compile_use_variant(source: &str, load_path: bool, emit_css: bool) {
    let fs = virtual_fs("dep.scss");
    let options = if load_path {
        grass::Options::default().fs(&fs).load_path("/virtual")
    } else {
        grass::Options::default().fs(&fs)
    };
    let source = if emit_css {
        source
    } else {
        "@use \"dep\" as dep;"
    };
    let _ = grass::from_string(source.to_owned(), &options);
}

fn compile_bare() {
    let _ = grass::from_string(PLAIN_SOURCE.to_owned(), &grass::Options::default());
}

fn compile_fs_only() {
    let fs = virtual_fs("dep.scss");
    let options = grass::Options::default().fs(&fs);
    let _ = grass::from_string(PLAIN_SOURCE.to_owned(), &options);
}

fn compile_load_path_only() {
    let options = grass::Options::default().load_path("/virtual");
    let _ = grass::from_string(PLAIN_SOURCE.to_owned(), &options);
}

fn compile_builtin_use() {
    let _ = grass::from_string(
        "@use \"sass:math\"; a { value: 1px; }".to_owned(),
        &grass::Options::default(),
    );
}

fn run_case(name: &str, mut compile: impl FnMut(usize)) -> isize {
    for iteration in 0..100 {
        compile(iteration);
    }
    let before = LIVE_BYTES.load(Ordering::Relaxed);
    for iteration in 0..10_000 {
        compile(iteration);
    }
    let after = LIVE_BYTES.load(Ordering::Relaxed);
    let delta = after - before;
    let per_compile = delta as f64 / 10_000.0;
    println!("{name}: delta={delta} bytes, per_compile={per_compile:.3}");
    delta
}

fn main() {
    run_case("original_use_same_explicit", |iteration| {
        compile_original(iteration, true, false, false)
    });
    run_case("bare_from_string", |_| compile_bare());
    run_case("fs_only", |_| compile_fs_only());
    run_case("load_path_only", |_| compile_load_path_only());
    run_case("builtin_use", |_| compile_builtin_use());
    run_case("original_import_same_explicit", |iteration| {
        compile_original(iteration, false, false, false)
    });
    run_case("original_use_same_partial", |iteration| {
        compile_original(iteration, true, true, false)
    });
    run_case("original_import_same_partial", |iteration| {
        compile_original(iteration, false, true, false)
    });
    run_case("original_use_unique_explicit", |iteration| {
        compile_original(iteration, true, false, true)
    });
    run_case("original_import_unique_explicit", |iteration| {
        compile_original(iteration, false, false, true)
    });
    run_case("use_direct_path_no_load_path", |_| {
        compile_use_variant(
            "@use \"/virtual/dep.scss\" as dep; a { value: dep.$value; }",
            false,
            true,
        )
    });
    run_case("use_load_path_no_css", |_| {
        compile_use_variant("@use \"dep\" as dep;", true, false)
    });
}
