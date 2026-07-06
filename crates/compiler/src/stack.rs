//! Segmented-stack helper for recursive-descent chokepoints.
//!
//! Mirrors swc's use of the `stacker` crate at its parser entry point: instead of
//! sizing a fixed recursion-depth limit to the smallest stack we ever run on
//! (napi's 1MiB worker threads), grow the stack on demand when it's close to
//! exhausted. `stacker` doesn't support wasm32, so it sits behind a default-on
//! feature with a no-op passthrough fallback here, letting the wasm build (which
//! disables the feature, see crates/lib/Cargo.toml) compile without it.

#[cfg(feature = "stacker")]
#[inline]
pub(crate) fn maybe_grow<R>(red_zone: usize, stack_size: usize, f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(red_zone, stack_size, f)
}

#[cfg(not(feature = "stacker"))]
#[inline]
pub(crate) fn maybe_grow<R>(_red_zone: usize, _stack_size: usize, f: impl FnOnce() -> R) -> R {
    f()
}
