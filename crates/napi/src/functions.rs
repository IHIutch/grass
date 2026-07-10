//! Sync `functions` bridge (todo #221 slice 2): registers JS callbacks as
//! grass `Builtin::Dynamic` closures (todo #221 slice 1,
//! `Options::add_custom_fn_with_signature`), invoked directly on the JS
//! thread while `compile`/`compileString` run.
//!
//! ## Thread-safety design (Phase A)
//!
//! The compiler-side hook requires `Arc<dyn Fn(...) + Send + Sync>`
//! (`BuiltinFn::Dynamic`, `crates/compiler/src/builtin/functions/mod.rs`),
//! but napi's `JsFunction`/`Env` are tied to the JS thread and are not
//! `Send`/`Sync` by nature. `compile`/`compileString` are fully synchronous
//! — they run entirely on the JS thread, so no callback here is ever
//! actually invoked from a different thread; the `Send + Sync` bound only
//! needs to be *satisfiable at the type level*, not exercised across a real
//! thread hop.
//!
//! We satisfy it the smallest way available rather than pulling in
//! `ThreadsafeFunction` machinery (which exists precisely to hop *across*
//! threads and would be pure overhead here): [`JsFunctionRef`] wraps napi's
//! own `Ref<()>` (persistent reference), which napi-rs itself declares
//! `unsafe impl<T> Send for Ref<T> {}` / `Sync` *unconditionally* — the
//! crate's own soundness contract already assumes a `Ref` is only ever
//! dereferenced (via `Env::get_reference_value`) on the JS thread that owns
//! the `Env` it was created from. We lean on exactly that same contract:
//! [`JsFunctionRef`] additionally carries the `Env` it was constructed
//! from, wrapped in a small local `SyncEnv` with the matching unsafe
//! impl — never used except to re-fetch and call the referenced
//! `JsFunction` from within the *same synchronous call* that constructed
//! it. That invariant holds structurally: `JsFunctionRef` is only ever
//! constructed while deserializing a `compile`/`compileString` call's
//! arguments (on the JS thread) and only ever dropped by the time that same
//! call returns (once `Options` — and the closures capturing an
//! `Arc<JsFunctionRef>` — go out of scope at the end of `compile`/
//! `compile_string`'s body).
//!
//! `compileAsync`/`compileStringAsync` do **not** use `JsFunctionRef`
//! directly — `Task::compute()` runs off the JS thread with no `Env` at all,
//! so nothing capturing a `SyncEnv`/`Ref<JsFunction>` can ever be handed to
//! a worker thread. See the `## Async calling convention (Phase A, slice 3)`
//! section below for how the async entries get there instead.
//!
//! Considered and rejected: `ThreadsafeFunction` even for the sync path
//! (works, but adds queueing/dispatch overhead for a call that's already on
//! the right thread — pure cost, no benefit); napi-rs's newer
//! `bindgen_runtime::Reference<T>` (that's for `#[napi]` *class instances*
//! wrapping Rust data, not for holding a plain `JsFunction` — not the right
//! tool here).
//!
//! ## Async calling convention (Phase A, slice 3)
//!
//! `compileAsync`/`compileStringAsync` upgrade each `JsFunctionRef` into an
//! [`AsyncJsFunctionRef`] on the JS thread, *before* the `AsyncTask` is
//! constructed (see `lib.rs`'s `take_async_functions`), via
//! [`JsFunctionRef::into_threadsafe`]. That upgrade builds a
//! `ThreadsafeFunction` whose call data is `(Vec<WireValue>,
//! mpsc::Sender<Result<WireValue, String>>)` —
//! [`WireValue`](crate::wire::WireValue) is a `Send`, `Env`-free stand-in
//! for `Value` (which is `Rc`-based and therefore `!Send`), used only to
//! survive the thread hop; see `wire.rs`.
//!
//! `AsyncJsFunctionRef::call` (invoked from `Task::compute()`, i.e. a libuv
//! worker thread) marshals its args to `WireValue`, sends them across via
//! `tsfn.call(_, Blocking)`, and blocks on an `mpsc::Receiver` for the
//! response. Getting the *return value* (or a thrown exception) back out of
//! a `ThreadsafeFunction` call is the part napi's high-level
//! `call_with_return_value` doesn't handle safely for us: it treats a
//! thrown JS exception as an unrecoverable `napi_fatal_error` (process
//! abort) rather than a value we can hand back through our channel — and
//! Sass custom functions throwing is completely normal (invalid-argument
//! errors, etc.), so using it would mean "any custom function throws"
//! crashes the whole Node process. Instead, `into_threadsafe` registers a
//! **dummy no-op JS function** (`noop_callback`) as the `ThreadsafeFunction`'s
//! nominal call target (required by the napi API, but never meaningfully
//! invoked — its result is discarded), and does the *real* work by hand
//! inside the threadsafe callback itself: re-fetch the real `JsFunction` via
//! the captured `Ref` (sound here — the callback runs on the JS thread, the
//! same invariant `JsFunctionRef` already relies on for the sync path),
//! call it directly, catch a thrown exception as a plain `Result::Err`, and
//! send the outcome (`Ok(WireValue)` or `Err(String)`) over the channel
//! ourselves. The callback itself always returns `Ok(Vec::new())` to the
//! framework, so the framework's own (unused) invocation of the dummy
//! target never hits a fatal-error path.
//!
//! **Async-returning JS functions** (an `async function`, or anything
//! returning a `Promise`/thenable) are explicitly unsupported: real
//! dart-sass awaits a Promise return in `compileAsync`, but doing that here
//! would mean blocking a libuv worker thread on `rx.recv()` while the result
//! depends on the JS thread's microtask queue draining — instead of
//! attempting it, the callback checks `js_return.is_promise()` immediately
//! after calling the function and, if true, sends a clear error
//! (`PROMISE_RETURN_ERR`) over the channel without ever waiting on the
//! promise to settle. This also transparently covers the riskiest
//! re-entrancy shape (an async custom function that itself `await`s a
//! nested `compileStringAsync`): since calling an `async function` always
//! returns a `Promise` *synchronously* (before any internal `await`
//! resumes), the promise check fires immediately — the nested compile is
//! never awaited by us, so there is nothing to deadlock on. See todo #221's
//! slice 3 report for the concurrency/re-entrancy stress-probe results this
//! reasoning was verified against.

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use napi::bindgen_prelude::FromNapiValue;
use napi::threadsafe_function::{
    ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::{sys, Env, Error, JsFunction, JsUnknown, NapiValue, Ref, Result, Status};

use grass_compiler::codemap::Span;
use grass_compiler::sass_value::{ArgumentResult, Value};
use grass_compiler::{Options, Result as SassResult, Visitor};

use crate::values::{js_value_to_sass, sass_value_to_js};
use crate::wire::{value_to_wire, wire_to_sass, WireValue};

/// # Safety
///
/// See this module's doc comment: a `SyncEnv` is only ever read back within
/// the same synchronous `compile`/`compileString` call that constructed it,
/// which never leaves the JS thread. It must never be stored anywhere that
/// could outlive that call (in particular, never inside an `AsyncTask`/
/// `Task`).
///
/// `pub(crate)` because `importers.rs`'s sync `FileImporter` bridge (todo
/// #221 slice 4) needs the exact same argument — a single JS callback,
/// referenced and invoked only within one synchronous entry-point call —
/// and reuses this type rather than re-deriving the same safety argument.
pub(crate) struct SyncEnv(pub(crate) Env);

// SAFETY: see the module doc comment and `SyncEnv`'s doc comment above.
unsafe impl Send for SyncEnv {}
unsafe impl Sync for SyncEnv {}

/// A persistent reference to a JS function passed via `CompileOptions.functions`,
/// together with the `Env` it was created from. See the module doc comment
/// for the full thread-safety argument.
pub struct JsFunctionRef {
    env: SyncEnv,
    func_ref: Ref<()>,
}

impl JsFunctionRef {
    fn call(&self, args: &[Value], span: Span) -> SassResult<Value> {
        let env = self.env.0;

        let func = env
            .get_reference_value::<JsFunction>(&self.func_ref)
            .map_err(|e| napi_err_to_sass(&e, span))?;

        let mut js_args = Vec::with_capacity(args.len());
        for arg in args {
            js_args.push(sass_value_to_js(env, arg).map_err(|e| napi_err_to_sass(&e, span))?);
        }

        // The JS-facing callback signature is `(args: Value[]) => Value`
        // (matching the real Sass JS API) — a SINGLE array argument, not
        // one JS argument per Sass argument. `JsFunction::call` passes each
        // slice element as its own positional JS argument, so `js_args`
        // must be wrapped into one array value first.
        let args_array =
            crate::values::to_unknown(env, js_args).map_err(|e| napi_err_to_sass(&e, span))?;

        match func.call(None, &[args_array]) {
            Ok(js_return) => {
                js_value_to_sass(env, js_return).map_err(|e| napi_err_to_sass(&e, span))
            }
            // A thrown JS exception. `napi::Error::from` (invoked internally
            // by `JsFunction::call` on a pending exception) stringifies the
            // thrown value via `coerce_to_string` — for a thrown `Error`
            // object this reproduces JS's default `Error.prototype.toString`
            // (`"Error: <message>"`); for a thrown bare string/value it's
            // that value's string coercion. Real dart-sass instead exposes
            // `.message`/`.sassMessage` as distinct fields plus a formatted
            // source frame; slice 2 does not replicate that distinction,
            // documented divergence (todo #221 slice 2 report).
            Err(e) => Err(napi_err_to_sass(&e, span)),
        }
    }
}

impl Drop for JsFunctionRef {
    fn drop(&mut self) {
        // `Ref<T>`'s own `Drop` (debug builds only) asserts the ref count is
        // already 0 — it does not unref itself. This is the real cleanup;
        // see the module doc comment for why `self.env` is still valid here.
        let _ = self.func_ref.unref(self.env.0);
    }
}

impl FromNapiValue for JsFunctionRef {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> Result<Self> {
        let env_wrapped = Env::from(env);

        let unknown = unsafe { napi::JsUnknown::from_raw(env, napi_val)? };
        if unknown.get_type()? != napi::ValueType::Function {
            return Err(Error::from_reason(
                "`functions` values must be JS functions".to_owned(),
            ));
        }

        let func: JsFunction = unsafe { JsFunction::from_raw(env, napi_val)? };
        let func_ref = env_wrapped.create_reference(func)?;

        Ok(JsFunctionRef {
            env: SyncEnv(env_wrapped),
            func_ref,
        })
    }
}

fn napi_err_to_sass(err: &Error, span: Span) -> Box<grass_compiler::Error> {
    (err.reason.clone(), span).into()
}

/// Registers `functions` (signature string -> JS callback) onto `options`
/// via todo #221 slice 1's `Options::add_custom_fn_with_signature`. Only
/// safe to call for the SYNCHRONOUS entry points (`compile`/
/// `compileString`) — see this module's doc comment.
pub fn register_functions(
    mut options: Options<'static>,
    functions: HashMap<String, JsFunctionRef>,
) -> Result<Options<'static>> {
    for (signature, func_ref) in functions {
        let handle = Arc::new(func_ref);

        options = options
            .add_custom_fn_with_signature(
                signature,
                move |mut args: ArgumentResult, _: &mut Visitor| {
                    let span = args.span();

                    let mut sass_args = Vec::new();
                    let mut i = 0;
                    while let Some(spanned) = args.get_positional(i) {
                        sass_args.push(spanned.node);
                        i += 1;
                    }

                    handle.call(&sass_args, span)
                },
            )
            .map_err(|e| Error::from_reason(e.to_string()))?;
    }

    Ok(options)
}

// --- Async calling convention (todo #221 slice 3) --------------------------
// See this module's doc comment, `## Async calling convention` section, for
// the full design rationale.

/// The error message produced when a `functions` callback returns a
/// `Promise`/thenable under `compileAsync`/`compileStringAsync`. Real
/// dart-sass awaits such a return value; grass's blocking-channel calling
/// convention cannot safely do that (see the module doc comment), so this
/// is a deliberate, documented gap rather than a silent hang or a value
/// mis-marshalled as if it were a plain `Value`.
const PROMISE_RETURN_ERR: &str = "async custom functions returning a Promise are not yet \
     supported in compileAsync; use a synchronous function";

type ThreadsafeCallArgs = (
    Vec<WireValue>,
    mpsc::Sender<std::result::Result<WireValue, String>>,
);

/// A JS `functions` callback usable from `Task::compute()` (i.e. off the JS
/// thread) — todo #221 slice 3's async calling convention. Built via
/// [`JsFunctionRef::into_threadsafe`] on the JS thread; [`Self::call`] is
/// meant to be invoked from a libuv worker thread.
pub struct AsyncJsFunctionRef {
    tsfn: ThreadsafeFunction<ThreadsafeCallArgs, ErrorStrategy::Fatal>,
}

impl AsyncJsFunctionRef {
    pub(crate) fn call(&self, args: &[Value], span: Span) -> SassResult<Value> {
        let mut wire_args = Vec::with_capacity(args.len());
        for arg in args {
            let wire = value_to_wire(arg).map_err(|kind| unsupported_wire_value_err(kind, span))?;
            wire_args.push(wire);
        }

        let (tx, rx) = mpsc::channel();
        let status = self
            .tsfn
            .call((wire_args, tx), ThreadsafeFunctionCallMode::Blocking);
        if status != Status::Ok {
            return Err(string_err_to_sass(
                format!("failed to schedule JS callback: {status:?}"),
                span,
            ));
        }

        match rx.recv() {
            Ok(Ok(wire)) => Ok(wire_to_sass(wire)),
            Ok(Err(msg)) => Err(string_err_to_sass(msg, span)),
            // The JS-thread callback dropped `tx` without sending — this
            // would only happen if the threadsafe function itself was
            // aborted/released mid-call (e.g. process teardown racing this
            // compile), not a normal error path.
            Err(_) => Err(string_err_to_sass(
                "the JS callback thread was dropped before responding".to_owned(),
                span,
            )),
        }
    }
}

impl JsFunctionRef {
    /// Upgrades this reference into a [`ThreadsafeFunction`]-backed handle
    /// callable from `Task::compute()` (off the JS thread) — todo #221
    /// slice 3. Must be called on the JS thread that owns `self.env`, same
    /// as everywhere else this type is touched; the intended call site is
    /// `compile_async`/`compile_string_async`'s synchronous body, before an
    /// `AsyncTask` is constructed (see `lib.rs`'s `take_async_functions`).
    ///
    /// See this module's doc comment for the full design: a dummy no-op JS
    /// function is registered as the `ThreadsafeFunction`'s nominal target
    /// (required by the napi API, but never meaningfully invoked) — the
    /// *real* call happens by hand, inside the threadsafe callback itself,
    /// using `self`'s `Ref<JsFunction>` (still valid there: the callback
    /// runs on the JS thread, the same invariant `JsFunctionRef` already
    /// relies on for the sync path).
    pub(crate) fn into_threadsafe(self) -> Result<AsyncJsFunctionRef> {
        let outer_env = self.env.0;
        let noop = outer_env.create_function("grass_async_fn_target", noop_callback)?;

        let tsfn = noop
            .create_threadsafe_function::<ThreadsafeCallArgs, JsUnknown, _, ErrorStrategy::Fatal>(
                0,
                move |ctx: ThreadSafeCallContext<ThreadsafeCallArgs>| -> Result<Vec<JsUnknown>> {
                    let (wire_args, tx) = ctx.value;
                    let env = ctx.env;

                    let outcome: std::result::Result<WireValue, String> = (|| {
                        let func = env
                            .get_reference_value::<JsFunction>(&self.func_ref)
                            .map_err(|e| e.reason.clone())?;

                        let mut js_args = Vec::with_capacity(wire_args.len());
                        for w in wire_args {
                            let v = wire_to_sass(w);
                            js_args.push(sass_value_to_js(env, &v).map_err(|e| e.reason.clone())?);
                        }
                        // Same single-array-argument convention as the sync path
                        // — see `JsFunctionRef::call`'s comment.
                        let args_array = crate::values::to_unknown(env, js_args)
                            .map_err(|e| e.reason.clone())?;

                        match func.call(None, &[args_array]) {
                            Ok(js_return) => {
                                // Must be checked BEFORE js_value_to_sass, which
                                // would otherwise reject a Promise with a generic
                                // "unsupported shape" error instead of this
                                // specific, documented one.
                                if js_return.is_promise().unwrap_or(false) {
                                    return Err(PROMISE_RETURN_ERR.to_owned());
                                }

                                let v = js_value_to_sass(env, js_return)
                                    .map_err(|e| e.reason.clone())?;
                                value_to_wire(&v).map_err(unsupported_wire_value_message)
                            }
                            Err(e) => Err(e.reason.clone()),
                        }
                    })();

                    // Always send SOMETHING and always return Ok(..) — the
                    // worker thread's `rx.recv()` must never be left hanging,
                    // and letting this closure return Err would route through
                    // napi's fatal-error/process-abort path (see module doc).
                    let _ = tx.send(outcome);
                    Ok(Vec::new())
                },
            )?;

        Ok(AsyncJsFunctionRef { tsfn })
    }
}

fn unsupported_wire_value_message(kind: &str) -> String {
    format!(
        "grass's `functions` bridge does not yet support Sass {kind} values (todo #221 slice \
         2/3). Supported types: booleans, null, numbers, strings, lists."
    )
}

fn unsupported_wire_value_err(kind: &str, span: Span) -> Box<grass_compiler::Error> {
    string_err_to_sass(unsupported_wire_value_message(kind), span)
}

fn string_err_to_sass(msg: String, span: Span) -> Box<grass_compiler::Error> {
    (msg, span).into()
}

/// A JS function that does nothing, used as the nominal call target every
/// `ThreadsafeFunction` the async bridge creates must be constructed with
/// (napi's API requires *some* JS function). See the module doc comment for
/// why the real call happens by hand instead of relying on the framework's
/// own auto-invocation of this target.
///
/// `pub(crate)` because `importers.rs`'s async `Importer`/`FileImporter`
/// bridge (todo #221 slice 5b) needs the exact same dummy-target trick for
/// its own `ThreadsafeFunction`s and reuses this rather than duplicating it.
pub(crate) unsafe extern "C" fn noop_callback(
    env: sys::napi_env,
    _info: sys::napi_callback_info,
) -> sys::napi_value {
    let mut undefined = std::ptr::null_mut();
    unsafe {
        sys::napi_get_undefined(env, &mut undefined);
    }
    undefined
}

/// Registers `functions` (signature string -> JS callback) onto `options`
/// for the ASYNCHRONOUS entry points (`compileAsync`/`compileStringAsync`),
/// todo #221 slice 3. `functions` must already be upgraded via
/// [`JsFunctionRef::into_threadsafe`] (a JS-thread-only step, performed in
/// `compile_async`/`compile_string_async`'s synchronous body before the
/// `AsyncTask` is constructed — see `lib.rs`'s `take_async_functions`). This
/// function itself touches no `Env` and is safe to call from
/// `Task::compute()` (off the JS thread), matching where it's actually used.
pub fn register_functions_async(
    mut options: Options<'static>,
    functions: HashMap<String, AsyncJsFunctionRef>,
) -> Result<Options<'static>> {
    for (signature, func_ref) in functions {
        let handle = Arc::new(func_ref);

        options = options
            .add_custom_fn_with_signature(
                signature,
                move |mut args: ArgumentResult, _: &mut Visitor| {
                    let span = args.span();

                    let mut sass_args = Vec::new();
                    let mut i = 0;
                    while let Some(spanned) = args.get_positional(i) {
                        sass_args.push(spanned.node);
                        i += 1;
                    }

                    handle.call(&sass_args, span)
                },
            )
            .map_err(|e| Error::from_reason(e.to_string()))?;
    }

    Ok(options)
}
