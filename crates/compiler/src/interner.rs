use lasso::{Rodeo, Spur};

use std::cell::UnsafeCell;
use std::fmt::{self, Display};

/// Global interner storage. Uses UnsafeCell instead of Mutex to eliminate
/// atomic CAS overhead on every intern/resolve call. grass is single-threaded
/// (Rc everywhere, no Send/Sync on core types), so concurrent access is
/// impossible in practice. Previous versions used Mutex (single atomic CAS per
/// call) and before that thread_local! (6.2% TLS descriptor overhead on macOS).
/// UnsafeCell avoids both costs entirely.
struct InternerStore(UnsafeCell<Option<Rodeo<Spur>>>);

/// SAFETY: grass is single-threaded. The interner is only accessed from the
/// compilation thread. No concurrent access is possible because core types
/// (Value, Scopes, etc.) are !Send + !Sync due to Rc usage.
unsafe impl Sync for InternerStore {}

static STRINGS: InternerStore = InternerStore(UnsafeCell::new(None));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Spur);

impl InternedString {
    pub fn get_or_intern<T: AsRef<str>>(s: T) -> Self {
        // SAFETY: single-threaded access (see InternerStore safety comment)
        let store = unsafe { &mut *STRINGS.0.get() };
        Self(store.get_or_insert_with(Rodeo::default).get_or_intern(s))
    }

    #[allow(dead_code)]
    pub fn resolve(self) -> String {
        // SAFETY: single-threaded access
        let store = unsafe { &*STRINGS.0.get() };
        store.as_ref().unwrap().resolve(&self.0).to_owned()
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.resolve_ref() == ""
    }

    pub fn resolve_ref<'a>(self) -> &'a str {
        // SAFETY: Rodeo stores interned strings in stable arena memory that is
        // never deallocated or moved. The Rodeo lives in a static, so the strings
        // live for the duration of the program. Single-threaded access guaranteed.
        let store = unsafe { &*STRINGS.0.get() };
        store.as_ref().unwrap().resolve(&self.0)
    }
}

impl Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: single-threaded access
        let store = unsafe { &*STRINGS.0.get() };
        write!(f, "{}", store.as_ref().unwrap().resolve(&self.0))
    }
}
