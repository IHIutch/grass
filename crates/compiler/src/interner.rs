use lasso::{Spur, ThreadedRodeo};

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::sync::LazyLock;

// Global interner storage. Uses ThreadedRodeo for thread-safe interning.
// Cross-thread InternedString compatibility is required for parallel module
// evaluation: a Spur interned on one thread must resolve correctly on another.
static STRINGS: LazyLock<ThreadedRodeo<Spur>> = LazyLock::new(ThreadedRodeo::default);

// Thread-local cache for both interning and resolution.
// Avoids DashMap overhead on the hot paths.
thread_local! {
    static LOCAL_CACHE: UnsafeCell<LocalCache> = UnsafeCell::new(LocalCache::new());
}

struct LocalCache {
    /// Fast-path for get_or_intern: maps string content → Spur.
    /// Avoids DashMap lookup for previously-seen strings on this thread.
    intern_map: HashMap<&'static str, Spur>,
    /// Fast-path for resolve: maps Spur index → &'static str.
    /// Direct array access, no hashing needed.
    resolve_ptrs: Vec<*const u8>,
    resolve_lens: Vec<usize>,
}

impl LocalCache {
    fn new() -> Self {
        Self {
            intern_map: HashMap::new(),
            resolve_ptrs: Vec::new(),
            resolve_lens: Vec::new(),
        }
    }

    #[inline]
    fn resolve(&self, idx: usize) -> Option<&'static str> {
        if idx < self.resolve_ptrs.len() {
            let ptr = self.resolve_ptrs[idx];
            if !ptr.is_null() {
                let len = self.resolve_lens[idx];
                // SAFETY: ptr came from ThreadedRodeo's arena (static lifetime).
                return Some(unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
                });
            }
        }
        None
    }

    #[inline]
    fn cache_resolved(&mut self, idx: usize, s: &'static str) {
        if idx >= self.resolve_ptrs.len() {
            self.resolve_ptrs.resize(idx + 1, std::ptr::null());
            self.resolve_lens.resize(idx + 1, 0);
        }
        self.resolve_ptrs[idx] = s.as_ptr();
        self.resolve_lens[idx] = s.len();
    }
}

/// Resolve a Spur from the global ThreadedRodeo and return a 'static reference.
#[inline]
fn resolve_static(key: Spur) -> &'static str {
    // SAFETY: ThreadedRodeo stores strings in stable arena memory that lives
    // for the program's lifetime (static LazyLock). The arena memory is never
    // deallocated or moved.
    let resolved = STRINGS.resolve(&key);
    unsafe { &*(resolved as *const str) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Spur);

impl InternedString {
    pub fn get_or_intern<T: AsRef<str>>(s: T) -> Self {
        LOCAL_CACHE.with(|cell| {
            // SAFETY: thread-local, no concurrent access
            let cache = unsafe { &mut *cell.get() };
            let s_ref = s.as_ref();

            // Fast path: check if already interned locally
            if let Some(&key) = cache.intern_map.get(s_ref) {
                return Self(key);
            }

            // Slow path: intern in global ThreadedRodeo
            let key = STRINGS.get_or_intern(s_ref);
            let idx = lasso::Key::into_usize(key);

            // Cache the resolution
            let stable = resolve_static(key);
            cache.cache_resolved(idx, stable);
            cache.intern_map.insert(stable, key);

            Self(key)
        })
    }

    #[allow(dead_code)]
    pub fn resolve(self) -> String {
        self.resolve_ref().to_owned()
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.resolve_ref() == ""
    }

    pub fn resolve_ref<'a>(self) -> &'a str {
        LOCAL_CACHE.with(|cell| {
            // SAFETY: thread-local, no concurrent access
            let cache = unsafe { &mut *cell.get() };
            let idx = lasso::Key::into_usize(self.0);
            if let Some(s) = cache.resolve(idx) {
                return s;
            }
            // Cache miss — resolve from global and cache locally
            let stable = resolve_static(self.0);
            cache.cache_resolved(idx, stable);
            stable
        })
    }
}

impl Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.resolve_ref())
    }
}
