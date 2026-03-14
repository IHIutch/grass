use lasso::{Rodeo, Spur};

use std::fmt::{self, Display};
use std::sync::Mutex;

/// Global interner storage. Uses Mutex instead of thread_local! to eliminate
/// TLS lookup overhead (_tlv_get_addr was 6.2% of profiled self-time on macOS).
/// The mutex is always uncontended in production (single-threaded compilation),
/// so the cost is just a single atomic CAS — faster than macOS TLS descriptor lookup.
static STRINGS: Mutex<Option<Rodeo<Spur>>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Spur);

impl InternedString {
    pub fn get_or_intern<T: AsRef<str>>(s: T) -> Self {
        let mut guard = STRINGS.lock().unwrap();
        Self(guard.get_or_insert_with(Rodeo::default).get_or_intern(s))
    }

    #[allow(dead_code)]
    pub fn resolve(self) -> String {
        STRINGS.lock().unwrap().as_ref().unwrap().resolve(&self.0).to_owned()
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.resolve_ref() == ""
    }

    pub fn resolve_ref<'a>(self) -> &'a str {
        // SAFETY: Rodeo stores interned strings in stable arena memory that is
        // never deallocated or moved. The Rodeo lives in a static, so the strings
        // live for the duration of the program. The Mutex ensures no concurrent
        // mutation while we take the pointer.
        let guard = STRINGS.lock().unwrap();
        unsafe { &*(guard.as_ref().unwrap().resolve(&self.0) as *const str) }
    }
}

impl Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = STRINGS.lock().unwrap();
        write!(f, "{}", guard.as_ref().unwrap().resolve(&self.0))
    }
}
