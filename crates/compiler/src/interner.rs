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

/// Keys index a **thread-local** interning table (see `STRINGS` above).
/// Ideally this type would be `!Send`/`!Sync` to make cross-thread misuse a
/// compile error: moving a key to another thread resolves it against that
/// thread's own (unrelated) table, silently yielding the wrong string or
/// panicking. In practice `Unit::Unknown(InternedString)` is stored in
/// `static LazyLock<...>` conversion tables (`crates/compiler/src/unit/conversion.rs`)
/// that require `Send + Sync`, so a `!Send`/`!Sync` marker is not viable
/// without restructuring those tables. Treat this doc comment as the
/// enforcement instead: do not send an `InternedString` (or any type
/// containing one) across threads and then resolve it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct InternedString(Spur);

impl InternedString {
    pub fn get_or_intern<T: AsRef<str>>(s: T) -> Self {
        STRINGS.with(|cell| Self(cell.borrow_mut().get_or_intern(s)))
    }

    /// Resolve without copying.
    ///
    /// SAFETY invariants (why the returned reference is usable):
    /// - lasso's `Rodeo` stores strings in stable arena memory that is never
    ///   moved or freed while the `Rodeo` lives;
    /// - the `Rodeo` lives in a `thread_local!`, destroyed only at thread exit;
    /// - in practice, one compilation runs on one thread and does not stash
    ///   this key or the resolved `&str` past that thread's lifetime (see the
    ///   type-level doc comment above: this is a convention, not a
    ///   compiler-enforced guarantee, since `InternedString` cannot be made
    ///   `!Send` without breaking the `Unit` conversion tables). Callers must
    ///   not stash the result in a `static`/leaked struct that outlives the
    ///   thread, nor move an `InternedString` to another thread and resolve
    ///   it there.
    pub fn resolve_ref(self) -> &'static str {
        STRINGS.with(|cell| unsafe { &*(cell.borrow().resolve(&self.0) as *const str) })
    }
}

impl Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        STRINGS.with(|cell| write!(f, "{}", cell.borrow().resolve(&self.0)))
    }
}
