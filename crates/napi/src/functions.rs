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
//! `compileAsync`/`compileStringAsync` do **not** wire this up — see
//! `register_functions`'s caller in `lib.rs`, which rejects `functions`
//! for those two entry points with a runtime error before an
//! `AsyncTask`/`Task::compute()` (which runs off the JS thread, with no
//! `Env` at all) is ever constructed. That keeps the invariant airtight:
//! nothing capturing a `SyncEnv`/`JsFunctionRef` is ever handed to a worker
//! thread. Slice 3 (async functions bridge) needs a different mechanism
//! entirely — `ThreadsafeFunction` + a blocking channel round-trip, per the
//! design doc (`docs/design/js-api-functions-importers.md` §4.2) — since
//! `compute()` genuinely runs off-thread there.
//!
//! Considered and rejected: `ThreadsafeFunction` even for the sync path
//! (works, but adds queueing/dispatch overhead for a call that's already on
//! the right thread — pure cost, no benefit); napi-rs's newer
//! `bindgen_runtime::Reference<T>` (that's for `#[napi]` *class instances*
//! wrapping Rust data, not for holding a plain `JsFunction` — not the right
//! tool here).

use std::collections::HashMap;
use std::sync::Arc;

use napi::bindgen_prelude::FromNapiValue;
use napi::{sys, Env, Error, JsFunction, NapiValue, Ref, Result};

use grass_compiler::codemap::Span;
use grass_compiler::sass_value::{ArgumentResult, Value};
use grass_compiler::{Options, Result as SassResult, Visitor};

use crate::values::{js_value_to_sass, sass_value_to_js};

/// # Safety
///
/// See this module's doc comment: a `SyncEnv` is only ever read back within
/// the same synchronous `compile`/`compileString` call that constructed it,
/// which never leaves the JS thread. It must never be stored anywhere that
/// could outlive that call (in particular, never inside an `AsyncTask`/
/// `Task`).
struct SyncEnv(Env);

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
            .add_custom_fn_with_signature(signature, move |mut args: ArgumentResult, _: &mut Visitor| {
                let span = args.span();

                let mut sass_args = Vec::new();
                let mut i = 0;
                while let Some(spanned) = args.get_positional(i) {
                    sass_args.push(spanned.node);
                    i += 1;
                }

                handle.call(&sass_args, span)
            })
            .map_err(|e| Error::from_reason(e.to_string()))?;
    }

    Ok(options)
}
