extern crate napi_build;

fn main() {
    napi_build::setup();

    // `napi_build::setup()` only applies `-undefined dynamic_lookup` to the
    // *cdylib* artifact (via `cargo:rustc-cdylib-link-arg`) — the real N-API
    // symbols (`napi_wrap`, `napi_call_function`, etc.) are resolved
    // dynamically against the Node host process at `dlopen` time, which is
    // fine for the actual `.node` binary but does NOT cover `cargo test`'s
    // test-harness binary, a plain executable that the normal linker still
    // demands every symbol be resolved for. Since todo #221 slice 2 added
    // `#[napi]` classes (`values.rs`) whose generated `ToNapiValue`/
    // `FromNapiValue` impls call real `napi_*` functions outside any
    // `#[cfg(test)]` gate, `cargo test -p grass_napi` started failing to
    // *link* (not merely to pass) without this.
    //
    // Fix: apply the same `-undefined dynamic_lookup` permissiveness to
    // *all* binaries built from this crate, not just the cdylib — this is
    // the standard napi-rs community workaround for `cargo test` on a napi
    // crate. It only defers symbol resolution to runtime; it doesn't
    // provide the symbols. That's fine as a link-time fix because the unit
    // tests here are careful never to *call* the FFI-touching marshalling
    // code (that's covered by `crates/napi/test.mjs` instead, run against
    // the real cdylib loaded into Node, where the symbols really are
    // available) — if a test ever did call it, it would fail at runtime
    // with a dyld symbol-lookup error instead of a link error.
    //
    // macOS only (matches `napi_build::setup()`'s own platform gate above);
    // not verified on Linux/Windows CI runners.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    }
}
