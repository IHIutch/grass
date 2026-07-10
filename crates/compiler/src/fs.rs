use std::{
    ffi::OsStr,
    ffi::OsString,
    io::{self, Error, ErrorKind},
    path::{Path, PathBuf},
};

use rustc_hash::FxHashSet;

/// A directory listing snapshot used to batch multiple existence probes
/// (that would otherwise each be a separate `stat`/`getattrlist` call) into a
/// single directory read.
///
/// To avoid changing case-sensitivity or symlink-following semantics versus a
/// direct `is_file`/`is_dir` check, this only ever proves two things without
/// re-touching the filesystem:
/// - a name is DEFINITELY a plain (non-symlink) file/dir, because it was seen
///   as one, byte-exact, in the listing; or
/// - a name is DEFINITELY absent, because no case-insensitive variant of it
///   appears anywhere in the listing at all.
///
/// Anything else (a same-named symlink, a case-only variant match, or a
/// file/dir mismatch) is ambiguous and callers must fall back to a direct
/// filesystem check — exactly preserving today's behavior for those rarer
/// cases.
#[derive(Debug, Default)]
pub struct DirListing {
    plain_files: FxHashSet<OsString>,
    plain_dirs: FxHashSet<OsString>,
    all_names_lower: FxHashSet<String>,
}

/// The kind of a directory entry, abstracted away from `std::fs::FileType` so
/// non-native `Fs` implementations (e.g. a JS-backed bridge on wasm) can
/// build a [`DirListing`] from their own directory-listing primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// Symlinks and anything else that couldn't be classified.
    Other,
}

impl DirListing {
    /// Records one directory entry. See [`EntryKind::Other`] for what's
    /// deliberately excluded from `plain_files`/`plain_dirs`.
    pub fn insert(&mut self, name: OsString, kind: EntryKind) {
        self.all_names_lower
            .insert(name.to_string_lossy().to_lowercase());
        match kind {
            EntryKind::File => {
                self.plain_files.insert(name);
            }
            EntryKind::Dir => {
                self.plain_dirs.insert(name);
            }
            // symlinks (and anything we couldn't stat) are deliberately left
            // out of plain_files/plain_dirs so lookups for them fall back to
            // a direct filesystem check.
            EntryKind::Other => {}
        }
    }

    /// `Some(true/false)` if provable from the listing alone; `None` if the
    /// caller must fall back to a direct `is_file` check.
    pub fn probe_is_file(&self, name: &OsStr) -> Option<bool> {
        if self.plain_files.contains(name) {
            return Some(true);
        }
        if !self
            .all_names_lower
            .contains(&name.to_string_lossy().to_lowercase())
        {
            return Some(false);
        }
        None
    }

    /// `Some(true/false)` if provable from the listing alone; `None` if the
    /// caller must fall back to a direct `is_dir` check.
    pub fn probe_is_dir(&self, name: &OsStr) -> Option<bool> {
        if self.plain_dirs.contains(name) {
            return Some(true);
        }
        if !self
            .all_names_lower
            .contains(&name.to_string_lossy().to_lowercase())
        {
            return Some(false);
        }
        None
    }
}

/// A trait to allow replacing the file system lookup mechanisms.
///
/// As it stands, this is imperfect: it’s still using the types and some operations from
/// `std::path`, which constrain it to the target platform’s norms. This could be ameliorated by
/// the use of associated types for `Path` and `PathBuf`, and putting all remaining methods on this
/// trait (`is_absolute`, `parent`, `join`, *&c.*); but that would infect too many other APIs to be
/// desirable, so we live with it as it is—which is also acceptable, because the motivating example
/// use case is mostly using this as an optimisation over the real platform underneath.
pub trait Fs: std::fmt::Debug {
    /// Returns `true` if the path exists on disk and is pointing at a directory.
    fn is_dir(&self, path: &Path) -> bool;
    /// Returns `true` if the path exists on disk and is pointing at a regular file.
    fn is_file(&self, path: &Path) -> bool;
    /// Read the entire contents of a file into a bytes vector.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Canonicalize a file path
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    /// Given a list of candidate file paths, return the first one that exists.
    /// This allows batch resolution in a single call, reducing overhead for
    /// implementations that cross a boundary (e.g. WASM-JS).
    ///
    /// The default implementation falls back to per-path `is_file()` checks.
    fn resolve_first_existing(&self, candidates: &[PathBuf]) -> Option<PathBuf> {
        candidates.iter().find(|p| self.is_file(p)).cloned()
    }

    /// Returns a snapshot of `dir`'s entries, used to batch many
    /// existence-probing candidates that share the same parent directory
    /// into a single directory read instead of one filesystem call per
    /// candidate.
    ///
    /// Returns `None` if the directory can't be listed (doesn't exist, IO
    /// error) or if this implementation doesn't support batched listing —
    /// callers must fall back to per-candidate `is_file`/`is_dir` checks in
    /// that case, so this is purely an optional optimization.
    fn dir_listing(&self, _dir: &Path) -> Option<DirListing> {
        None
    }
}

/// Use [`std::fs`] to read any files from disk.
///
/// This is the default file system implementation.
#[derive(Debug)]
pub struct StdFs;

impl Fs for StdFs {
    #[inline]
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    #[inline]
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    #[inline]
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    #[inline]
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn dir_listing(&self, dir: &Path) -> Option<DirListing> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut listing = DirListing::default();
        for entry in entries.flatten() {
            let kind = match entry.file_type() {
                Ok(ft) if ft.is_file() => EntryKind::File,
                Ok(ft) if ft.is_dir() => EntryKind::Dir,
                _ => EntryKind::Other,
            };
            listing.insert(entry.file_name(), kind);
        }
        Some(listing)
    }
}

/// A file system implementation that acts like it’s completely empty.
///
/// This may be useful for security as it denies all access to the file system (so `@import` is
/// prevented from leaking anything); you’ll need to use [`from_string`][crate::from_string] for
/// this to make any sense (since [`from_path`][crate::from_path] would fail to find a file).
#[derive(Debug)]
pub struct NullFs;

impl Fs for NullFs {
    #[inline]
    fn is_file(&self, _path: &Path) -> bool {
        false
    }

    #[inline]
    fn is_dir(&self, _path: &Path) -> bool {
        false
    }

    #[inline]
    fn read(&self, _path: &Path) -> io::Result<Vec<u8>> {
        Err(Error::new(
            ErrorKind::NotFound,
            "NullFs, there is no file system",
        ))
    }
}
