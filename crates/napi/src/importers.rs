//! `FileImporter` + full `Importer` bridge (todo #221 slices 4-5b): wraps a
//! JS object shaped like the Sass JS API's `FileImporter`
//! (`{ findFileUrl(url, context) }`) or full `Importer`
//! (`{ canonicalize(url, context), load(canonicalUrl) }`) into a
//! compiler-side [`grass_compiler::Importer`], producing
//! [`ImportResolution::DelegateToPath`] (`FileImporter`) or
//! [`ImportResolution::Resolved`] (full `Importer`) results consumed by
//! `find_import_uncached`'s importer-walk step
//! (`crates/compiler/src/evaluate/visitor.rs`).
//!
//! The two shapes are mutually exclusive per entry (matching the real Sass
//! JS API's TypeScript types, which forbid mixing `findFileUrl` with
//! `canonicalize`/`load`) and are discriminated at deserialization time —
//! see [`ImporterRef`]'s `FromNapiValue` impl.
//!
//! Both shapes now work under all four entry points: `compile`/
//! `compileString` call straight through (same thread, no channel needed —
//! see [`FileImporterRef`]/[`FullImporterRef`]), while `compileAsync`/
//! `compileStringAsync` upgrade each entry into a `ThreadsafeFunction`-backed
//! handle ([`AsyncFileImporterRef`]/[`AsyncFullImporterRef`]) via
//! [`ImporterRef::into_threadsafe`], reusing `functions.rs`'s slice 3
//! calling convention (dummy no-op JS target + hand-rolled catch, since
//! napi's `call_with_return_value` aborts the process on a thrown JS
//! exception — see that module's doc comment for the full rationale) and
//! its documented Promise-return limitation (a `canonicalize`/`load`/
//! `findFileUrl` that itself returns a Promise cannot be awaited from here;
//! see [`PROMISE_RETURN_ERR`]).
//!
//! A full `Importer`'s `canonicalize`+`load` pair needs *two* sequential
//! round trips per resolution attempt — under the async calling convention
//! that's two separate `ThreadsafeFunction`s, called one after the other
//! (blocking-recv on the first before ever calling the second); see
//! [`AsyncFullImporterRef::canonicalize`].
//!
//! ## Why `FileImporterRef`/`FullImporterRef` still need unsafe `Send`/`Sync` impls
//!
//! The compiler-side `Importer` trait itself has no `Send`/`Sync` bound —
//! `Options::importers: Vec<Rc<dyn Importer>>`, an `Rc`, not an `Arc` like
//! `BuiltinFn::Dynamic`'s `Arc<dyn Fn(...) + Send + Sync>` — so nothing
//! about *import resolution* needs to cross threads for the sync entry
//! points. But a sync `ImporterRef` also lives inside `CompileOptions`,
//! which is itself stored inside `CompileTask`/`CompileStringTask`
//! (`lib.rs`), and napi's `Task` trait requires `Task: Send`. That bound is
//! checked at the *type* level, not the value level — even though
//! `take_async_importers` always upgrades a real `ImporterRef` into an
//! async handle (or leaves the field `None`) before an `AsyncTask` is ever
//! constructed, so a sync `ImporterRef` is never actually moved into a task,
//! `CompileOptions`'s *type* must still satisfy `Send` for the struct
//! embedding it to compile at all. Same reasoning `functions.rs`'s
//! `JsFunctionRef`/`SyncEnv` needs.
//!
//! The real safety invariant is unchanged: a `FileImporterRef`/
//! `FullImporterRef` must never be read from (or dropped, since `Drop` calls
//! back into `Env`) outside the single synchronous `compile`/`compileString`
//! call whose argument deserialization constructed it. The async handles
//! (`AsyncFileImporterRef`/`AsyncFullImporterRef`) need no such unsafe impl
//! — they hold only `ThreadsafeFunction`s, which are `Send`/`Sync` by
//! design (that's the entire point of the type).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;

use napi::bindgen_prelude::FromNapiValue;
use napi::threadsafe_function::{
    ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::{sys, Env, Error, JsFunction, JsObject, JsUnknown, NapiValue, Ref, Result, Status, ValueType};

use grass_compiler::codemap::Span;
use grass_compiler::{ImportResolution, Importer, InputSyntax, Options, Result as SassResult};

use crate::functions::noop_callback;

/// The error message produced when `canonicalize`/`load`/`findFileUrl`
/// returns a `Promise`/thenable under `compileAsync`/`compileStringAsync`.
/// Real dart-sass awaits such a return value; grass's blocking-channel
/// calling convention cannot safely do that (same limitation as
/// `functions.rs`'s custom functions — see its module doc comment), so this
/// is a deliberate, documented gap rather than a silent hang or a value
/// mis-marshalled as if it were a plain string/object.
const PROMISE_RETURN_ERR: &str = "async importers returning a Promise from canonicalize/load/ \
     findFileUrl are not yet supported in compileAsync/compileStringAsync; use a synchronous \
     importer";

fn napi_err_to_sass(err: &Error, span: Span) -> Box<grass_compiler::Error> {
    (err.reason.clone(), span).into()
}

fn string_err_to_sass(msg: String, span: Span) -> Box<grass_compiler::Error> {
    (msg, span).into()
}

/// Builds the `context: { fromImport, containingUrl }` argument passed to
/// `canonicalize`/`findFileUrl`, per the Sass JS API's
/// `CanonicalizeContext`.
fn build_context(env: Env, from_import: bool, containing_url: Option<&str>) -> Result<JsUnknown> {
    let mut ctx = env.create_object()?;
    ctx.set_named_property("fromImport", from_import)?;
    ctx.set_named_property("containingUrl", containing_url.map(str::to_owned))?;
    Ok(ctx.into_unknown())
}

/// Interprets `findFileUrl`'s return value (shared between the sync and
/// async calling conventions): `null`/`undefined` decline (`Ok(None)`), a
/// string is treated as a `file:` URL (`Ok(Some(url))` — the `file://`
/// scheme prefix is stripped if present, bare unprefixed strings accepted
/// too as an ergonomic relaxation beyond the real API), a Promise/thenable
/// hits the documented async limitation, anything else is a clear type
/// error. No percent-decoding and no Windows drive-letter handling is
/// performed — a documented gap, not exercised by this project's
/// macOS/Linux dev and CI targets.
fn interpret_find_file_url_return(js_return: JsUnknown) -> std::result::Result<Option<String>, String> {
    let ty = js_return.get_type().map_err(|e| e.reason.clone())?;
    if matches!(ty, ValueType::Null | ValueType::Undefined) {
        return Ok(None);
    }
    if js_return.is_promise().unwrap_or(false) {
        return Err(PROMISE_RETURN_ERR.to_owned());
    }
    if ty != ValueType::String {
        return Err(format!(
            "findFileUrl must return a file: URL string or null/undefined, got JS type {ty:?}"
        ));
    }
    js_return
        .coerce_to_string()
        .and_then(|s| s.into_utf8())
        .and_then(|s| s.into_owned())
        .map_err(|e| e.reason.clone())
        .map(Some)
}

/// Interprets `canonicalize`'s return value (shared between the sync and
/// async calling conventions): `null`/`undefined` decline (`Ok(None)`), a
/// string is the canonical URL (`Ok(Some(url))`), a Promise/thenable hits
/// the documented async limitation, anything else is a clear type error.
fn interpret_canonicalize_return(js_return: JsUnknown) -> std::result::Result<Option<String>, String> {
    let ty = js_return.get_type().map_err(|e| e.reason.clone())?;
    if matches!(ty, ValueType::Null | ValueType::Undefined) {
        return Ok(None);
    }
    if js_return.is_promise().unwrap_or(false) {
        return Err(PROMISE_RETURN_ERR.to_owned());
    }
    if ty != ValueType::String {
        return Err(format!(
            "canonicalize must return a URL string or null/undefined, got JS type {ty:?}"
        ));
    }
    js_return
        .coerce_to_string()
        .and_then(|s| s.into_utf8())
        .and_then(|s| s.into_owned())
        .map_err(|e| e.reason.clone())
        .map(Some)
}

/// Interprets `load`'s return value (shared between the sync and async
/// calling conventions): `null`/`undefined` decline (`Ok(None)`), an object
/// with `{contents, syntax}` resolves (`Ok(Some((contents, syntax_str)))`,
/// `syntax_str` mapped to an [`InputSyntax`] by the caller), a
/// Promise/thenable hits the documented async limitation, anything else is
/// a clear type error.
fn interpret_load_return(
    js_return: JsUnknown,
) -> std::result::Result<Option<(String, String)>, String> {
    let ty = js_return.get_type().map_err(|e| e.reason.clone())?;
    if matches!(ty, ValueType::Null | ValueType::Undefined) {
        return Ok(None);
    }
    if js_return.is_promise().unwrap_or(false) {
        return Err(PROMISE_RETURN_ERR.to_owned());
    }
    if ty != ValueType::Object {
        return Err(format!(
            "load must return {{contents, syntax}} or null/undefined, got JS type {ty:?}"
        ));
    }

    let obj = js_return.coerce_to_object().map_err(|e| e.reason.clone())?;

    let contents = obj
        .get_named_property::<JsUnknown>("contents")
        .map_err(|e| e.reason.clone())
        .and_then(coerce_to_owned_string)?;
    let syntax = obj
        .get_named_property::<JsUnknown>("syntax")
        .map_err(|e| e.reason.clone())
        .and_then(coerce_to_owned_string)?;

    Ok(Some((contents, syntax)))
}

fn coerce_to_owned_string(v: JsUnknown) -> std::result::Result<String, String> {
    v.coerce_to_string()
        .and_then(|s| s.into_utf8())
        .and_then(|s| s.into_owned())
        .map_err(|e| e.reason.clone())
}

/// Maps `load`'s `syntax` string (`'scss' | 'sass' | 'css'`, per the Sass JS
/// API) to an [`InputSyntax`]; any other value is a clear error.
fn syntax_from_str(s: &str) -> std::result::Result<InputSyntax, String> {
    match s {
        "scss" => Ok(InputSyntax::Scss),
        "sass" => Ok(InputSyntax::Sass),
        "css" => Ok(InputSyntax::Css),
        other => Err(format!(
            "load's `syntax` must be \"scss\", \"sass\", or \"css\", got {other:?}"
        )),
    }
}

fn file_url_to_path(url: &str) -> PathBuf {
    PathBuf::from(url.strip_prefix("file://").unwrap_or(url))
}

// --- Sync FileImporter (todo #221 slice 4) ---------------------------------

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

// SAFETY: see the module doc comment's "Why `FileImporterRef`/
// `FullImporterRef` still need unsafe `Send`/`Sync` impls" section — a real
// `FileImporterRef` is never actually sent across threads for the sync
// entry points; this only satisfies the type-level `Send` bound
// `CompileOptions` (embedded in `CompileTask`/`CompileStringTask`) needs to
// compile.
unsafe impl Send for FileImporterRef {}
unsafe impl Sync for FileImporterRef {}

impl FileImporterRef {
    fn from_object(env: Env, obj: &JsObject) -> Result<Self> {
        let find_file_url: JsFunction = obj.get_named_property("findFileUrl")?;
        let find_file_url_ref = env.create_reference(find_file_url)?;
        Ok(FileImporterRef {
            env,
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
        let ctx = build_context(env, from_import, containing_url).map_err(|e| napi_err_to_sass(&e, span))?;

        match func.call(None, &[url_js, ctx]) {
            Ok(js_return) => match interpret_find_file_url_return(js_return) {
                Ok(Some(s)) => Ok(ImportResolution::DelegateToPath(file_url_to_path(&s))),
                Ok(None) => Ok(ImportResolution::NotFound),
                Err(msg) => Err(string_err_to_sass(msg, span)),
            },
            // A thrown JS exception is stringified and carries the call-site
            // span, same as any other compile error.
            Err(e) => Err(napi_err_to_sass(&e, span)),
        }
    }
}

// --- Sync full Importer (todo #221 slice 5b) -------------------------------

/// A persistent reference to a JS full `Importer`'s `canonicalize`+`load`
/// methods, together with the `Env` it was created from. Same
/// construction/lifetime invariants as [`FileImporterRef`].
pub struct FullImporterRef {
    env: Env,
    canonicalize_ref: Ref<()>,
    load_ref: Ref<()>,
}

impl std::fmt::Debug for FullImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FullImporterRef").finish_non_exhaustive()
    }
}

// SAFETY: see `FileImporterRef`'s identical unsafe impl above and the module
// doc comment.
unsafe impl Send for FullImporterRef {}
unsafe impl Sync for FullImporterRef {}

impl FullImporterRef {
    fn from_object(env: Env, obj: &JsObject) -> Result<Self> {
        let canonicalize: JsFunction = obj.get_named_property("canonicalize")?;
        let load: JsFunction = obj.get_named_property("load")?;
        let canonicalize_ref = env.create_reference(canonicalize)?;
        let load_ref = env.create_reference(load)?;
        Ok(FullImporterRef {
            env,
            canonicalize_ref,
            load_ref,
        })
    }
}

impl Drop for FullImporterRef {
    fn drop(&mut self) {
        let _ = self.canonicalize_ref.unref(self.env);
        let _ = self.load_ref.unref(self.env);
    }
}

impl Importer for FullImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        let env = self.env;

        let canonicalize_fn = env
            .get_reference_value::<JsFunction>(&self.canonicalize_ref)
            .map_err(|e| napi_err_to_sass(&e, span))?;
        let url_js = env
            .create_string(url)
            .map_err(|e| napi_err_to_sass(&e, span))?
            .into_unknown();
        let ctx = build_context(env, from_import, containing_url).map_err(|e| napi_err_to_sass(&e, span))?;

        let canonical_url = match canonicalize_fn.call(None, &[url_js, ctx]) {
            Ok(js_return) => match interpret_canonicalize_return(js_return) {
                Ok(Some(u)) => u,
                Ok(None) => return Ok(ImportResolution::NotFound),
                Err(msg) => return Err(string_err_to_sass(msg, span)),
            },
            Err(e) => return Err(napi_err_to_sass(&e, span)),
        };

        let load_fn = env
            .get_reference_value::<JsFunction>(&self.load_ref)
            .map_err(|e| napi_err_to_sass(&e, span))?;
        let url_arg = env
            .create_string(&canonical_url)
            .map_err(|e| napi_err_to_sass(&e, span))?
            .into_unknown();

        match load_fn.call(None, &[url_arg]) {
            Ok(js_return) => match interpret_load_return(js_return) {
                Ok(Some((contents, syntax_str))) => {
                    let syntax = syntax_from_str(&syntax_str).map_err(|msg| string_err_to_sass(msg, span))?;
                    Ok(ImportResolution::Resolved {
                        canonical_url,
                        contents,
                        syntax,
                    })
                }
                Ok(None) => Ok(ImportResolution::NotFound),
                Err(msg) => Err(string_err_to_sass(msg, span)),
            },
            Err(e) => Err(napi_err_to_sass(&e, span)),
        }
    }
}

// --- Shape discrimination (todo #221 slice 5b) -----------------------------

/// Either JS-facing `importers` shape, per the Sass JS API: `FileImporter`
/// (`findFileUrl` only) or full `Importer` (`canonicalize`+`load`, no
/// `findFileUrl`). Mutually exclusive per entry, discriminated in
/// [`FromNapiValue::from_napi_value`] below by which methods the passed
/// object actually has.
pub enum ImporterRef {
    File(FileImporterRef),
    Full(FullImporterRef),
}

impl std::fmt::Debug for ImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImporterRef::File(_) => f.debug_tuple("ImporterRef::File").finish(),
            ImporterRef::Full(_) => f.debug_tuple("ImporterRef::Full").finish(),
        }
    }
}

impl Importer for ImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        match self {
            ImporterRef::File(f) => f.canonicalize(url, from_import, containing_url, span),
            ImporterRef::Full(full) => full.canonicalize(url, from_import, containing_url, span),
        }
    }
}

impl FromNapiValue for ImporterRef {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let env_wrapped = Env::from(env);
        let obj: JsObject = unsafe { JsObject::from_raw(env, napi_val)? };

        let has_find_file_url = obj.has_named_property("findFileUrl").unwrap_or(false);
        let has_canonicalize = obj.has_named_property("canonicalize").unwrap_or(false);
        let has_load = obj.has_named_property("load").unwrap_or(false);

        if has_find_file_url && !has_canonicalize && !has_load {
            Ok(ImporterRef::File(FileImporterRef::from_object(env_wrapped, &obj)?))
        } else if !has_find_file_url && has_canonicalize && has_load {
            Ok(ImporterRef::Full(FullImporterRef::from_object(env_wrapped, &obj)?))
        } else if !has_find_file_url && !has_canonicalize && !has_load {
            Err(Error::from_reason(
                "each `importers` entry must be an object with either a `findFileUrl(url, \
                 context)` method (FileImporter shape) or `canonicalize(url, context)` + \
                 `load(canonicalUrl)` methods (full Importer shape)"
                    .to_owned(),
            ))
        } else {
            Err(Error::from_reason(
                "each `importers` entry must be EITHER a FileImporter ({findFileUrl(url, \
                 context)}) OR a full Importer ({canonicalize(url, context), \
                 load(canonicalUrl)}), not a mix of both shapes"
                    .to_owned(),
            ))
        }
    }
}

/// Registers `importers` (an ordered list of `FileImporter`/`Importer`
/// entries, checked in array order ahead of the default filesystem/load-path
/// resolution — see `Options::add_importer`) onto `options`. Only safe to
/// call for the SYNCHRONOUS entry points (`compile`/`compileString`) — see
/// this module's doc comment.
pub fn register_importers(mut options: Options<'static>, importers: Vec<ImporterRef>) -> Options<'static> {
    for importer in importers {
        options = options.add_importer(Rc::new(importer));
    }
    options
}

// --- Async calling convention (todo #221 slice 5b) -------------------------
// Reuses `functions.rs`'s slice 3 design exactly: a dummy no-op JS
// `ThreadsafeFunction` target, the real call done by hand inside the
// threadsafe callback (catching a thrown exception as a plain `Result::Err`
// instead of letting napi's fatal-error/process-abort path see it), and a
// Promise-return guard checked immediately after the call. See
// `functions.rs`'s module doc comment for the full rationale.

type FileImporterCallArgs = (
    String,
    bool,
    Option<String>,
    mpsc::Sender<std::result::Result<Option<String>, String>>,
);

/// A JS `FileImporter` callback usable from `Task::compute()` (i.e. off the
/// JS thread) — the async counterpart to [`FileImporterRef`]. Built via
/// [`FileImporterRef::into_threadsafe`] on the JS thread.
pub struct AsyncFileImporterRef {
    tsfn: ThreadsafeFunction<FileImporterCallArgs, ErrorStrategy::Fatal>,
}

impl std::fmt::Debug for AsyncFileImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncFileImporterRef").finish_non_exhaustive()
    }
}

impl AsyncFileImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        let (tx, rx) = mpsc::channel();
        let status = self.tsfn.call(
            (url.to_owned(), from_import, containing_url.map(str::to_owned), tx),
            ThreadsafeFunctionCallMode::Blocking,
        );
        if status != Status::Ok {
            return Err(string_err_to_sass(
                format!("failed to schedule JS importer callback: {status:?}"),
                span,
            ));
        }

        match rx.recv() {
            Ok(Ok(Some(s))) => Ok(ImportResolution::DelegateToPath(file_url_to_path(&s))),
            Ok(Ok(None)) => Ok(ImportResolution::NotFound),
            Ok(Err(msg)) => Err(string_err_to_sass(msg, span)),
            Err(_) => Err(string_err_to_sass(
                "the JS importer callback thread was dropped before responding".to_owned(),
                span,
            )),
        }
    }
}

impl FileImporterRef {
    /// Upgrades this reference into a [`ThreadsafeFunction`]-backed handle
    /// callable from `Task::compute()` (off the JS thread) — todo #221
    /// slice 5b, mirroring `functions.rs`'s
    /// [`JsFunctionRef::into_threadsafe`]. Must be called on the JS thread
    /// that owns `self.env`, before an `AsyncTask` is constructed (see
    /// `lib.rs`'s `take_async_importers`).
    pub(crate) fn into_threadsafe(self) -> Result<AsyncFileImporterRef> {
        let outer_env = self.env;
        let noop = outer_env.create_function("grass_async_importer_find_file_url_target", noop_callback)?;

        let tsfn = noop.create_threadsafe_function::<FileImporterCallArgs, JsUnknown, _, ErrorStrategy::Fatal>(
            0,
            move |ctx: ThreadSafeCallContext<FileImporterCallArgs>| -> Result<Vec<JsUnknown>> {
                let (url, from_import, containing_url, tx) = ctx.value;
                let env = ctx.env;

                let outcome: std::result::Result<Option<String>, String> = (|| {
                    let func = env
                        .get_reference_value::<JsFunction>(&self.find_file_url_ref)
                        .map_err(|e| e.reason.clone())?;
                    let url_js = env.create_string(&url).map_err(|e| e.reason.clone())?.into_unknown();
                    let ctx_obj = build_context(env, from_import, containing_url.as_deref())
                        .map_err(|e| e.reason.clone())?;

                    let js_return = func.call(None, &[url_js, ctx_obj]).map_err(|e| e.reason.clone())?;
                    interpret_find_file_url_return(js_return)
                })();

                // Always send SOMETHING and always return Ok(..) — the
                // worker thread's `rx.recv()` must never be left hanging,
                // and letting this closure return Err would route through
                // napi's fatal-error/process-abort path (see
                // `functions.rs`'s module doc comment).
                let _ = tx.send(outcome);
                Ok(Vec::new())
            },
        )?;

        Ok(AsyncFileImporterRef { tsfn })
    }
}

type CanonicalizeCallArgs = (
    String,
    bool,
    Option<String>,
    mpsc::Sender<std::result::Result<Option<String>, String>>,
);
type LoadCallArgs = (String, mpsc::Sender<std::result::Result<Option<(String, String)>, String>>);

/// A JS full `Importer`'s `canonicalize`+`load` pair usable from
/// `Task::compute()` — the async counterpart to [`FullImporterRef`]. Two
/// separate `ThreadsafeFunction`s (one per JS method), called sequentially:
/// `canonicalize` first, and only if it resolves to a URL, `load` with that
/// URL.
pub struct AsyncFullImporterRef {
    canonicalize_tsfn: ThreadsafeFunction<CanonicalizeCallArgs, ErrorStrategy::Fatal>,
    load_tsfn: ThreadsafeFunction<LoadCallArgs, ErrorStrategy::Fatal>,
}

impl std::fmt::Debug for AsyncFullImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncFullImporterRef").finish_non_exhaustive()
    }
}

impl AsyncFullImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        let (tx, rx) = mpsc::channel();
        let status = self.canonicalize_tsfn.call(
            (url.to_owned(), from_import, containing_url.map(str::to_owned), tx),
            ThreadsafeFunctionCallMode::Blocking,
        );
        if status != Status::Ok {
            return Err(string_err_to_sass(
                format!("failed to schedule JS importer callback: {status:?}"),
                span,
            ));
        }

        let canonical_url = match rx.recv() {
            Ok(Ok(Some(u))) => u,
            Ok(Ok(None)) => return Ok(ImportResolution::NotFound),
            Ok(Err(msg)) => return Err(string_err_to_sass(msg, span)),
            Err(_) => {
                return Err(string_err_to_sass(
                    "the JS importer callback thread was dropped before responding".to_owned(),
                    span,
                ))
            }
        };

        let (tx2, rx2) = mpsc::channel();
        let status2 = self
            .load_tsfn
            .call((canonical_url.clone(), tx2), ThreadsafeFunctionCallMode::Blocking);
        if status2 != Status::Ok {
            return Err(string_err_to_sass(
                format!("failed to schedule JS importer callback: {status2:?}"),
                span,
            ));
        }

        match rx2.recv() {
            Ok(Ok(Some((contents, syntax_str)))) => {
                let syntax = syntax_from_str(&syntax_str).map_err(|msg| string_err_to_sass(msg, span))?;
                Ok(ImportResolution::Resolved {
                    canonical_url,
                    contents,
                    syntax,
                })
            }
            Ok(Ok(None)) => Ok(ImportResolution::NotFound),
            Ok(Err(msg)) => Err(string_err_to_sass(msg, span)),
            Err(_) => Err(string_err_to_sass(
                "the JS importer callback thread was dropped before responding".to_owned(),
                span,
            )),
        }
    }
}

impl FullImporterRef {
    /// Upgrades this reference into two [`ThreadsafeFunction`]-backed
    /// handles (one per JS method) callable from `Task::compute()` — todo
    /// #221 slice 5b. `self` is wrapped in an `Arc` (NOT `Rc` — `Rc` is
    /// unconditionally `!Send`/`!Sync` regardless of unsafe impls on the
    /// pointee, since its refcount isn't atomic; `Arc`'s IS, so `Arc<T>`
    /// really is `Send`/`Sync` whenever `T` is) so both closures can share
    /// ownership without a partial move out of a `Drop` type (a plain
    /// destructure of `self.canonicalize_ref`/`self.load_ref` is rejected by
    /// the borrow checker since `FullImporterRef` has a custom `Drop` impl);
    /// the underlying JS refs are unreffed exactly once, when the last
    /// `ThreadsafeFunction` holding a clone is itself dropped.
    pub(crate) fn into_threadsafe(self) -> Result<AsyncFullImporterRef> {
        let env = self.env;
        let shared = std::sync::Arc::new(self);

        let canonicalize_noop =
            env.create_function("grass_async_importer_canonicalize_target", noop_callback)?;
        let shared_for_canonicalize = shared.clone();
        let canonicalize_tsfn = canonicalize_noop
            .create_threadsafe_function::<CanonicalizeCallArgs, JsUnknown, _, ErrorStrategy::Fatal>(
                0,
                move |ctx: ThreadSafeCallContext<CanonicalizeCallArgs>| -> Result<Vec<JsUnknown>> {
                    let (url, from_import, containing_url, tx) = ctx.value;
                    let env = ctx.env;

                    let outcome: std::result::Result<Option<String>, String> = (|| {
                        let func = env
                            .get_reference_value::<JsFunction>(&shared_for_canonicalize.canonicalize_ref)
                            .map_err(|e| e.reason.clone())?;
                        let url_js = env.create_string(&url).map_err(|e| e.reason.clone())?.into_unknown();
                        let ctx_obj = build_context(env, from_import, containing_url.as_deref())
                            .map_err(|e| e.reason.clone())?;

                        let js_return = func.call(None, &[url_js, ctx_obj]).map_err(|e| e.reason.clone())?;
                        interpret_canonicalize_return(js_return)
                    })();

                    let _ = tx.send(outcome);
                    Ok(Vec::new())
                },
            )?;

        let load_noop = env.create_function("grass_async_importer_load_target", noop_callback)?;
        let shared_for_load = shared.clone();
        let load_tsfn = load_noop.create_threadsafe_function::<LoadCallArgs, JsUnknown, _, ErrorStrategy::Fatal>(
            0,
            move |ctx: ThreadSafeCallContext<LoadCallArgs>| -> Result<Vec<JsUnknown>> {
                let (canonical_url, tx) = ctx.value;
                let env = ctx.env;

                let outcome: std::result::Result<Option<(String, String)>, String> = (|| {
                    let func = env
                        .get_reference_value::<JsFunction>(&shared_for_load.load_ref)
                        .map_err(|e| e.reason.clone())?;
                    let url_js = env
                        .create_string(&canonical_url)
                        .map_err(|e| e.reason.clone())?
                        .into_unknown();

                    let js_return = func.call(None, &[url_js]).map_err(|e| e.reason.clone())?;
                    interpret_load_return(js_return)
                })();

                let _ = tx.send(outcome);
                Ok(Vec::new())
            },
        )?;

        Ok(AsyncFullImporterRef {
            canonicalize_tsfn,
            load_tsfn,
        })
    }
}

/// The async counterpart to [`ImporterRef`] — either JS-facing shape,
/// upgraded to its `ThreadsafeFunction`-backed handle via
/// [`ImporterRef::into_threadsafe`].
pub enum AsyncImporterRef {
    File(AsyncFileImporterRef),
    Full(AsyncFullImporterRef),
}

impl std::fmt::Debug for AsyncImporterRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AsyncImporterRef::File(_) => f.debug_tuple("AsyncImporterRef::File").finish(),
            AsyncImporterRef::Full(_) => f.debug_tuple("AsyncImporterRef::Full").finish(),
        }
    }
}

impl Importer for AsyncImporterRef {
    fn canonicalize(
        &self,
        url: &str,
        from_import: bool,
        containing_url: Option<&str>,
        span: Span,
    ) -> SassResult<ImportResolution> {
        match self {
            AsyncImporterRef::File(f) => f.canonicalize(url, from_import, containing_url, span),
            AsyncImporterRef::Full(full) => full.canonicalize(url, from_import, containing_url, span),
        }
    }
}

impl ImporterRef {
    /// Upgrades this reference into its async counterpart — a JS-thread-only
    /// step (building a `ThreadsafeFunction` needs `Env`), performed in
    /// `compile_async`/`compile_string_async`'s synchronous body before the
    /// `AsyncTask`/`Task::compute()` (which runs off the JS thread) is ever
    /// constructed. See `lib.rs`'s `take_async_importers`.
    pub(crate) fn into_threadsafe(self) -> Result<AsyncImporterRef> {
        match self {
            ImporterRef::File(f) => Ok(AsyncImporterRef::File(f.into_threadsafe()?)),
            ImporterRef::Full(full) => Ok(AsyncImporterRef::Full(full.into_threadsafe()?)),
        }
    }
}

/// Registers `importers` for the ASYNCHRONOUS entry points (`compileAsync`/
/// `compileStringAsync`), todo #221 slice 5b. `importers` must already be
/// upgraded via [`ImporterRef::into_threadsafe`] (a JS-thread-only step,
/// performed in `compile_async`/`compile_string_async`'s synchronous body
/// before the `AsyncTask` is constructed — see `lib.rs`'s
/// `take_async_importers`). This function itself touches no `Env` and is
/// safe to call from `Task::compute()` (off the JS thread), matching where
/// it's actually used.
pub fn register_importers_async(
    mut options: Options<'static>,
    importers: Vec<AsyncImporterRef>,
) -> Options<'static> {
    for importer in importers {
        options = options.add_importer(Rc::new(importer));
    }
    options
}
