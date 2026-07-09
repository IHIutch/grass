//! Sync `FileImporter` bridge (todo #221 slice 4): wraps a JS object shaped
//! like the Sass JS API's `FileImporter` (`{ findFileUrl(url, context) }`)
//! into a compiler-side [`grass_compiler::Importer`], producing
//! [`ImportResolution::DelegateToPath`] results consumed by
//! `find_import_uncached`'s importer-walk step
//! (`crates/compiler/src/evaluate/visitor.rs`).
//!
//! Only the `FileImporter` shape is supported — a full JS `Importer`
//! (`canonicalize`+`load`, arbitrary non-`file:` schemes,
//! `ImportResolution::Resolved`) is todo #221 slice 5.
//!
//! Sync entry points only (`compile`/`compileString`); `compileAsync`/
//! `compileStringAsync` reject a non-empty `importers` list outright
//! (`lib.rs`'s `reject_importers_for_async`) rather than silently ignoring
//! it or attempting to support it — unlike `functions`, which upgraded from
//! a sync-only rejection (its own former slice 2) to a real
//! `ThreadsafeFunction`-backed bridge in slice 3, closing the async gap for
//! importers is deferred to slice 5 alongside the full `Importer` shape,
//! not treated as a smaller standalone follow-up.
//!
//! ## Why `FileImporterRef` still needs an unsafe `Send`/`Sync` impl
//!
//! The compiler-side `Importer` trait itself has no `Send`/`Sync` bound —
//! `Options::importers: Vec<Rc<dyn Importer>>`, an `Rc`, not an `Arc` like
//! `BuiltinFn::Dynamic`'s `Arc<dyn Fn(...) + Send + Sync>` — so nothing
//! about *import resolution* needs to cross threads. But `FileImporterRef`
//! also lives inside `CompileOptions`, which is itself stored inside
//! `CompileTask`/`CompileStringTask` (`lib.rs`), and napi's `Task` trait
//! requires `Task: Send`. That bound is checked at the *type* level, not
//! the value level — even though `reject_importers_for_async` guarantees a
//! non-empty `importers` list never reaches `compile_async`/
//! `compile_string_async` (so a real `FileImporterRef` is never actually
//! moved into a task, only ever `None`), `CompileOptions`'s *type* must
//! still satisfy `Send` for the struct embedding it to compile at all. This
//! is exactly the same reason `functions.rs`'s `JsFunctionRef` needs its
//! `SyncEnv` unsafe impl, even though `take_async_functions` similarly
//! empties that field before a task is ever constructed.
//!
//! The real safety invariant is unchanged from `functions.rs`: a
//! `FileImporterRef` must never be read from (or dropped, since `Drop`
//! calls back into `Env`) outside the single synchronous `compile`/
//! `compileString` call whose argument deserialization constructed it.

use std::path::PathBuf;

use napi::bindgen_prelude::FromNapiValue;
use napi::{sys, Env, Error, JsFunction, JsObject, JsUnknown, NapiValue, Ref, Result, ValueType};

use grass_compiler::codemap::Span;
use grass_compiler::{ImportResolution, Importer, Options, Result as SassResult};

/// A persistent reference to a JS `FileImporter`'s `findFileUrl` method,
/// together with the `Env` it was created from. Only ever constructed while
/// deserializing a `compile`/`compileString` call's `importers` argument
/// (on the JS thread) and only ever used/dropped within that same
/// synchronous call.
pub struct FileImporterRef {
    env: Env,
    find_file_url_ref: Ref<()>,
}

impl std::fmt::Debug for FileImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileImporterRef").finish_non_exhaustive()
    }
}

// SAFETY: see the module doc comment's "Why `FileImporterRef` still needs
// an unsafe `Send`/`Sync` impl" section — a real `FileImporterRef` is never
// actually sent across threads (`reject_importers_for_async` refuses any
// non-empty `importers` list before an `AsyncTask` is ever constructed);
// this only satisfies the type-level `Send` bound `CompileOptions`
// (embedded in `CompileTask`/`CompileStringTask`) needs to compile.
unsafe impl Send for FileImporterRef {}
unsafe impl Sync for FileImporterRef {}

impl FromNapiValue for FileImporterRef {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let env_wrapped = Env::from(env);

        let obj: JsObject = unsafe { JsObject::from_raw(env, napi_val)? };
        let find_file_url: JsFunction = obj.get_named_property("findFileUrl").map_err(|_| {
            Error::from_reason(
                "each `importers` entry must be an object with a `findFileUrl(url, context)` \
                 method (only the FileImporter shape is supported so far — a full `Importer` \
                 with `canonicalize`+`load` is todo #221 slice 5)"
                    .to_owned(),
            )
        })?;
        let find_file_url_ref = env_wrapped.create_reference(find_file_url)?;

        Ok(FileImporterRef {
            env: env_wrapped,
            find_file_url_ref,
        })
    }
}

impl Drop for FileImporterRef {
    fn drop(&mut self) {
        // `Ref<T>`'s own `Drop` (debug builds only) asserts the ref count is
        // already 0 — it does not unref itself. This is the real cleanup;
        // see the module doc comment for why `self.env` is still valid here.
        let _ = self.find_file_url_ref.unref(self.env);
    }
}

impl Importer for FileImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        let env = self.env;

        let func = env
            .get_reference_value::<JsFunction>(&self.find_file_url_ref)
            .map_err(|e| napi_err_to_sass(&e, span))?;

        let url_js = env
            .create_string(url)
            .map_err(|e| napi_err_to_sass(&e, span))?
            .into_unknown();

        let mut ctx = env.create_object().map_err(|e| napi_err_to_sass(&e, span))?;
        ctx.set_named_property("fromImport", from_import)
            .map_err(|e| napi_err_to_sass(&e, span))?;
        ctx.set_named_property("containingUrl", containing_url.map(str::to_owned))
            .map_err(|e| napi_err_to_sass(&e, span))?;

        match func.call(None, &[url_js, ctx.into_unknown()]) {
            Ok(js_return) => js_return_to_resolution(js_return, span),
            // Matches `functions.rs`'s `JsFunctionRef::call`: a thrown JS
            // exception is stringified (`.message`/`.toString()`) and
            // carries the call-site span, same as any other compile error.
            Err(e) => Err(napi_err_to_sass(&e, span)),
        }
    }
}

/// Converts `findFileUrl`'s return value into an [`ImportResolution`].
/// `null`/`undefined` decline (per the JS contract). A string is treated as
/// a `file:` URL — the `file://` scheme prefix is stripped if present
/// (bare, unprefixed strings are accepted too, as an ergonomic relaxation
/// beyond the real API). No percent-decoding and no Windows drive-letter
/// handling is performed — a documented gap, not exercised by this
/// project's macOS/Linux dev and CI targets.
fn js_return_to_resolution(js_return: JsUnknown, span: Span) -> SassResult<ImportResolution> {
    match js_return.get_type().map_err(|e| napi_err_to_sass(&e, span))? {
        ValueType::Null | ValueType::Undefined => Ok(ImportResolution::NotFound),
        ValueType::String => {
            let s = js_return
                .coerce_to_string()
                .and_then(|s| s.into_utf8())
                .and_then(|s| s.into_owned())
                .map_err(|e| napi_err_to_sass(&e, span))?;
            Ok(ImportResolution::DelegateToPath(file_url_to_path(&s)))
        }
        other => Err(string_err_to_sass(
            format!(
                "findFileUrl must return a file: URL string or null/undefined, got JS type \
                 {other:?}"
            ),
            span,
        )),
    }
}

fn file_url_to_path(url: &str) -> PathBuf {
    PathBuf::from(url.strip_prefix("file://").unwrap_or(url))
}

fn napi_err_to_sass(err: &Error, span: Span) -> Box<grass_compiler::Error> {
    (err.reason.clone(), span).into()
}

fn string_err_to_sass(msg: String, span: Span) -> Box<grass_compiler::Error> {
    (msg, span).into()
}

/// Registers `importers` (an ordered list of `FileImporter`s, checked in
/// array order ahead of the default filesystem/load-path resolution — see
/// `Options::add_importer`) onto `options`. Only safe to call for the
/// SYNCHRONOUS entry points (`compile`/`compileString`) — see this module's
/// doc comment.
pub fn register_importers(
    mut options: Options<'static>,
    importers: Vec<FileImporterRef>,
) -> Options<'static> {
    for importer in importers {
        options = options.add_importer(std::rc::Rc::new(importer));
    }
    options
}
