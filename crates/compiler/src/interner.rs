use lasso::{Rodeo, Spur};

use std::cell::RefCell;
use std::fmt::{self, Display};

// Global interner storage. Uses thread_local! to eliminate mutex overhead entirely.
// Each thread gets its own Rodeo, which is sound because cargo test runs tests on
// separate threads. In production (single-threaded compilation), there is exactly
// one Rodeo instance with zero synchronization cost.
thread_local! {
    static STRINGS: RefCell<Rodeo<Spur>> = RefCell::new(Rodeo::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Spur);

impl InternedString {
    pub fn get_or_intern<T: AsRef<str>>(s: T) -> Self {
        STRINGS.with(|cell| Self(cell.borrow_mut().get_or_intern(s)))
    }

    #[allow(dead_code)]
    pub fn resolve(self) -> String {
        STRINGS.with(|cell| cell.borrow().resolve(&self.0).to_owned())
    }

    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self.resolve_ref() == ""
    }

    pub fn resolve_ref<'a>(self) -> &'a str {
        // SAFETY: Rodeo stores interned strings in stable arena memory that is
        // never deallocated or moved. The thread_local lives for the thread's
        // lifetime. The RefCell borrow is short-lived but the underlying arena
        // memory is stable, so extending the lifetime is safe.
        STRINGS.with(|cell| unsafe { &*(cell.borrow().resolve(&self.0) as *const str) })
    }
}

impl Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        STRINGS.with(|cell| write!(f, "{}", cell.borrow().resolve(&self.0)))
    }
}
