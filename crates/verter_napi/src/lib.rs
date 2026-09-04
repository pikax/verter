// NAPI-RS generates variables from camelCase struct fields — suppress warnings.
#![allow(non_snake_case)]

//! # verter_napi — Node.js bindings for Verter
//!
//! NAPI-RS binding layer that exposes [`verter_session::VerterHost`] and
//! the standalone CSS style-processing entry points to Node.js.
//!
//! ## Host API
//!
//! This crate shares the core `VerterHost` API with [`verter_wasm`] and adds
//! native-only batch and CSS entry points. The CSS routes require Node.js:
//!
//! - **`prepareStyleForPreprocessor`** — rewrites `v-bind()` in AUTHORED
//!   style content before it is handed to an external SCSS/Less/Stylus
//!   preprocessor.
//! - **`transformVueStyle`** — applies the v-bind + CSS-Modules +
//!   scoped-selector cascade to CSS the caller already treats as plain.
//! - **`analyzeStyle`** — read-only style fact extraction (static + CSS
//!   Modules class names), no rewrite.
//!
//! ## Performance
//!
//! Uses `#[napi(object)]` structs for zero-copy V8 ↔ Rust transfer.
//! All panics are caught via [`catch_panic`] to prevent Node.js crashes.
//!
//! ## FFI architecture
//!
//! NAPI structs use camelCase field names matching the JS API convention.
//! They map to/from `verter_ffi` types via zero-copy `From` impls
//! (field-by-field moves, no serialization). The shared conversion logic
//! in `verter_ffi::convert` handles the FFI ↔ host type mapping.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_ffi::convert::*;
use verter_ffi::types::*;
use verter_session as host;
use verter_type_expr::TypeExpr;

mod audit;
mod compile_request_response;
mod host_compile_request;
#[cfg(test)]
mod host_compile_request_tests;
pub mod host_compile_request_ts;
mod js_value_graph;
mod memory_audit;
mod meta;
mod typeinfo;

pub use host_compile_request::{
    decode_host_compile_request, napi_host_compile_request_to_ffi, NapiHostCompileRequest,
    NapiRequestedProduct,
};
// Reachable so the boundary suites can drive materialisation over a
// modelled graph. Not part of the addon's JS surface.
pub use compile_request_response::compile_request_failure_to_napi;
use compile_request_response::{
    binding_failure_entry, binding_failure_to_napi, compile_request_construction_refused,
    compile_request_error, compile_request_failure_status, compile_request_response_to_napi,
    failure_canonical_id, host_diagnostic_to_napi, host_diagnostics_to_napi, host_ide_to_napi,
};
#[doc(hidden)]
pub use js_value_graph::{
    materialize_js_value, materialize_js_value_with_budget, JsObjectKeys, JsValueClass,
    JsValueGraph, JsValueMaterializationBudget, MAX_ARRAY_ELEMENTS, MAX_DECODED_VALUES_PER_REQUEST,
    MAX_NESTING_DEPTH, MAX_RETAINED_BYTES_PER_REQUEST, MIN_VALUE_RETAINED_BYTES,
};

/// Maximum native payload bytes retained while decoding one typed batch,
/// covering every entry's canonical id, source bytes and request graph.
///
/// Aggregate over the whole call: the counter never resets between
/// entries, so once it is exhausted every LATER entry is refused too. It
/// is reported per ENTRY all the same — at the position that crossed it
/// and at each one after — because the alternative loses every earlier
/// sibling's already-decoded work and names no input, leaving a caller
/// with no way to tell which entry pushed the batch over. Entries that
/// decoded before the ceiling was reached still compile and still answer.
///
/// The ceiling is fixed here, with no runtime override; `docs/api/native.md`
/// states it so callers size their batches rather than discovering it.
///
/// Contrast [`MAX_DECODED_VALUES_PER_REQUEST`], which is per ENTRY (the
/// batch resets the counter between entries) and therefore refuses only
/// the entry that exhausted it, leaving every later entry free to decode.
const MAX_COMPILE_REQUEST_BATCH_RETAINED_BYTES: usize = 64 * 1024 * 1024;

// Re-imports for code actions and diagnostics (parity with verter_wasm)
use verter_actions::{ActionContext, ActionEngine};
use verter_diagnostics::rules::RuleRegistry;
use verter_diagnostics::Linter;

/// Run a closure, converting any panic into a napi::Error.
/// Prevents Rust panics from crashing the Node.js process.
fn catch_panic<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> Result<T> {
    // Every `#[napi]` entry body runs inside this wrapper, so one scope here
    // covers every native boundary crossing without per-entry annotation.
    verter_audit::attribute_scope!(NapiBoundaryCall);
    std::panic::catch_unwind(f).map_err(|panic_info| {
        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown internal error".to_string()
        };
        Error::new(
            Status::GenericFailure,
            format!("internal compiler error: {msg}"),
        )
    })
}

pub(crate) fn ffi_err(msg: impl std::fmt::Display) -> Error {
    Error::new(Status::InvalidArg, msg.to_string())
}

fn clear_pending_exception(env: &Env) -> Result<()> {
    let mut pending = false;
    let status = unsafe { napi::sys::napi_is_exception_pending(env.raw(), &mut pending) };
    if status != napi::sys::Status::napi_ok {
        return Err(Error::new(
            Status::GenericFailure,
            "failed to inspect a pending JavaScript exception",
        ));
    }
    if !pending {
        return Ok(());
    }
    let mut exception = std::ptr::null_mut();
    let status = unsafe { napi::sys::napi_get_and_clear_last_exception(env.raw(), &mut exception) };
    if status != napi::sys::Status::napi_ok {
        return Err(Error::new(
            Status::GenericFailure,
            "failed to clear a pending JavaScript exception",
        ));
    }
    Ok(())
}

/// Bytes of a thrown value this binding is willing to copy into a Rust
/// error message.
///
/// A thrown string is caller-controlled and unbounded; the message only
/// has to be readable, so a hostile multi-megabyte throw is answered by
/// the generic label rather than retained.
const MAX_EXCEPTION_MESSAGE_BYTES: usize = 8 * 1024;

/// Read an exception's message without invoking user-controlled string
/// coercion. `Error::from(Unknown)` would run `toString` / `toPrimitive`,
/// and a throw there would leave a new pending exception that poisons the
/// next sibling's N-API call.
fn exception_message_without_coercion(
    env: &Env,
    exception: napi::sys::napi_value,
) -> Result<String> {
    let ty = napi::type_of!(env.raw(), exception)?;
    if ty == ValueType::String {
        // Measured before it is copied, and the measurement is a BOUND:
        // discarding it would leave a probe that reads like a guard and
        // enforces nothing.
        if js_value_graph::napi_utf8_string_len(env.raw(), exception)? > MAX_EXCEPTION_MESSAGE_BYTES
        {
            return Ok("JavaScript exception".to_string());
        }
        return unsafe { String::from_napi_value(env.raw(), exception) };
    }
    if ty != ValueType::Object {
        return Ok("JavaScript exception".to_string());
    }

    let mut message = std::ptr::null_mut();
    let status = unsafe {
        napi::sys::napi_get_named_property(env.raw(), exception, c"message".as_ptr(), &mut message)
    };
    if status != napi::sys::Status::napi_ok {
        clear_pending_exception(env)?;
        return Ok("JavaScript exception".to_string());
    }
    if napi::type_of!(env.raw(), message)? != ValueType::String {
        return Ok("JavaScript exception".to_string());
    }
    // Same bound as the thrown-string path: `error.message` is equally
    // caller-controlled and equally unbounded.
    if js_value_graph::napi_utf8_string_len(env.raw(), message)? > MAX_EXCEPTION_MESSAGE_BYTES {
        return Ok("JavaScript exception".to_string());
    }
    unsafe { String::from_napi_value(env.raw(), message) }
}

/// One engine-side handle scope, closed when it drops.
///
/// A native call runs in ONE scope, so every handle a batch's traversal
/// creates — a key name, a looked-up property, a decoded element — stays
/// pinned until the whole call returns. The per-call byte and value
/// budgets bound what the traversal RETAINS natively; they say nothing
/// about the engine-side handles it pins, and a batch admitted by those
/// budgets can still pin millions of them at once. Opening a nested scope
/// per batch entry bounds that to one entry's worth: the entry's handles
/// are released as soon as its own decode has produced owned Rust values.
///
/// Nothing engine-side may outlive the guard. Every value a batch entry's
/// decode produces — the source `Arc<str>`, the canonical id, the decoded
/// request, a failure's message — is owned Rust, so the whole entry is
/// self-contained by construction. Dropping (rather than closing
/// explicitly) is what makes the early-`continue` and early-`return` paths
/// safe, and closing on drop keeps the required LIFO nesting.
struct JsHandleScope {
    env: napi::sys::napi_env,
    scope: napi::sys::napi_handle_scope,
}

impl JsHandleScope {
    fn open(env: &Env) -> Result<Self> {
        let mut scope = std::ptr::null_mut();
        napi::check_status!(
            unsafe { napi::sys::napi_open_handle_scope(env.raw(), &mut scope) },
            "Failed to open a handle scope for a compile request batch entry"
        )?;
        Ok(Self {
            env: env.raw(),
            scope,
        })
    }
}

impl Drop for JsHandleScope {
    fn drop(&mut self) {
        // SAFETY: opened on this env by `open`, and closed exactly once —
        // guards are held in nested lexical scopes, so closes are LIFO.
        unsafe {
            napi::sys::napi_close_handle_scope(self.env, self.scope);
        }
    }
}

/// Capture and clear a JavaScript exception left pending by a recoverable
/// accessor failure. Continuing with it pending makes every later N-API call
/// fail, defeating per-entry batch isolation.
///
/// The recovered error is re-tagged off [`Status::PendingException`]. The
/// exception is no longer pending — this function just cleared it — and an
/// error still CLAIMING to be one is dropped rather than thrown:
/// `JsError::throw_into` returns early on that status, so a route that
/// returns the recovered error to JavaScript would resolve to `undefined`
/// with no error at all. A batch entry that folds the error into its own
/// `failure` slot never noticed; a route that throws does.
fn recover_pending_exception(env: &Env, fallback: Error) -> Result<Error> {
    let throwable_status = |status: Status| match status {
        Status::PendingException => Status::GenericFailure,
        other => other,
    };
    let mut pending = false;
    let status = unsafe { napi::sys::napi_is_exception_pending(env.raw(), &mut pending) };
    if status != napi::sys::Status::napi_ok {
        return Err(Error::new(
            Status::GenericFailure,
            "failed to inspect a pending JavaScript exception",
        ));
    }
    if !pending {
        return Ok(Error::new(
            throwable_status(fallback.status),
            fallback.reason.clone(),
        ));
    }

    let mut exception = std::ptr::null_mut();
    let status = unsafe { napi::sys::napi_get_and_clear_last_exception(env.raw(), &mut exception) };
    if status != napi::sys::Status::napi_ok {
        return Err(Error::new(
            Status::GenericFailure,
            "failed to clear a pending JavaScript exception",
        ));
    }
    let message = match exception_message_without_coercion(env, exception) {
        Ok(message) if !message.is_empty() => message,
        _ => {
            clear_pending_exception(env)?;
            fallback.reason.clone()
        }
    };
    Ok(Error::new(throwable_status(fallback.status), message))
}

/// Measure a JavaScript string before converting it into an owned Rust value.
fn js_string_utf8_len(env: &Env, value: napi::sys::napi_value) -> Result<usize> {
    if napi::type_of!(env.raw(), value)? != ValueType::String {
        return Err(Error::new(Status::StringExpected, "expected a string"));
    }
    js_value_graph::napi_utf8_string_len(env.raw(), value)
}

fn napi_value_is_array(env: &Env, value: napi::sys::napi_value) -> Result<bool> {
    let mut is_array = false;
    napi::check_status!(
        unsafe { napi::sys::napi_is_array(env.raw(), value, &mut is_array) },
        "Failed to detect whether a value is an array"
    )?;
    Ok(is_array)
}

fn napi_value_is_buffer(env: &Env, value: napi::sys::napi_value) -> Result<bool> {
    let mut is_buffer = false;
    napi::check_status!(
        unsafe { napi::sys::napi_is_buffer(env.raw(), value, &mut is_buffer) },
        "Failed to detect whether a value is a Buffer"
    )?;
    Ok(is_buffer)
}

fn decode_compile_requests_priority(
    env: &Env,
    options: Option<Unknown<'_>>,
) -> Result<Option<verter_scheduler::stage::Priority>> {
    use js_value_graph::{JsObjectKeys, JsValueClass, JsValueGraph, NapiValueGraph};
    use verter_scheduler::stage::Priority;

    let Some(value) = options else {
        return Ok(Some(Priority::Background));
    };
    let ty = match value.get_type() {
        Ok(ty) => ty,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    if ty == ValueType::Undefined || ty == ValueType::Null {
        return Ok(Some(Priority::Background));
    }
    // SAFETY: the options value belongs to this live env.
    let graph = unsafe { NapiValueGraph::new(env.raw()) };
    // The same classification the request graph uses, for the same
    // reason: a Buffer, typed array or DataView exposes every byte index
    // as an enumerable own key, so enumerating one would materialise
    // millions of V8 key strings before any count could refuse them.
    // `napi_is_array` alone does not see those, and this argument is the
    // only enumerated one on the route that is not already an array.
    match graph.classify(&value.raw()) {
        Ok(JsValueClass::Object) => {}
        Ok(_) => return Err(ffi_err("compile request batch options must be an object")),
        Err(error) => {
            let error = recover_pending_exception(env, error)?;
            return Err(error);
        }
    }
    let keys = match graph.own_enumerable_keys(&value.raw()) {
        Ok(keys) => keys,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    let count = match keys.count() {
        Ok(count) => count,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    if count > MAX_ARRAY_ELEMENTS {
        return Err(ffi_err(format!(
            "A request object exposes {count} keys, above the \
             {MAX_ARRAY_ELEMENTS} a request may carry"
        )));
    }
    // Which own key states `priority`, if any. Absence is the default,
    // never a prototype lookup.
    let mut stated: Option<u32> = None;
    for index in 0..count {
        // Measured before it is copied, the same bound
        // `exception_message_without_coercion` applies to a thrown string:
        // an own key is caller-controlled and unbounded, and a refusal
        // only has to be readable. Above the bound the key is named by its
        // size rather than quoted, so a hostile multi-megabyte key cannot
        // ride into a thrown Error's message.
        let key_bytes = match keys.retained_bytes(index) {
            Ok(bytes) => bytes,
            Err(error) => return Err(recover_pending_exception(env, error)?),
        };
        if key_bytes > MAX_EXCEPTION_MESSAGE_BYTES {
            return Err(ffi_err(format!(
                "unknown field, named by {key_bytes} bytes above the \
                 {MAX_EXCEPTION_MESSAGE_BYTES} a refusal quotes"
            )));
        }
        let key = match keys.at(index) {
            Ok(key) => key,
            Err(error) => return Err(recover_pending_exception(env, error)?),
        };
        if key != "priority" {
            return Err(ffi_err(format!("unknown field `{key}`")));
        }
        stated = Some(index);
    }
    // The OWN-key gate is what makes an inherited `priority` inert: the
    // value is read only when an own enumerable key states it, so
    // `Object.create({ priority: "interactive" })` — zero own keys — takes
    // the default and never consults the prototype. Removing this gate is
    // what would let a caller select a priority they never wrote on this
    // object; the reader below cannot restore it, because every property
    // read at this boundary (`property_at` and `Object::get` alike) is
    // `napi_get_property`, a full `[[Get]]`, and an own property shadows
    // its prototype's anyway.
    //
    // `property_at` is chosen on its own merit: the key list already holds
    // a handle to the name, so looking the property up through it costs no
    // second engine-side string.
    //
    // One rule at one boundary: a payload is its own properties, for the
    // key list AND for the value behind it.
    let Some(index) = stated else {
        return Ok(Some(Priority::Background));
    };
    let raw = match graph.property_at(&value.raw(), &keys, index) {
        Ok(raw) => raw,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    let ty = match napi::type_of!(env.raw(), raw) {
        Ok(ty) => ty,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    if ty == ValueType::Undefined {
        return Ok(Some(Priority::Background));
    }
    if ty != ValueType::String {
        return Err(ffi_err(
            "invalid priority, expected 'interactive' or 'background'",
        ));
    }
    let priority = match unsafe { String::from_napi_value(env.raw(), raw) } {
        Ok(priority) => priority,
        Err(error) => return Err(recover_pending_exception(env, error)?),
    };
    match priority.as_str() {
        "background" => Ok(Some(Priority::Background)),
        "interactive" => Ok(Some(Priority::Interactive)),
        other => Err(ffi_err(format!(
            "invalid priority '{other}', expected 'interactive' or 'background'"
        ))),
    }
}

/// Answers a diagnostic message when `entries` (as returned by
/// [`verter_session::VerterHost::compile_request_many`], one per input in
/// original order) does not hold one entry per input. `None` when it does.
///
/// A count mismatch is the one shape that cannot be attributed to a
/// position: the zip that fills the caller's output slots truncates to the
/// shorter side, so a dropped or duplicated entry would leave a slot
/// silently unfilled and every later entry paired with the wrong input's
/// source. There is no entry to blame, so the whole call fails loudly.
///
/// A per-POSITION mismatch is attributable and is handled where the
/// pairing happens, by [`batch_entry_position_mismatch`] — it fails that
/// entry rather than discarding every sibling's compiled output.
fn batch_entry_count_mismatch(
    entries: &[host_compile::CompileRequestBatchEntry],
    expected_canonical_ids: &[String],
) -> Option<String> {
    (entries.len() != expected_canonical_ids.len()).then(|| {
        format!(
            "typed compile batch returned {} entries for {} inputs",
            entries.len(),
            expected_canonical_ids.len()
        )
    })
}

/// The refusal one entry carries when the executor answered a different
/// canonical id at its position than the input there asked for.
///
/// `expected` is HOST-canonical — resolved through
/// `VerterHost::resolve_alias_or_canonical`, exactly as the executor
/// resolves its own inputs — so a non-canonical input (a Windows path, a
/// registered alias, a `?`-suffixed id) is not read as a transposition.
///
/// The executor's own contract already guarantees one entry per input in
/// order; this is the binding's check that the guarantee held, because a
/// reordered batch would otherwise pair a response and its diagnostics
/// with the WRONG entry's source text — and the `ideCompanion` product's
/// offsets are computed from that text, so the mispairing would publish
/// silently rather than fail. Failing the affected position keeps that
/// impossible without discarding the batch: a sibling whose id did land
/// where its input asked is still paired with its own source.
fn batch_entry_position_mismatch(answered: &str, expected: &str) -> String {
    format!(
        "typed compile batch returned entry '{answered}' at the position expected for '{expected}'"
    )
}

/// The longest name a batch entry declares (`canonicalId`).
///
/// An own key longer than this cannot be one of the three, so it is
/// skipped by its MEASURED size and never copied into Rust — the same
/// treatment the batch options give a hostile key.
const MAX_BATCH_ENTRY_FIELD_NAME_BYTES: usize = "canonicalId".len();

/// One batch entry's three declared fields, as OWN enumerable properties.
///
/// A field whose value is STATED as `undefined` reads as absent, exactly
/// as it does through `Object::get`, so an entry that spells a field
/// `undefined` is missing it rather than holding an invalid value.
struct BatchEntryFields {
    canonical_id: Option<napi::sys::napi_value>,
    source: Option<napi::sys::napi_value>,
    request: Option<napi::sys::napi_value>,
}

/// Reads one batch entry's three fields from its OWN enumerable
/// properties.
///
/// A payload is its own properties — the rule the request graph and the
/// batch options already hold, applied to the wrapper that carries them.
/// `Object::get` is `napi_get_property`, a full `[[Get]]`, so reading the
/// wrapper that way would accept
/// `compileRequests([Object.create({ canonicalId, source, request })])`:
/// an entry whose every field the caller never wrote on the object they
/// handed over. Enumerating the own keys instead makes an inherited field
/// absent, which the caller sees as the missing-field refusal.
///
/// The entry is CLASSIFIED before its keys are enumerated, for the reason
/// the batch options are: a Buffer, typed array or DataView exposes every
/// byte index as an enumerable own key, so enumerating one would
/// materialise a V8 key string per byte before any count could refuse
/// them.
fn read_batch_entry_fields(
    env: &Env,
    graph: &js_value_graph::NapiValueGraph,
    input: &Object<'_>,
) -> Result<BatchEntryFields> {
    use js_value_graph::{JsObjectKeys, JsValueClass, JsValueGraph};

    if graph.classify(&input.raw())? != JsValueClass::Object {
        return Err(ffi_err("compile request batch input must be an object"));
    }
    let keys = graph.own_enumerable_keys(&input.raw())?;
    let count = keys.count()?;
    if count > MAX_ARRAY_ELEMENTS {
        return Err(ffi_err(format!(
            "A compile request batch input exposes {count} keys, above the \
             {MAX_ARRAY_ELEMENTS} a request may carry"
        )));
    }
    let mut fields = BatchEntryFields {
        canonical_id: None,
        source: None,
        request: None,
    };
    for index in 0..count {
        if keys.retained_bytes(index)? > MAX_BATCH_ENTRY_FIELD_NAME_BYTES {
            continue;
        }
        let slot = match keys.at(index)?.as_str() {
            "canonicalId" => &mut fields.canonical_id,
            "source" => &mut fields.source,
            "request" => &mut fields.request,
            _ => continue,
        };
        let raw = graph.property_at(&input.raw(), &keys, index)?;
        if napi::type_of!(env.raw(), raw)? != ValueType::Undefined {
            *slot = Some(raw);
        }
    }
    Ok(fields)
}

/// The refusal every entry from `position` onwards carries once the
/// call's AGGREGATE retained-byte ceiling is exhausted.
///
/// Exhaustion is a call-wide state — the counter never resets, so every
/// later entry hits it too — but it is reported per ENTRY, at the position
/// that crossed it and at each one after. Aborting the call instead would
/// discard every sibling's already-decoded work and name no input, leaving
/// a caller with no way to tell which entry pushed the batch over.
fn batch_retained_bytes_refusal(position: usize, reason: &str) -> String {
    format!(
        "compile request batch input at index {position}: {reason}. That ceiling \
         is aggregate over the whole call, so every later entry refuses too; \
         compile fewer inputs per call."
    )
}

fn read_batch_source_buffer(
    env: &Env,
    raw: napi::sys::napi_value,
    budget: &mut JsValueMaterializationBudget,
) -> Result<std::sync::Arc<str>> {
    if !napi_value_is_buffer(env, raw)? {
        return Err(Error::new(
            Status::InvalidArg,
            "compile request batch `source` must be a Buffer",
        ));
    }
    let mut data = std::ptr::null_mut();
    let mut len = 0usize;
    napi::check_status!(
        unsafe { napi::sys::napi_get_buffer_info(env.raw(), raw, &mut data, &mut len) },
        "Failed to read compile request batch source length"
    )?;
    budget.retain_bytes(len)?;
    // A zero-length Buffer may report a NULL data pointer, and
    // `slice::from_raw_parts` requires a non-null aligned pointer even for
    // a zero length — so the empty case never builds a slice at all.
    let bytes: &[u8] = if len == 0 {
        &[]
    } else {
        // SAFETY: `napi_is_buffer` succeeded and `napi_get_buffer_info`
        // filled a live pointer/length pair for this env; the copy below is
        // retained before the JS value can be collected.
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) }
    };
    // One copy, not two: validating into `&str` and then interning the
    // `Arc<str>` from that borrow copies the payload once, where
    // `String::from_utf8(bytes.to_vec())` followed by `Arc::from(String)`
    // copies it twice — a whole extra batch-sized transient allocation on
    // the JS thread.
    std::str::from_utf8(bytes)
        .map(std::sync::Arc::<str>::from)
        .map_err(|error| {
            Error::new(
                Status::InvalidArg,
                format!("Buffer is not valid UTF-8: {error}"),
            )
        })
}

/// Convert a `Buffer` (raw bytes) to a `String`, validating UTF-8.
fn buffer_to_string(buf: Buffer) -> Result<String> {
    String::from_utf8(buf.into()).map_err(|e| {
        Error::new(
            Status::InvalidArg,
            format!("Buffer is not valid UTF-8: {e}"),
        )
    })
}

pub(crate) fn host_error_status(err: &host::HostError) -> Status {
    match err {
        host::HostError::InvalidQuery
        | host::HostError::MissingSource { .. }
        | host::HostError::MissingVirtualNode { .. } => Status::InvalidArg,
        // Typed unsupported-language failure: the request named a
        // language row with no registered implementation — same status
        // family as the classify errors (the caller's input names a
        // language the host cannot serve), distinguishable from a
        // generic internal failure.
        host::HostError::Scheduler(
            verter_scheduler::job::SchedulerError::UnsupportedLanguage { .. },
        ) => Status::InvalidArg,
        host::HostError::CompileError(_) => Status::GenericFailure,
        #[allow(unreachable_patterns)]
        _ => Status::GenericFailure,
    }
}

fn host_error(err: host::HostError) -> Error {
    Error::new(host_error_status(&err), host_error_to_string(&err))
}

// =============================================================================
// Standalone CSS Style Processing (NAPI-only)
//
// Available in NAPI but not WASM because CSS preprocessing (LESS/SCSS/Stylus)
// requires Node.js. The WASM host processes styles inline during compilation.
//
// Three-way explicit-boundary CSS API:
// - `prepareStyleForPreprocessor` — AUTHORED-dialect v-bind rewrite, before
//   an external SCSS/Less/Stylus preprocessor.
// - `transformVueStyle` — v-bind + CSS-Modules + scoped-selector cascade
//   over CSS the caller already treats as plain.
// - `analyzeStyle` — read-only fact extraction, no rewrite.
// =============================================================================

fn css_dialect(value: Option<&str>) -> Result<verter_css_syntax::CssDialect> {
    match value {
        None => Ok(verter_css_syntax::CssDialect::Css),
        Some(lang) => verter_css_syntax::CssDialect::from_lang(lang).ok_or_else(|| {
            // Listed from the owner's own table so the message cannot drift
            // from what the call actually accepts.
            let expected = verter_css_syntax::CssDialect::LANG_SPELLINGS
                .map(|(spelling, _)| spelling)
                .join(", ");
            Error::new(
                Status::InvalidArg,
                format!("unknown CSS dialect {lang:?}; expected one of: {expected}"),
            )
        }),
    }
}

fn style_rewrite_failure_error(
    failure: verter_compiler::style_planner::StyleRewriteFailure,
) -> Error {
    Error::new(Status::GenericFailure, failure.to_string())
}

#[napi(object)]
pub struct VueStyleVBind {
    /// The original expression text (e.g., "color" or "theme.color")
    pub expression: String,
    /// The generated CSS variable name (e.g., "--a4f2eed6-color")
    pub varName: String,
}

fn to_napi_v_bind_vars(vars: Vec<verter_compiler::style_planner::VBindVar>) -> Vec<VueStyleVBind> {
    vars.into_iter()
        .map(|var| VueStyleVBind {
            expression: var.expression,
            varName: var.var_name,
        })
        .collect()
}

#[napi(object)]
#[derive(Default)]
pub struct PrepareStyleForPreprocessorOptions {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scopeId: String,
    /// Authored dialect: "css" (default) | "scss" | "sass" | "less" | "stylus".
    pub dialect: Option<String>,
    pub filename: Option<String>,
}

#[napi(object)]
pub struct PrepareStyleForPreprocessorResult {
    /// Authored code with `v-bind()` rewritten to `var(--scope-hash)`.
    pub code: String,
    /// v-bind() expressions found and replaced.
    pub vBindVars: Vec<VueStyleVBind>,
}

/// Rewrite `v-bind()` in AUTHORED (possibly non-CSS) style content, before
/// handing it to an external SCSS/Less/Stylus preprocessor.
#[napi]
pub fn prepare_style_for_preprocessor(
    css: Buffer,
    options: PrepareStyleForPreprocessorOptions,
) -> Result<PrepareStyleForPreprocessorResult> {
    let css = buffer_to_string(css)?;
    catch_panic(std::panic::AssertUnwindSafe(|| {
        let filename = options.filename.as_deref().unwrap_or("style");
        let input = verter_compiler::style_planner::AuthoredStyleInput::new(
            &css,
            css_dialect(options.dialect.as_deref())?,
            filename,
            filename,
            filename,
        )
        .without_source_map();
        verter_compiler::style_planner::transform_vue_v_bind(input, &options.scopeId)
            .map_err(style_rewrite_failure_error)
    }))?
    .map(|outcome| match outcome {
        verter_compiler::style_planner::StyleRewriteOutcome::Rewritten { code, facts, .. } => {
            PrepareStyleForPreprocessorResult {
                code,
                vBindVars: to_napi_v_bind_vars(facts.v_bind_vars),
            }
        }
        verter_compiler::style_planner::StyleRewriteOutcome::Unchanged { facts } => {
            PrepareStyleForPreprocessorResult {
                code: css,
                vBindVars: to_napi_v_bind_vars(facts.v_bind_vars),
            }
        }
    })
}

#[napi(object)]
#[derive(Default)]
pub struct TransformVueStyleOptions {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scopeId: String,
    /// Whether this style block is scoped
    pub scoped: Option<bool>,
    /// Whether this is a CSS module block
    pub isModule: Option<bool>,
    /// Custom module name (None = "$style")
    pub moduleName: Option<String>,
    /// Source filename for source map generation
    pub filename: Option<String>,
    /// Whether to generate a source map
    pub sourcemap: Option<bool>,
}

#[napi(object)]
pub struct TransformVueStyleResult {
    /// Transformed CSS code
    pub code: String,
    /// Source map as JSON string (only when `sourcemap: true` was requested).
    pub sourceMap: Option<String>,
    /// CSS module class mappings (original → hashed), each entry is [original, hashed]
    pub moduleClasses: Vec<Vec<String>>,
    /// CSS module variable name (e.g. "$style" or custom name from `<style module="...">`)
    pub moduleName: Option<String>,
    /// v-bind() expressions found and replaced
    pub vBindVars: Vec<VueStyleVBind>,
    /// Every refusal the cascade published for this call and still returned
    /// code for: the per-selector soft refusals (`code` published minus the
    /// untrustworthy rule each entry names) plus any stage failure
    /// `cascade_output_is_publishable` deliberately does NOT refuse the whole
    /// call over. Empty on every ordinary successful transform.
    ///
    /// Read from the cascade's single publication route
    /// (`outcome.result.diagnostics()`), not re-derived from its own record of
    /// what each authority reported — re-deriving would format every refusal a
    /// second time and is how the same refusal ends up reported twice.
    pub refusals: Vec<String>,
}

/// Run Vue's v-bind + CSS-Modules + scoped-selector cascade over CSS the
/// caller already treats as plain.
///
/// The bytes are taken as the CALLER'S OWN, at their own stage: this entry
/// receives no provenance and cannot invent any, so the reported spans address
/// exactly the buffer that was passed in. It deliberately does NOT claim the
/// bytes came from a preprocessor — a caller that knows they did, and knows
/// which tool produced them, records that at the admission boundary
/// (`PreprocessedStyle`) rather than here.
#[napi]
pub fn transform_vue_style(
    css: Buffer,
    options: TransformVueStyleOptions,
) -> Result<TransformVueStyleResult> {
    let css = buffer_to_string(css)?;
    catch_panic(std::panic::AssertUnwindSafe(|| {
        let filename = options.filename.as_deref().unwrap_or("style.css");
        // `transform_vue_style` never trusts a bare caller-asserted "this
        // is CSS" label — it parses the received bytes as native CSS
        // through the shared verification entry and builds the
        // `VerifiedPlainCss` witness from that parse (call-site
        // discipline, not a compiler-enforced proof that every witness
        // anywhere was built this way).
        let parsed = verter_compiler::style_planner::parse_plain_css_for_verification(
            &css,
            verter_compiler::style_planner::StyleRewriteStage::AuthoredVBind,
        )
        .map_err(style_rewrite_failure_error)?;
        let verified =
            verter_compiler::style_planner::VerifiedPlainCss::from_parsed_native_css(&parsed)
                .ok_or_else(|| {
                    Error::new(
                        Status::GenericFailure,
                        "verification parser did not produce native CSS syntax IR",
                    )
                })?;
        let module = options.isModule.unwrap_or(false);
        let want_source_map = options.sourcemap.unwrap_or(false);
        let outcome = verter_compiler::style_planner::transform_vue_style(
            verified,
            // The caller supplies bytes and no provenance. `Authored` is the
            // only arm that asserts nothing beyond what is known here — that
            // these are the caller's own bytes, and that reported spans
            // address exactly them. `Preprocessed` would assert an external
            // tool ran, which this entry has no way to know; a caller that
            // does know records it at the admission boundary instead.
            verter_compiler::style_planner::CascadeInput::Authored,
            filename,
            filename,
            filename,
            &options.scopeId,
            module,
            options.scoped.unwrap_or(false),
            want_source_map,
        );
        if !verter_compiler::style_planner::cascade_output_is_publishable(&outcome, &css) {
            return Err(Error::new(
                Status::GenericFailure,
                outcome
                    .stage_failures
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        let source_map = if want_source_map {
            verter_compiler::style_planner::cascade_requested_source_map(&outcome, &css, filename)
        } else {
            None
        };
        let module_name = if module {
            Some(options.moduleName.unwrap_or_else(|| "$style".to_string()))
        } else {
            None
        };
        let refusals = outcome
            .result
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.message().to_string())
            .collect();
        Ok(TransformVueStyleResult {
            code: outcome.result.into_code(),
            sourceMap: source_map,
            moduleClasses: outcome
                .facts
                .module_classes
                .into_iter()
                .map(|(name, hashed)| vec![name, hashed])
                .collect(),
            moduleName: module_name,
            vBindVars: to_napi_v_bind_vars(outcome.facts.v_bind_vars),
            refusals,
        })
    }))?
}

#[napi(object)]
#[derive(Default)]
pub struct AnalyzeStyleOptions {
    pub scopeId: String,
    /// Authored dialect: "css" (default) | "scss" | "sass" | "less" | "stylus".
    pub dialect: Option<String>,
    pub filename: Option<String>,
}

#[napi(object)]
pub struct AnalyzeStyleResult {
    /// Every complete static class selector, in first-occurrence order.
    pub staticClasses: Vec<String>,
    /// CSS-Modules would-be hashed name for each, each entry is [original, hashed]
    pub moduleClasses: Vec<Vec<String>>,
}

/// Read-only style facts — NO rewrite. Drives IDE-style completions/analysis
/// callers that need to know what a style block declares without paying for
/// (or risking) a rewrite.
#[napi]
pub fn analyze_style(css: Buffer, options: AnalyzeStyleOptions) -> Result<AnalyzeStyleResult> {
    let css = buffer_to_string(css)?;
    catch_panic(std::panic::AssertUnwindSafe(|| {
        let filename = options.filename.as_deref().unwrap_or("style");
        let input = verter_compiler::style_planner::AuthoredStyleInput::new(
            &css,
            css_dialect(options.dialect.as_deref())?,
            filename,
            filename,
            filename,
        );
        verter_compiler::style_planner::analyze_style(input, &options.scopeId)
            .map_err(style_rewrite_failure_error)
    }))?
    .map(|analysis| AnalyzeStyleResult {
        staticClasses: analysis.static_classes,
        moduleClasses: analysis
            .module_classes
            .into_iter()
            .map(|(name, hashed)| vec![name, hashed])
            .collect(),
    })
}

/// Per-thread count of `parse_selector` executions.
/// `matchCssSelectors` must not increment this.
#[napi]
pub fn parse_selector_thread_invocations() -> f64 {
    verter_semantic::analysis::parse_selector_thread_invocations() as f64
}

// =============================================================================
// NAPI ↔ FFI zero-copy boundary structs
//
// These use camelCase field names for JS convention. They map to/from
// verter_ffi types via From impls (field-by-field moves, zero allocation).
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct NapiHostConfig {
    pub devMode: Option<bool>,
    pub compileErrorPolicy: Option<String>,
    pub lspScheme: Option<String>,
    pub maxProfilesPerFile: Option<u32>,
    pub resolveExtensions: Option<Vec<String>>,
    pub analysisLevel: Option<String>,
    /// Enable Rust-first native audit for component-meta requests.
    /// When true, timing/memory/store data is captured per request.
    pub auditEnabled: Option<bool>,
    /// Enable per-request semantic footprint capture. Requires
    /// `auditEnabled = true` — necessary for
    /// `getComponentMetaWithAudit` to return a populated bundle.
    pub footprintCapture: Option<bool>,
    /// Capacity of the host-owned typeinfo scratch cache used by
    /// `evaluateTypeExpressionWithAudit`. `None` (default) selects
    /// 64 entries; `Some(0)` disables the cache; other values cap
    /// the LRU at the chosen size.
    pub typeinfoScratchCacheCapacity: Option<u32>,
    /// Worker count for the host-owned CPU pool used by every host batch
    /// API's outer coordinator — `compile_many` and the component-meta
    /// batch. `None` (default) resolves to
    /// `std::thread::available_parallelism` at host-construction time;
    /// `Some(0)` is treated as `None`; other positive values cap the
    /// pool's worker count. The host pool is built once at host
    /// construction and reused across every batch call — to
    /// change the pool size, construct a new host.
    pub hostCpuThreads: Option<u32>,
    /// Worker count for the scheduler-owned CPU stage pool. `None` or
    /// `Some(0)` keeps the scheduler default; a positive value fixes the
    /// pool size for this host.
    pub schedulerCpuThreads: Option<u32>,
    /// Worker count for the scheduler-owned I/O stage pool. `None` or
    /// `Some(0)` keeps the scheduler default; a positive value fixes the
    /// pool size for this host.
    pub schedulerIoThreads: Option<u32>,
    /// Enable host performance-metrics collection. `None`/absent keeps
    /// the default `false` (counters stay zero; `getMetrics()` returns
    /// `null`). Replaces the retired `session_metrics` Cargo feature as
    /// the runtime opt-in — previously that feature had to be compiled
    /// in; now it is a per-host construction choice.
    pub metricsEnabled: Option<bool>,
}

impl From<NapiHostConfig> for FfiHostConfig {
    fn from(n: NapiHostConfig) -> Self {
        Self {
            dev_mode: n.devMode,
            compile_error_policy: n.compileErrorPolicy,
            lsp_scheme: n.lspScheme,
            max_profiles_per_file: n.maxProfilesPerFile,
            resolve_extensions: n.resolveExtensions,
            analysis_level: n.analysisLevel,
            audit_enabled: n.auditEnabled,
            footprint_capture: n.footprintCapture,
            typeinfo_scratch_cache_capacity: n.typeinfoScratchCacheCapacity,
            host_cpu_threads: n.hostCpuThreads,
            metrics_enabled: n.metricsEnabled,
        }
    }
}

fn scheduler_config_from_napi(
    config: &NapiHostConfig,
) -> verter_scheduler::scheduler::SchedulerConfig {
    let mut scheduler = verter_scheduler::scheduler::SchedulerConfig::default();
    if let Some(threads) = config.schedulerCpuThreads.filter(|&threads| threads > 0) {
        scheduler.cpu_threads = threads as usize;
    }
    if let Some(threads) = config.schedulerIoThreads.filter(|&threads| threads > 0) {
        scheduler.io_threads = threads as usize;
    }
    scheduler
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiCompileProfile {
    pub filename: Option<String>,
    pub isProduction: Option<bool>,
    pub customElement: Option<bool>,
    pub ssr: Option<bool>,
    /// SSR asset-collection module id registered on `ssrContext.modules`.
    /// Vite's ssr-manifest keys are ROOT-RELATIVE — the plugin supplies
    /// `normalizePath(relative(root, filename))`; absent falls back to the
    /// canonical id. Already honored on the `compile_many` runtime-render
    /// batch lane (`NapiCompileBatchRenderProfile.ssrModuleId`); this is
    /// the same field for the per-file lane (`get`/`getIde`/
    /// `compileWithAudit`/etc.), which previously had no channel for it.
    pub ssrModuleId: Option<String>,
    pub hmrStrategy: Option<String>,
    pub componentId: Option<String>,
    pub delimiters: Option<Vec<String>>,
    pub customElements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtimeModuleName: Option<String>,
    pub typesModuleName: Option<String>,
    pub forceVapor: Option<bool>,
    pub forceJs: Option<bool>,
    pub sourceMap: Option<bool>,
    /// Compilation target preset: "bundler" (default), "ide", or "analysis".
    pub target: Option<String>,
    /// Inline the render function inside `setup()` (Vue production topology;
    /// absent resolves to isProduction).
    pub inline: Option<bool>,
    /// Experimental: strict slot children type checking.
    pub strictSlots: Option<bool>,
    /// Requested compile cache mode: "stateless", "content", or
    /// "session" (default).
    pub requestedMode: Option<String>,
}

impl From<NapiCompileProfile> for FfiCompileProfile {
    fn from(n: NapiCompileProfile) -> Self {
        Self {
            filename: n.filename,
            is_production: n.isProduction,
            custom_element: n.customElement,
            ssr: n.ssr,
            ssr_module_id: n.ssrModuleId,
            hmr_strategy: n.hmrStrategy,
            component_id: n.componentId,
            delimiters: n.delimiters,
            custom_elements: n.customElements,
            comments: n.comments,
            runtime_module_name: n.runtimeModuleName,
            types_module_name: n.typesModuleName,
            force_vapor: n.forceVapor,
            force_js: n.forceJs,
            source_map: n.sourceMap,
            target: n.target,
            inline: n.inline,
            strict_slots: n.strictSlots,
            requested_mode: n.requestedMode,
        }
    }
}

/// The exact camelCase key set `NapiCompileProfile`'s 19 declared fields
/// use. `#[napi(object)]`'s derived `FromNapiValue` only reads THESE
/// declared property names off a JS object — it never enumerates the
/// object's own keys — so an unrecognized property (e.g. a caller typo
/// like `compatConfig`) is silently dropped before any Rust-side
/// validation ever runs. This is the NAPI-side counterpart to the FFI/WASM
/// boundary's `#[serde(deny_unknown_fields)]` fix on `FfiCompileProfile`:
/// a fresh JS-object key-enumeration check via `Object::keys()`, since
/// napi-rs's `#[napi(object)]` derive has no `deny_unknown_fields`-
/// equivalent attribute.
const NAPI_COMPILE_PROFILE_KNOWN_KEYS: &[&str] = &[
    "filename",
    "isProduction",
    "customElement",
    "ssr",
    "ssrModuleId",
    "hmrStrategy",
    "componentId",
    "delimiters",
    "customElements",
    "comments",
    "runtimeModuleName",
    "typesModuleName",
    "forceVapor",
    "forceJs",
    "sourceMap",
    "target",
    "inline",
    "strictSlots",
    "requestedMode",
];

/// Refuse a `compileProfile`-shaped JS object carrying any key outside
/// [`NAPI_COMPILE_PROFILE_KNOWN_KEYS`].
fn reject_unknown_compile_profile_keys(obj: &Object) -> Result<()> {
    for key in Object::keys(obj)? {
        if !NAPI_COMPILE_PROFILE_KNOWN_KEYS.contains(&key.as_str()) {
            return Err(Error::new(
                Status::InvalidArg,
                format!(
                    "unrecognized compileProfile field '{key}'; expected one of: {}",
                    NAPI_COMPILE_PROFILE_KNOWN_KEYS.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// Validate a raw `compileProfile` JS object's keys, then convert it to the
/// typed `NapiCompileProfile` via the SAME derived `FromNapiValue` codegen
/// napi-rs would otherwise call automatically at the argument boundary —
/// this is not a second conversion path, only a validation step inserted
/// before the existing one runs.
fn napi_compile_profile_from_object(obj: Object) -> Result<NapiCompileProfile> {
    reject_unknown_compile_profile_keys(&obj)?;
    // SAFETY: `obj` is a live `Object` obtained from a valid napi_env/
    // napi_value pair (either napi-rs's own argument extraction, or —
    // for the nested case in `reconstruct_with_validated_compile_profile`
    // — `Object::get::<Object>` over one, which napi-rs already validated
    // as an object). `NapiCompileProfile::from_napi_value` is the exact
    // conversion napi-rs's own derive would run for this argument
    // position; only the unknown-key check above is new.
    unsafe { NapiCompileProfile::from_napi_value(obj.value().env, obj.raw()) }
}

/// For an outer `#[napi(object)]` struct `T` with a nested
/// `compileProfile: Option<NapiCompileProfile>` field (`NapiVirtualQuery`,
/// `NapiBlockOverrideRequest`): validate that nested object's keys, THEN
/// reconstruct the full typed `T` via its normal derived conversion. The
/// outer struct's OWN fields are unaffected — only `compileProfile`'s keys
/// are checked, matching the review's scoped finding.
fn reconstruct_with_validated_compile_profile<T: FromNapiValue>(obj: Object) -> Result<T> {
    if let Some(profile_obj) = obj.get::<Object>("compileProfile")? {
        reject_unknown_compile_profile_keys(&profile_obj)?;
    }
    // SAFETY: `obj` is a live `Object` obtained from napi-rs's own argument
    // extraction (the caller received it in place of the typed `T` napi
    // would otherwise have bound directly). `T::from_napi_value` is the
    // exact conversion napi-rs's own derive would run for this argument
    // position; only the nested unknown-key check above is new.
    unsafe { T::from_napi_value(obj.value().env, obj.raw()) }
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiIdeProjectConfig {
    pub root: String,
    pub workspaceRoot: String,
    pub tsconfigPath: Option<String>,
    pub providerRoot: Option<String>,
    pub workspaceAliases: Option<Vec<NapiWorkspaceAlias>>,
    pub compilerOptions: Option<NapiIdeProjectCompilerOptions>,
    pub references: Option<Vec<String>>,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiWorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiIdeProjectCompilerOptions {
    pub baseUrl: Option<String>,
    pub paths: Option<Vec<NapiTsConfigPath>>,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct NapiTsConfigPath {
    pub pattern: String,
    pub targets: Vec<String>,
}

#[napi(object)]
#[derive(Clone)]
pub struct NapiVirtualNodeKind {
    pub kind: String,
    pub index: Option<u32>,
}

impl From<NapiVirtualNodeKind> for FfiVirtualNodeKind {
    fn from(n: NapiVirtualNodeKind) -> Self {
        Self {
            kind: n.kind,
            index: n.index,
        }
    }
}

impl From<FfiVirtualNodeKind> for NapiVirtualNodeKind {
    fn from(f: FfiVirtualNodeKind) -> Self {
        Self {
            kind: f.kind,
            index: f.index,
        }
    }
}

#[napi(object)]
pub struct NapiDependencyResolution {
    pub specifier: String,
    #[napi(ts_type = "string | undefined")]
    pub resolved_canonical_id: Option<String>,
    #[napi(ts_type = "string[] | undefined")]
    pub possible_canonical_ids: Option<Vec<String>>,
}

#[napi(object)]
pub struct NapiUpsertRequest {
    pub canonicalId: Option<String>,
    pub inputId: String,
    /// SFC source code as UTF-8 bytes (e.g., `fs.readFileSync(path)`).
    pub source: Buffer,
    pub fileKind: Option<String>,
    pub aliases: Option<Vec<String>>,
}

#[napi(object)]
pub struct NapiVirtualQuery {
    pub rawId: Option<String>,
    pub canonicalId: Option<String>,
    pub nodeKind: Option<NapiVirtualNodeKind>,
    pub compileProfile: Option<NapiCompileProfile>,
}

impl From<NapiVirtualQuery> for FfiVirtualQuery {
    fn from(n: NapiVirtualQuery) -> Self {
        Self {
            raw_id: n.rawId,
            canonical_id: n.canonicalId,
            node_kind: n.nodeKind.map(Into::into),
            compile_profile: n.compileProfile.map(Into::into),
        }
    }
}

// --- Output structs (Rust → V8) ---

#[napi(object)]
pub struct NapiSliceChanges {
    pub scriptChanged: bool,
    pub templateChanged: bool,
    pub styleIndicesChanged: Vec<u32>,
    pub customIndicesChanged: Vec<u32>,
    pub structureChanged: bool,
    pub descriptorChanged: bool,
}

#[napi(object)]
pub struct NapiDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub spanStart: u32,
    pub spanEnd: u32,
}

#[napi(object)]
pub struct NapiDiagnosticsSnapshot {
    pub diagnostics: Vec<NapiDiagnostic>,
    pub hasErrors: bool,
}

#[napi(object)]
pub struct NapiExternalSourceRequest {
    pub ownerCanonicalId: String,
    pub blockKind: String,
    pub specifier: String,
    pub resolvedCanonicalId: String,
    pub blockToken: String,
    pub ownerRevision: String,
    pub artifactToken: String,
    pub carrierSourceSpaceToken: String,
}

#[napi(object)]
pub struct NapiScriptImportInfo {
    pub source: String,
    pub isTypeOnly: bool,
    pub bindings: Vec<String>,
}

#[napi(object)]
pub struct NapiPreprocessorRequest {
    pub contentClass: String,
    /// The `lang` attribute value (e.g., "pug", "coffee", "scss").
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
    pub availability: String,
    pub correlationToken: String,
    pub blockToken: String,
    pub ownerRevision: String,
    pub artifactToken: String,
    pub expectedLanguage: String,
    pub priorBasisToken: Option<String>,
    pub basisToken: String,
    pub sourceSpaceToken: String,
    pub contentHash: String,
    pub customType: Option<String>,
}

#[napi(object)]
pub struct NapiPreprocessorDiagnostic {
    /// `"error"`, `"warning"`, or `"info"`.
    pub severity: String,
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[napi(object)]
pub struct NapiBlockOverrideEntry {
    pub correlationToken: String,
    pub blockToken: String,
    pub ownerRevision: String,
    pub artifactToken: String,
    pub expectedLanguage: String,
    pub priorBasisToken: Option<String>,
    pub basisToken: String,
    pub sourceSpaceToken: String,
    /// Preprocessed code as UTF-8 bytes.
    pub code: Buffer,
    pub codeHash: String,
    /// Source map from the preprocessor, if available.
    pub sourceMap: Option<String>,
    pub sourceMapHash: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub diagnostics: Option<Vec<NapiPreprocessorDiagnostic>>,
    pub processorIdentity: Option<String>,
    pub processorVersion: Option<String>,
    pub configFingerprint: Option<String>,
    /// Superseded wire field, replaced by `dependencies`/`diagnostics`/
    /// `processorIdentity`/`processorVersion`/`configFingerprint` above.
    /// Accepted so an un-migrated caller's object literal (still typed with
    /// this property) keeps matching this struct's published surface instead
    /// of silently drifting from it; never read below. Remove once every
    /// caller migrates to the new fields.
    pub suppliedProvenance: Option<String>,
}

#[napi(object)]
pub struct NapiBlockOverrideRequest {
    pub canonicalId: String,
    pub compileProfile: Option<NapiCompileProfile>,
    pub overrides: Vec<NapiBlockOverrideEntry>,
}

#[napi(object)]
pub struct NapiExportSignature {
    pub name: String,
    pub isType: bool,
    pub reexportSource: Option<String>,
    pub reexportLocal: Option<String>,
}

#[napi(object)]
pub struct NapiResolvedExport {
    pub name: String,
    pub isType: bool,
    pub sourceCanonicalId: Option<String>,
    pub sourceName: String,
}

#[napi(object)]
pub struct NapiUpdateResult {
    pub canonicalId: String,
    pub changed: bool,
    pub sliceChanges: NapiSliceChanges,
    pub changedVirtualNodes: Vec<NapiVirtualNodeKind>,
    pub removedVirtualNodes: Vec<NapiVirtualNodeKind>,
    pub changedVirtualIds: Vec<String>,
    pub removedVirtualIds: Vec<String>,
    pub changedLspIds: Vec<String>,
    pub removedLspIds: Vec<String>,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub externalSourceRequests: Vec<NapiExternalSourceRequest>,
    pub importSpecifiers: Vec<NapiScriptImportInfo>,
    pub moduleReferences: Vec<NapiModuleReference>,
    pub preprocessorRequests: Vec<NapiPreprocessorRequest>,
    pub exportSignatures: Vec<NapiExportSignature>,
    pub parseDurationMs: f64,
}

#[napi(object)]
pub struct NapiModuleReference {
    pub syntax: String,
    pub semantics: String,
    pub isTypeOnly: bool,
    pub rawText: String,
    pub literalSpecifier: Option<String>,
    pub finiteSpecifiers: Vec<String>,
    pub staticPrefix: Option<String>,
    pub analyzability: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub exprSpanStart: u32,
    pub exprSpanEnd: u32,
}

#[napi(object)]
pub struct NapiResolvedId {
    pub canonicalId: String,
    pub nodeKind: NapiVirtualNodeKind,
    pub existsInHost: bool,
    pub bundlerId: String,
    pub lspId: String,
}

#[napi(object)]
pub struct NapiVirtualMeta {
    pub scopeId: Option<String>,
    pub blockType: Option<String>,
}

#[napi(object)]
pub struct NapiVirtualFileResponse {
    pub id: String,
    pub code: String,
    pub sourceMap: Option<String>,
    pub lang: Option<String>,
    pub stale: bool,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub meta: NapiVirtualMeta,
    /// `true` iff this response was served from a warm cache slot (the
    /// fact-validated session slot OR the content-addressed store).
    pub cacheHit: bool,
    /// Requested compile cache mode ("stateless" / "content" / "session").
    pub requestedMode: String,
    /// Actual compile cache mode the runtime ran under.
    pub actualMode: String,
    /// Highest-priority downgrade reason, or `None` when none fired.
    pub downgradeReason: Option<String>,
}

/// A single destructured binding's source mapping (UTF-16 for JS).
#[napi(object)]
pub struct NapiDestructuredBinding {
    pub name: String,
    pub sourceStart: u32,
    pub sourceEnd: u32,
}

/// Metadata for the destructured block region in the generated TSX (UTF-16 for JS).
#[napi(object)]
pub struct NapiDestructuredBlockMeta {
    pub bindings: Vec<NapiDestructuredBinding>,
    pub blockStart: u32,
    pub blockEnd: u32,
}

/// IDE output for type checking (dedicated API, not a virtual file).
#[napi(object)]
pub struct NapiIdeResponse {
    pub code: String,
    pub sourceMap: Option<String>,
    pub isJsx: bool,
    pub destructuredBlock: Option<NapiDestructuredBlockMeta>,
}

/// One separately-addressed output of a typed runtime compile product.
#[napi(object)]
pub struct NapiCompileRequestVirtualNode {
    pub node: NapiVirtualNodeKind,
    pub code: String,
    pub sourceMap: Option<String>,
    pub lang: Option<String>,
    pub meta: NapiVirtualMeta,
}

/// One product row in a typed compile response.
///
/// Exactly one payload field is present, selected by `kind`.
#[napi(object)]
pub struct NapiCompileRequestProduct {
    pub kind: String,
    pub nodes: Option<Vec<NapiCompileRequestVirtualNode>>,
    pub ide: Option<NapiIdeResponse>,
    /// JSON rendering of the host analysis payload.
    pub analysis: Option<String>,
}

/// Complete typed compile response.
#[napi(object)]
pub struct NapiCompileRequestResponse {
    pub canonicalId: String,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub products: Vec<NapiCompileRequestProduct>,
}

/// Typed terminal failure for one batch entry.
#[napi(object)]
pub struct NapiCompileRequestFailure {
    pub kind: String,
    pub canonicalId: String,
    pub message: String,
    pub diagnostics: NapiDiagnosticsSnapshot,
    pub requestedFramework: Option<String>,
    pub registeredFramework: Option<String>,
    pub productKind: Option<String>,
    pub diagnosticCode: Option<String>,
}

/// One batch result at its original input position.
#[napi(object, use_nullable = true)]
pub struct NapiCompileRequestsEntry {
    pub canonicalId: String,
    pub response: Option<NapiCompileRequestResponse>,
    pub failure: Option<NapiCompileRequestFailure>,
}

/// TSC output for TypeScript declaration generation (macro-extraction only).
///
/// `code` is the TYPESCRIPT-LABELED rendering of the public-API surface: every
/// JS consumer of this wire places it at a fixed `.ts`-shaped companion path
/// (the plugin mirror's `.verter.ts`, the playground carrier store), where the
/// JavaScript/JSDoc rendering of a widened JS Options-API stub would have its
/// types silently ignored. The wire carries no dialect and no second channel —
/// selection happens host-side (`TscResponse::ts_labeled_code`).
#[napi(object, use_nullable = true)]
pub struct NapiTscResponse {
    pub code: String,
    pub sourceMap: Option<String>,
}

/// Stable structured identity for a failed public-API projection.
#[napi(object)]
pub struct NapiTscMacroFailureSubject {
    pub kind: String,
    pub syntaxIndex: u32,
}

#[napi(object)]
pub struct NapiSourceRange {
    pub start: u32,
    pub end: u32,
}

#[napi(object)]
pub struct NapiTscScriptSetupAttrsFailureSubject {
    pub kind: String,
    pub sourceRange: NapiSourceRange,
}

/// Stable structured identity for a failed public-API projection.
#[napi(object, use_nullable = true)]
pub struct NapiPublicApiProjectionError {
    pub code: String,
    pub detailCode: String,
    pub subject: Either<NapiTscMacroFailureSubject, NapiTscScriptSetupAttrsFailureSubject>,
    pub declarationShapeReason: Option<String>,
    pub memberOrdinal: Option<u32>,
    pub outcomeKind: Option<String>,
    pub outcomeReason: Option<String>,
    pub outcomeDiagnostic: Option<String>,
}

/// Explicit tri-state public-API result: value, ordinary absence, or failure.
#[napi(object, use_nullable = true)]
pub struct NapiPublicApiResult {
    pub value: Option<NapiTscResponse>,
    pub error: Option<NapiPublicApiProjectionError>,
}

impl From<FfiTscResponse> for NapiTscResponse {
    fn from(value: FfiTscResponse) -> Self {
        Self {
            code: value.code,
            sourceMap: value.source_map,
        }
    }
}

impl From<FfiPublicApiProjectionError> for NapiPublicApiProjectionError {
    fn from(value: FfiPublicApiProjectionError) -> Self {
        let subject = match value.subject {
            PublicApiProjectionSubject::Macro { syntax_index } => {
                Either::A(NapiTscMacroFailureSubject {
                    kind: "macro".to_string(),
                    syntaxIndex: syntax_index,
                })
            }
            PublicApiProjectionSubject::ScriptSetupAttrs { source_range } => {
                Either::B(NapiTscScriptSetupAttrsFailureSubject {
                    kind: "scriptSetupAttrs".to_string(),
                    sourceRange: NapiSourceRange {
                        start: source_range.start,
                        end: source_range.end,
                    },
                })
            }
        };
        Self {
            code: value.code,
            detailCode: value.detail_code,
            subject,
            declarationShapeReason: value.declaration_shape_reason,
            memberOrdinal: value.member_ordinal,
            outcomeKind: value.outcome_kind,
            outcomeReason: value.outcome_reason,
            outcomeDiagnostic: value.outcome_diagnostic,
        }
    }
}

impl From<FfiPublicApiResult> for NapiPublicApiResult {
    fn from(value: FfiPublicApiResult) -> Self {
        Self {
            value: value.value.map(Into::into),
            error: value.error.map(Into::into),
        }
    }
}

#[napi(object)]
pub struct NapiRemoveResult {
    pub canonicalId: String,
}

// --- Code action structs ---

#[napi(object)]
pub struct NapiTextEdit {
    pub spanStart: u32,
    pub spanEnd: u32,
    pub newText: String,
}

#[napi(object)]
pub struct NapiCodeAction {
    pub title: String,
    pub kind: String,
    pub edits: Vec<NapiTextEdit>,
    pub isPreferred: bool,
    pub diagnosticRule: Option<String>,
}

impl From<FfiCodeAction> for NapiCodeAction {
    fn from(f: FfiCodeAction) -> Self {
        Self {
            title: f.title,
            kind: f.kind,
            edits: f
                .edits
                .into_iter()
                .map(|e| NapiTextEdit {
                    spanStart: e.span_start,
                    spanEnd: e.span_end,
                    newText: e.new_text,
                })
                .collect(),
            isPreferred: f.is_preferred,
            diagnosticRule: f.diagnostic_rule,
        }
    }
}

// --- Lint rule metadata structs ---

#[napi(object)]
pub struct NapiLintRuleMetadata {
    pub name: String,
    pub category: String,
    pub defaultSeverity: String,
}

impl From<FfiLintRuleMetadata> for NapiLintRuleMetadata {
    fn from(f: FfiLintRuleMetadata) -> Self {
        Self {
            name: f.name,
            category: f.category,
            defaultSeverity: f.default_severity,
        }
    }
}

// --- Document symbol structs ---

#[napi(object)]
pub struct NapiDocumentSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: u32,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub selectionStart: u32,
    pub selectionEnd: u32,
    pub children: Vec<NapiDocumentSymbol>,
}

impl From<FfiDocumentSymbol> for NapiDocumentSymbol {
    fn from(f: FfiDocumentSymbol) -> Self {
        Self {
            name: f.name,
            detail: f.detail,
            kind: f.kind,
            spanStart: f.span_start,
            spanEnd: f.span_end,
            selectionStart: f.selection_start,
            selectionEnd: f.selection_end,
            children: f.children.into_iter().map(Into::into).collect(),
        }
    }
}

// --- CSS selector matching structs ---

#[napi(object)]
pub struct NapiElementMatch {
    pub tag: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub result: String,
}

impl From<FfiElementMatch> for NapiElementMatch {
    fn from(f: FfiElementMatch) -> Self {
        Self {
            tag: f.tag,
            spanStart: f.span_start,
            spanEnd: f.span_end,
            result: f.result,
        }
    }
}

#[napi(object)]
pub struct NapiSelectorMatchResult {
    pub selectorText: String,
    pub selectorStart: u32,
    pub selectorEnd: u32,
    pub matches: Vec<NapiElementMatch>,
}

impl From<FfiSelectorMatchResult> for NapiSelectorMatchResult {
    fn from(f: FfiSelectorMatchResult) -> Self {
        Self {
            selectorText: f.selector_text,
            selectorStart: f.selector_start,
            selectorEnd: f.selector_end,
            matches: f.matches.into_iter().map(Into::into).collect(),
        }
    }
}

// --- Lint diagnostic struct ---

#[napi(object)]
pub struct NapiLintDiagnostic {
    pub rule: String,
    pub category: String,
    pub severity: String,
    pub message: String,
    pub spanStart: u32,
    pub spanEnd: u32,
    pub tags: Vec<String>,
    pub spanKind: String,
}

/// Point-in-time snapshot of host performance metrics.
///
/// Only populated when built with the `session_metrics` feature.
/// Obtain via [`NapiVerterHost::getMetrics`].
#[napi(object)]
pub struct NapiHostMetrics {
    /// Total number of `upsert()` calls.
    pub upserts: f64,
    /// Total compile requests (cache misses that triggered Rust compilation).
    pub compileRequests: f64,
    /// Compile requests served from cache.
    pub compileCacheHits: f64,
    /// Cache hit rate (0.0 – 1.0).
    pub compileCacheHitRate: f64,
    /// Total `getVirtualFile()` calls.
    pub virtualLoads: f64,
    /// Total `resolve()` calls.
    pub resolves: f64,
    /// Cumulative parse/hash time across all upserts (microseconds).
    pub sliceHashTimeUsTotal: f64,
    /// Average parse/hash time per upsert (microseconds).
    pub avgSliceHashTimeUs: f64,
    /// Cumulative Rust compilation time (microseconds).
    pub compileTimeUsTotal: f64,
}

pub(crate) fn host_node_kind_to_napi(input: &host::VirtualNodeKind) -> NapiVirtualNodeKind {
    match input {
        host::VirtualNodeKind::Main => NapiVirtualNodeKind {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => NapiVirtualNodeKind {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => NapiVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => NapiVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => NapiVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

/// Map a JS HMR-strategy string to the host [`host::HmrStrategy`]. Mirrors
/// the `verter_ffi` profile conversion (`"vite"` / `"webpack"` / `"none"`,
/// case-insensitive). Faithful mapping — an unknown value is an error, not
/// a silent drop to `None`.
fn ffi_hmr_strategy_to_host(s: &str) -> std::result::Result<host::HmrStrategy, String> {
    if s.eq_ignore_ascii_case("vite") {
        Ok(host::HmrStrategy::Vite)
    } else if s.eq_ignore_ascii_case("webpack") {
        Ok(host::HmrStrategy::Webpack)
    } else if s.eq_ignore_ascii_case("none") {
        Ok(host::HmrStrategy::None)
    } else {
        Err(format!(
            "invalid hmrStrategy '{s}', expected 'vite', 'webpack', or 'none'"
        ))
    }
}

/// Convert a single host [`host::HostDiagnostic`] into its NAPI wire
/// shape. Used to surface the private Vue render worker's soft-macro warnings
/// on [`NapiCompileBatchEntry::diagnostics`].
fn napi_diagnostic_from_host(d: &host::HostDiagnostic) -> NapiDiagnostic {
    host_diagnostic_to_napi(d, None)
}

fn host_block_kind_to_str(kind: &host::ExternalBlockKind) -> &'static str {
    match kind {
        host::ExternalBlockKind::Script => "script",
        host::ExternalBlockKind::Template => "template",
        host::ExternalBlockKind::Style => "style",
        host::ExternalBlockKind::Custom => "custom",
    }
}

fn host_module_reference_syntax_to_str(
    syntax: verter_semantic::analysis::ModuleReferenceSyntax,
) -> &'static str {
    match syntax {
        verter_semantic::analysis::ModuleReferenceSyntax::StaticImport => "staticImport",
        verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom => "exportFrom",
        verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport => "dynamicImport",
        verter_semantic::analysis::ModuleReferenceSyntax::RequireCall => "requireCall",
    }
}

fn host_module_reference_semantics_to_str(
    semantics: verter_semantic::analysis::ModuleReferenceSemantics,
) -> &'static str {
    match semantics {
        verter_semantic::analysis::ModuleReferenceSemantics::Import => "import",
        verter_semantic::analysis::ModuleReferenceSemantics::Require => "require",
    }
}

fn host_module_reference_analyzability_to_str(
    analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
) -> &'static str {
    match analyzability {
        verter_semantic::analysis::ModuleReferenceAnalyzability::Exact => "exact",
        verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet => "finiteSet",
        verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic => "unknownDynamic",
    }
}

fn napi_module_reference_syntax_from_str(
    syntax: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSyntax> {
    match syntax {
        "staticImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::StaticImport),
        "exportFrom" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom),
        "dynamicImport" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport),
        "requireCall" => Ok(verter_semantic::analysis::ModuleReferenceSyntax::RequireCall),
        other => Err(ffi_err(format!("unknown module reference syntax: {other}"))),
    }
}

fn napi_module_reference_semantics_from_str(
    semantics: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceSemantics> {
    match semantics {
        "import" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Import),
        "require" => Ok(verter_semantic::analysis::ModuleReferenceSemantics::Require),
        other => Err(ffi_err(format!(
            "unknown module reference semantics: {other}"
        ))),
    }
}

fn napi_module_reference_analyzability_from_str(
    analyzability: &str,
) -> Result<verter_semantic::analysis::ModuleReferenceAnalyzability> {
    match analyzability {
        "exact" => Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::Exact),
        "finiteSet" => Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet),
        "unknownDynamic" => {
            Ok(verter_semantic::analysis::ModuleReferenceAnalyzability::UnknownDynamic)
        }
        other => Err(ffi_err(format!(
            "unknown module reference analyzability: {other}"
        ))),
    }
}

fn napi_module_reference_to_analysis(
    input: NapiModuleReference,
) -> Result<verter_semantic::analysis::AnalyzedModuleReference> {
    Ok(verter_semantic::analysis::AnalyzedModuleReference {
        syntax: napi_module_reference_syntax_from_str(&input.syntax)?,
        semantics: napi_module_reference_semantics_from_str(&input.semantics)?,
        is_type_only: input.isTypeOnly,
        span: verter_span::Span::new(input.spanStart, input.spanEnd),
        expr_span: verter_span::Span::new(input.exprSpanStart, input.exprSpanEnd),
        raw_text: input.rawText,
        literal_specifier: input.literalSpecifier,
        finite_specifiers: input.finiteSpecifiers,
        static_prefix: input.staticPrefix,
        analyzability: napi_module_reference_analyzability_from_str(&input.analyzability)?,
    })
}

fn default_known_dependency_extensions() -> Vec<String> {
    vec![
        "".to_string(),
        ".ts".to_string(),
        ".tsx".to_string(),
        ".js".to_string(),
        ".jsx".to_string(),
        ".mts".to_string(),
        ".mjs".to_string(),
        ".cts".to_string(),
        ".cjs".to_string(),
        ".vue".to_string(),
        ".svelte".to_string(),
    ]
}

fn host_module_reference_to_napi(input: host::ScriptModuleReference) -> NapiModuleReference {
    NapiModuleReference {
        syntax: host_module_reference_syntax_to_str(input.syntax).to_string(),
        semantics: host_module_reference_semantics_to_str(input.semantics).to_string(),
        isTypeOnly: input.is_type_only,
        rawText: input.raw_text,
        literalSpecifier: input.literal_specifier,
        finiteSpecifiers: input.finite_specifiers,
        staticPrefix: input.static_prefix,
        analyzability: host_module_reference_analyzability_to_str(input.analyzability).to_string(),
        spanStart: input.span.start,
        spanEnd: input.span.end,
        exprSpanStart: input.expr_span.start,
        exprSpanEnd: input.expr_span.end,
    }
}

fn host_update_to_napi(input: host::HostUpdateResult, source: Option<&str>) -> NapiUpdateResult {
    NapiUpdateResult {
        canonicalId: input.canonical_id,
        changed: input.changed,
        sliceChanges: NapiSliceChanges {
            scriptChanged: input.slice_changes.script_changed,
            templateChanged: input.slice_changes.template_changed,
            styleIndicesChanged: input
                .slice_changes
                .style_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            customIndicesChanged: input
                .slice_changes
                .custom_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            structureChanged: input.slice_changes.structure_changed,
            descriptorChanged: input.slice_changes.descriptor_changed,
        },
        changedVirtualNodes: input
            .changed_virtual_nodes
            .iter()
            .map(host_node_kind_to_napi)
            .collect(),
        removedVirtualNodes: input
            .removed_virtual_nodes
            .iter()
            .map(host_node_kind_to_napi)
            .collect(),
        changedVirtualIds: input.changed_virtual_ids,
        removedVirtualIds: input.removed_virtual_ids,
        changedLspIds: input.changed_lsp_ids,
        removedLspIds: input.removed_lsp_ids,
        diagnostics: host_diagnostics_to_napi(&input.diagnostics, source),
        externalSourceRequests: input
            .external_source_requests
            .into_iter()
            .map(|req| NapiExternalSourceRequest {
                ownerCanonicalId: req.owner_canonical_id,
                blockKind: host_block_kind_to_str(&req.block_kind).to_string(),
                specifier: req.specifier,
                resolvedCanonicalId: req.resolved_canonical_id,
                blockToken: req.block_token,
                ownerRevision: req.owner_revision,
                artifactToken: req.artifact_token,
                carrierSourceSpaceToken: req.carrier_source_space_token,
            })
            .collect(),
        importSpecifiers: input
            .import_specifiers
            .into_iter()
            .map(|imp| NapiScriptImportInfo {
                source: imp.source,
                isTypeOnly: imp.is_type_only,
                bindings: imp.bindings,
            })
            .collect(),
        moduleReferences: input
            .module_references
            .into_iter()
            .map(host_module_reference_to_napi)
            .collect(),
        preprocessorRequests: input
            .preprocessor_requests
            .iter()
            .map(|req| NapiPreprocessorRequest {
                contentClass: match req.content_class {
                    host::BlockContentClass::Template => "template".to_string(),
                    host::BlockContentClass::Script => "script".to_string(),
                    host::BlockContentClass::Style => "style".to_string(),
                    host::BlockContentClass::Custom => "custom".to_string(),
                },
                lang: req.lang.clone(),
                content: req.content.clone(),
                availability: match req.availability {
                    host::BlockContentAvailability::NativeAvailable => "nativeAvailable",
                    host::BlockContentAvailability::ProcessedContentRequired => {
                        "processedContentRequired"
                    }
                    host::BlockContentAvailability::SuppliedAvailable => "suppliedAvailable",
                    host::BlockContentAvailability::Missing => "missing",
                    host::BlockContentAvailability::Conflict => "conflict",
                    host::BlockContentAvailability::Stale => "stale",
                }
                .to_string(),
                correlationToken: req.captured_echo.request.correlation_token.to_string(),
                blockToken: req.captured_echo.request.block_token.to_string(),
                ownerRevision: req.captured_echo.request.owner_revision.to_string(),
                artifactToken: req.captured_echo.request.artifact_token.to_string(),
                expectedLanguage: req.captured_echo.request.expected_language.clone(),
                priorBasisToken: req
                    .captured_echo
                    .request
                    .prior_basis_token
                    .as_ref()
                    .map(ToString::to_string),
                basisToken: req.captured_echo.basis_token.to_string(),
                sourceSpaceToken: req.source_space_token.to_string(),
                contentHash: req.content_hash.to_string(),
                customType: req.custom_type.clone(),
            })
            .collect(),
        exportSignatures: input
            .export_signatures
            .into_iter()
            .map(|sig| NapiExportSignature {
                name: sig.name,
                isType: sig.is_type,
                reexportSource: sig.reexport_source,
                reexportLocal: sig.reexport_local,
            })
            .collect(),
        parseDurationMs: input.parse_duration_ms,
    }
}

fn host_virtual_file_to_napi(
    input: host::VirtualFileResponse,
    source: Option<&str>,
) -> NapiVirtualFileResponse {
    NapiVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        sourceMap: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: host_diagnostics_to_napi(&input.diagnostics, source),
        meta: NapiVirtualMeta {
            scopeId: input.meta.scope_id,
            blockType: input.meta.block_type,
        },
        cacheHit: input.cache_hit,
        requestedMode: input.requested_mode.to_string(),
        actualMode: input.actual_mode.to_string(),
        downgradeReason: input.downgrade_reason.map(|r| r.to_string()),
    }
}

fn napi_project_config_to_ide(
    config: NapiIdeProjectConfig,
) -> verter_semantic::resolver_core::IdeProjectConfig {
    let mut ide = verter_workspace::ide_project_config(
        config.root.clone(),
        config.workspaceRoot,
        config.tsconfigPath,
    );
    if let Some(provider_root) = config.providerRoot {
        ide.provider_root = provider_root;
    }
    if let Some(aliases) = config.workspaceAliases {
        ide.workspace_aliases = aliases
            .into_iter()
            .map(|a| verter_semantic::resolver_core::WorkspaceAlias {
                find: a.find,
                replacement: a.replacement,
            })
            .collect();
    }
    if let Some(opts) = config.compilerOptions {
        ide.compiler_options.base_url = opts.baseUrl;
        if let Some(paths) = opts.paths {
            ide.compiler_options.paths =
                paths.into_iter().map(|p| (p.pattern, p.targets)).collect();
        }
    }
    if let Some(refs) = config.references {
        ide.references = refs;
    }
    ide
}

fn host_resolved_id_to_napi(input: host::ResolvedId) -> NapiResolvedId {
    NapiResolvedId {
        canonicalId: input.canonical_id,
        nodeKind: host_node_kind_to_napi(&input.node_kind),
        existsInHost: input.exists_in_host,
        bundlerId: input.bundler_id,
        lspId: input.lsp_id,
    }
}

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// Shared with WASM (crates/verter_wasm):
// - Both: new, resolve, upsert, applyBlockOverrides,
//         getVirtualFile, listVirtualFiles, remove, setImportDependencies,
//         getAnalysis, getTsx, lint, getCodeActions, getLintRuleMetadata,
//         getDocumentSymbols, matchCssSelectors, computeCrossFileOptimizations,
//         compileRequest
// - NAPI-only: prepareStyleForPreprocessor / transformVueStyle / analyzeStyle
//   (CSS entry points), getTsc, compileMany, compileRequests, getMetrics
// =============================================================================

// ═══════════════════════════════════════════════════════════════════════════
// Workspace
// ═══════════════════════════════════════════════════════════════════════════

/// Directory entry returned by `Workspace.readDir()`.
#[napi(object)]
pub struct NapiDirEntry {
    pub path: String,
    pub is_dir: bool,
}

/// Workspace object backed by `FilesystemWorkspace`.
///
/// Provides file access, import resolution, and project configuration.
/// Construct first, then pass to `VerterHost.withWorkspace()`.
#[napi(js_name = "Workspace")]
pub struct NapiWorkspace {
    inner: std::sync::Arc<verter_workspace::FilesystemWorkspace>,
}

impl NapiWorkspace {
    /// Get the underlying workspace as a trait object.
    pub(crate) fn workspace(&self) -> std::sync::Arc<dyn verter_workspace::WorkspaceAccess> {
        std::sync::Arc::clone(&self.inner) as std::sync::Arc<dyn verter_workspace::WorkspaceAccess>
    }
}

#[napi]
impl NapiWorkspace {
    /// Create a new workspace rooted at the given directories.
    ///
    /// **Lazy by design.** The constructor stores the roots and the
    /// backing `FilesystemWorkspace` only — it does NOT auto-discover
    /// tsconfigs or build a project graph. Until a caller invokes
    /// [`Self::configure_projects`] (`workspace.configureProjects(...)`
    /// in JS), `Engine::resolve_import` walks an empty `ProjectGraph`
    /// and falls through to the bare-VFS resolver.
    ///
    /// JS consumers that need a configured workspace MUST call
    /// `configureProjects` after construction, supplying the alias map
    /// derived from the project's tsconfig chain. The canonical pattern
    /// lives in `packages/component-meta/src/compat/checker.ts`:
    /// `extractPathAliases(parsedTsconfig, projectRoot)` produces the
    /// `NapiIdeProjectConfig` shape, which is passed to
    /// `workspace.configureProjects([aliases])`. Bench and audit
    /// harnesses mirror the same shape.
    #[napi(constructor)]
    pub fn new(roots: Vec<String>) -> Self {
        let ws = verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions {
            roots,
            eager_preload: false,
        });
        Self {
            inner: std::sync::Arc::new(ws),
        }
    }

    // ── Async filesystem operations ──
    //
    // All filesystem methods are async to avoid blocking the Node.js event loop.
    // The underlying VFS operations are synchronous but run on the libuv thread pool.

    /// Read a file from the workspace (overlay → snapshot → disk).
    #[napi(js_name = "readFile")]
    pub async fn read_file(&self, path: String) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.read_file(&path).map(|s| s.to_string()))
    }

    /// Check if a file exists in the workspace.
    #[napi(js_name = "fileExists")]
    pub async fn file_exists(&self, path: String) -> Result<bool> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.file_exists(&path))
    }

    /// Check if a path is a directory.
    #[napi(js_name = "isDir")]
    pub async fn is_dir(&self, path: String) -> Result<bool> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.is_dir(&path))
    }

    /// Write file content. Creates parent directories as needed.
    #[napi(js_name = "writeFile")]
    pub async fn write_file(&self, path: String, content: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .write_file(&path, &content)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Read directory entries. Returns array of { path, isDir }.
    #[napi(js_name = "readDir")]
    pub async fn read_dir(&self, dir: String) -> Result<Vec<NapiDirEntry>> {
        use verter_workspace::WorkspaceRead;
        self.inner
            .read_dir(&dir)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| NapiDirEntry {
                        path: e.path,
                        is_dir: e.is_dir,
                    })
                    .collect()
            })
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Recursively walk a directory. Returns matching file paths.
    #[napi(js_name = "walk")]
    pub async fn walk(
        &self,
        root: String,
        exclude_dirs: Vec<String>,
        extensions: Option<Vec<String>>,
    ) -> Result<Vec<String>> {
        use verter_workspace::WorkspaceRead;
        let exts = extensions;
        self.inner
            .walk(
                &root,
                &|dir_path| {
                    let name = dir_path.rsplit('/').next().unwrap_or(dir_path);
                    !exclude_dirs.iter().any(|ex| ex == name)
                },
                &|file_path| match &exts {
                    Some(ext_list) => ext_list.iter().any(|ext| file_path.ends_with(ext.as_str())),
                    None => true,
                },
            )
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Delete a file.
    #[napi(js_name = "deleteFile")]
    pub async fn delete_file(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .delete_file(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Create a directory and all parent directories.
    #[napi(js_name = "createDirAll")]
    pub async fn create_dir_all(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .create_dir_all(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Delete a directory and all its contents.
    #[napi(js_name = "deleteDirAll")]
    pub async fn delete_dir_all(&self, path: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .delete_dir_all(&path)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Copy a file from src to dst.
    #[napi(js_name = "copyFile")]
    pub async fn copy_file(&self, src: String, dst: String) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        self.inner
            .copy_file(&src, &dst)
            .map_err(|e| Error::new(Status::GenericFailure, format!("{e}")))
    }

    /// Resolve symlinks to real path. Returns null if not found.
    #[napi(js_name = "realpath")]
    pub async fn realpath(&self, path: String) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        Ok(self.inner.realpath(&path))
    }

    /// Resolve an import specifier with context.
    #[napi(js_name = "resolveImport")]
    pub async fn resolve_import(
        &self,
        importer: String,
        specifier: String,
        phase: Option<String>,
        kind: Option<String>,
    ) -> Result<Option<String>> {
        use verter_workspace::WorkspaceRead;
        let phase = match phase.as_deref() {
            Some("provider") => verter_semantic::resolver_core::ResolvePhase::ProviderGraph,
            _ => verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
        };
        let kind = match kind.as_deref() {
            Some("type") => verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
            Some("require") => verter_semantic::resolver_core::ResolveRequestKind::RequireCall,
            Some("src") => verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
            _ => verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
        };
        let ctx = verter_semantic::resolver_core::ResolutionContext { phase, kind };
        Ok(self
            .inner
            .resolve_import(&importer, &specifier, ctx)
            .map(|r| r.source_id))
    }

    /// Configure project resolver from tsconfig/alias data.
    /// Replaces (not merges with) any auto-discovered graph.
    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_semantic::resolver_core::IdeProjectConfig> = projects
                .into_iter()
                .map(napi_project_config_to_ide)
                .collect();
            use verter_workspace::WorkspaceAccess;
            self.inner.configure_resolver(configs);
        }))
    }

    /// Notify workspace that an editor buffer is open/changed.
    #[napi(js_name = "notifyUpsert")]
    pub fn notify_upsert(&self, canonical_id: String, source: Buffer) -> Result<()> {
        use verter_workspace::WorkspaceAccess;
        let source_str = std::str::from_utf8(&source)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid UTF-8: {e}")))?;
        self.inner
            .notify_upsert(&canonical_id, std::sync::Arc::from(source_str));
        Ok(())
    }

    /// Notify workspace that an editor buffer was closed.
    #[napi(js_name = "notifyClose")]
    pub fn notify_close(&self, canonical_id: String) {
        use verter_workspace::WorkspaceAccess;
        self.inner.notify_close(&canonical_id);
    }

    /// Notify workspace that a file was deleted.
    #[napi(js_name = "notifyDelete")]
    pub fn notify_delete(&self, canonical_id: String) {
        use verter_workspace::WorkspaceAccess;
        self.inner.notify_delete(&canonical_id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Host
// ═══════════════════════════════════════════════════════════════════════════

/// Manages a collection of Vue SFCs and their compiled virtual files (script,
/// template, styles). Files are upserted as source, then lazily compiled into
/// virtual outputs that a bundler or LSP can request individually.
#[napi(js_name = "VerterHost")]
pub struct NapiVerterHost {
    inner: std::sync::Arc<host::VerterHost>,
}

#[napi]
impl NapiVerterHost {
    /// Creates a new `VerterHost` with the given configuration.
    ///
    /// - `config` — optional host settings (dev mode, compile error policy,
    ///   LSP scheme, analysis level, etc.). Defaults are used when `None`.
    ///
    /// Returns an error if the configuration contains invalid values (e.g. an
    /// unrecognised `compileErrorPolicy` string).
    #[napi(constructor)]
    pub fn new(config: Option<NapiHostConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        let scheduler_config = scheduler_config_from_napi(&config);
        let ffi_config: FfiHostConfig = config.into();
        Ok(Self {
            inner: std::sync::Arc::new(host::VerterHost::new_standalone_with_scheduler_config(
                ffi_config_to_host(ffi_config).map_err(ffi_err)?,
                scheduler_config,
            )),
        })
    }

    /// Creates a new `VerterHost` backed by the given workspace.
    ///
    /// The workspace handles all file access and import resolution.
    /// Use `workspace.configureProjects()` before calling this to set up
    /// the project resolver.
    #[napi(factory)]
    pub fn with_workspace(
        config: Option<NapiHostConfig>,
        workspace: &NapiWorkspace,
    ) -> Result<Self> {
        let config = config.unwrap_or_default();
        let scheduler_config = scheduler_config_from_napi(&config);
        let ffi_config: FfiHostConfig = config.into();
        let host_config = ffi_config_to_host(ffi_config).map_err(ffi_err)?;
        Ok(Self {
            inner: std::sync::Arc::new(host::VerterHost::new_with_scheduler_config(
                host_config,
                workspace.inner.clone(),
                scheduler_config,
            )),
        })
    }

    /// Resolves a raw import ID (e.g. `./Foo.vue?type=style&index=0`) into its
    /// canonical ID, virtual node kind, and bundler/LSP identifiers.
    ///
    /// Returns `None` if the ID does not match any file tracked by this host.
    #[napi]
    pub fn resolve(&self, raw_id: String) -> Result<Option<NapiResolvedId>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.resolve(&raw_id).map(host_resolved_id_to_napi)
        }))
    }

    /// Inserts or updates a file in the host.
    ///
    /// Parses the SFC source, diffs it against the previously stored version
    /// (if any), and returns a detailed changeset describing which virtual
    /// nodes changed, any diagnostics, and external source requests that the
    /// caller must resolve (e.g. `<script src="...">` references).
    ///
    /// - `request.inputId` — the file path used for import resolution.
    /// - `request.source` — SFC source as a UTF-8 `Buffer`.
    /// - `request.fileKind` — optional explicit kind (`"vue"`/`"sfc"`/
    ///   `"vue_sfc"`, `"svelte"`, or `"non_sfc"`/`"text"`/`"file"`);
    ///   classified from the canonical path when `None`.
    ///
    /// Returns an error if the source is not valid UTF-8 or if the file kind
    /// is unrecognised.
    #[napi]
    pub fn upsert(&self, request: NapiUpsertRequest) -> Result<NapiUpdateResult> {
        let source = buffer_to_string(request.source)?;
        let source_for_spans = source.clone();
        let ffi_req = FfiUpsertRequest {
            canonical_id: request.canonicalId,
            input_id: request.inputId,
            source,
            file_kind: request.fileKind,
            aliases: request.aliases,
        };
        let host_req = ffi_upsert_to_host(ffi_req).map_err(ffi_err)?;
        catch_panic(std::panic::AssertUnwindSafe(|| self.inner.upsert(host_req)))?
            .map(|result| host_update_to_napi(result, Some(source_for_spans.as_str())))
            .map_err(host_error)
    }

    /// Execute one typed compile request against an already-registered source.
    #[napi(js_name = "compileRequest")]
    pub fn compile_request(
        &self,
        env: Env,
        canonical_id: String,
        #[napi(ts_arg_type = "import('./host-compile-request.generated').HostCompileRequest")]
        request: NapiHostCompileRequest,
    ) -> Result<NapiCompileRequestResponse> {
        // Resolved to the host's identity once, up front, so this route
        // reports ONE id spelling whatever the outcome. A success already
        // answers `response.canonical_id`, which is canonical; without
        // this, a construction refusal would answer the caller's raw
        // spelling instead, and the same route would name the same file
        // two different ways depending on whether it compiled.
        let canonical_id = self.inner.resolve_alias_or_canonical(&canonical_id);
        let request =
            match host_compile_request::napi_host_compile_request_to_compile_request(request) {
                Ok(request) => request,
                Err(error) => {
                    let failure = binding_failure_to_napi(
                        canonical_id,
                        compile_request_construction_refused(&error),
                    );
                    return Err(compile_request_error(&env, Status::InvalidArg, failure)?);
                }
            };
        let response = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compile_request(&canonical_id, request)
        }))?;
        match response {
            Ok(response) => {
                let source = self
                    .inner
                    .get_source(&response.canonical_id)
                    .ok_or_else(|| {
                        Error::new(
                            Status::GenericFailure,
                            "typed compile succeeded without a registered source",
                        )
                    })?;
                compile_request_response_to_napi(response, &source)
            }
            Err(failure) => {
                let source_id = failure_canonical_id(&failure, &canonical_id).to_string();
                let source = self.inner.get_source(&source_id);
                let status = compile_request_failure_status(&failure);
                let failure =
                    compile_request_failure_to_napi(failure, canonical_id, source.as_deref());
                Err(compile_request_error(&env, status, failure)?)
            }
        }
    }

    /// Register and execute one typed request per batch entry.
    #[napi(js_name = "compileRequests")]
    pub fn compile_requests(
        &self,
        env: Env,
        #[napi(
            ts_arg_type = "Array<{ canonicalId: string; source: Buffer; request: import('./host-compile-request.generated').HostCompileRequest }>"
        )]
        inputs: Object<'_>,
        #[napi(ts_arg_type = "{ priority?: 'interactive' | 'background' }")] options: Option<
            Unknown<'_>,
        >,
    ) -> Result<Vec<NapiCompileRequestsEntry>> {
        let priority = decode_compile_requests_priority(&env, options)?;
        // `Vec<Object>` is unsafe at this boundary: napi-rs reserves the JS
        // array's declared length before entering this method. A sparse array
        // can declare billions of elements without owning them, so classify
        // the value as an array, recover any pending exception, then enforce
        // MAX_ARRAY_ELEMENTS before any allocation proportional to length.
        if !match napi_value_is_array(&env, inputs.raw()) {
            Ok(is_array) => is_array,
            Err(error) => {
                let error = recover_pending_exception(&env, error)?;
                return Err(error);
            }
        } {
            let _ = recover_pending_exception(
                &env,
                ffi_err("compile request batch input must be an array"),
            );
            return Err(ffi_err("compile request batch input must be an array"));
        }
        let declared = match inputs.get_array_length() {
            Ok(declared) => declared,
            Err(error) => {
                return Err(recover_pending_exception(&env, error)?);
            }
        };
        if declared > MAX_ARRAY_ELEMENTS {
            return Err(ffi_err(format!(
                "A request array declares {declared} elements, above the \
                 {MAX_ARRAY_ELEMENTS} a request may carry"
            )));
        }
        let len = declared as usize;
        let mut output: Vec<Option<NapiCompileRequestsEntry>> =
            std::iter::repeat_with(|| None).take(len).collect();
        let mut positions = Vec::with_capacity(len);
        let mut sources = Vec::with_capacity(len);
        let mut expected_canonical_ids = Vec::with_capacity(len);
        let mut converted = Vec::with_capacity(len);
        let mut payload_budget = JsValueMaterializationBudget::new(
            MAX_DECODED_VALUES_PER_REQUEST,
            MAX_COMPILE_REQUEST_BATCH_RETAINED_BYTES,
        );
        // SAFETY: every value read through this graph comes from `inputs`,
        // which napi-rs extracted from this live env.
        let graph = unsafe { js_value_graph::NapiValueGraph::new(env.raw()) };
        for array_index in 0..declared {
            let position = array_index as usize;
            if payload_budget.bytes_exhausted() {
                // Nothing further can be decoded, so nothing further is
                // read: the remaining entries answer the ceiling directly
                // rather than each re-discovering it.
                output[position] = Some(binding_failure_entry(
                    String::new(),
                    batch_retained_bytes_refusal(
                        position,
                        &format!(
                            "the batch already retains \
                             {MAX_COMPILE_REQUEST_BATCH_RETAINED_BYTES} bytes \
                             of decoded payload"
                        ),
                    ),
                ));
                continue;
            }
            // One nested handle scope per entry. Every engine-side handle
            // this iteration creates — the input object, its property
            // values, and every key name and property the request decode
            // walks — is released when the guard drops at the end of the
            // iteration, instead of staying pinned in the call's single
            // outer scope until the whole batch returns. Everything that
            // escapes the iteration is owned Rust.
            let _entry_scope = JsHandleScope::open(&env)?;
            let input = match inputs.get_element::<Object<'_>>(array_index) {
                Ok(input) => input,
                Err(error) => {
                    let error = recover_pending_exception(&env, error)?;
                    output[position] = Some(binding_failure_entry(
                        String::new(),
                        format!(
                            "invalid compile request batch input at index {array_index}: {}",
                            error.reason
                        ),
                    ));
                    continue;
                }
            };
            let fields = match read_batch_entry_fields(&env, &graph, &input) {
                Ok(fields) => fields,
                Err(error) => {
                    let error = recover_pending_exception(&env, error)?;
                    output[position] = Some(binding_failure_entry(
                        String::new(),
                        format!(
                            "invalid compile request batch input at index {array_index}: {}",
                            error.reason
                        ),
                    ));
                    continue;
                }
            };
            let canonical_id = match fields.canonical_id {
                Some(raw_canonical_id) => {
                    let length = match js_string_utf8_len(&env, raw_canonical_id) {
                        Ok(length) => length,
                        Err(error) => {
                            let error = recover_pending_exception(&env, error)?;
                            output[position] = Some(binding_failure_entry(
                                String::new(),
                                format!("invalid `canonicalId`: {}", error.reason),
                            ));
                            continue;
                        }
                    };
                    if let Err(error) = payload_budget.retain_bytes(length) {
                        output[position] = Some(binding_failure_entry(
                            String::new(),
                            batch_retained_bytes_refusal(position, &error.reason),
                        ));
                        continue;
                    }
                    // SAFETY: the value was read from this env and validated
                    // as a string immediately above.
                    match unsafe { String::from_napi_value(env.raw(), raw_canonical_id) } {
                        // Canonicalized HERE, once, through the host's own
                        // identity resolver — not left as the caller's raw
                        // spelling. `compile_request_many` canonicalizes
                        // every input it is handed (a Windows drive letter
                        // lowercases, a backslash becomes a slash, a `?`
                        // query tail and an extended-length prefix are
                        // stripped, a registered alias resolves), so a
                        // binding that kept the raw spelling would report
                        // one id on a locally-refused entry and a different
                        // id on its compiling sibling, and the position
                        // check below would read every non-canonical input
                        // as a transposition. A registered canonical is its
                        // own alias, so resolving the resolved id again is
                        // the same demand; that check is per-entry
                        // precisely so a resolver that ever stopped being
                        // idempotent costs the affected entry rather than
                        // every sibling's compiled output.
                        Ok(canonical_id) => self.inner.resolve_alias_or_canonical(&canonical_id),
                        Err(error) => {
                            let error = recover_pending_exception(&env, error)?;
                            output[position] = Some(binding_failure_entry(
                                String::new(),
                                format!("invalid `canonicalId`: {}", error.reason),
                            ));
                            continue;
                        }
                    }
                }
                None => {
                    output[position] = Some(binding_failure_entry(
                        String::new(),
                        "compile request batch input is missing `canonicalId`".to_string(),
                    ));
                    continue;
                }
            };
            let source = match fields.source {
                Some(raw_source) => {
                    match read_batch_source_buffer(&env, raw_source, &mut payload_budget) {
                        Ok(source) => source,
                        Err(error) => {
                            let error = recover_pending_exception(&env, error)?;
                            let reason = if payload_budget.bytes_exhausted() {
                                batch_retained_bytes_refusal(position, &error.reason)
                            } else {
                                error.reason.clone()
                            };
                            output[position] = Some(binding_failure_entry(canonical_id, reason));
                            continue;
                        }
                    }
                }
                None => {
                    output[position] = Some(binding_failure_entry(
                        canonical_id,
                        "compile request batch input is missing `source`".to_string(),
                    ));
                    continue;
                }
            };
            payload_budget.reset_decoded_values();
            let request = match fields.request {
                Some(raw_request) => {
                    // SAFETY: the value was read from this env; the raw
                    // request retains the env/value pair rather than
                    // interpreting it, and the decode below is what reads it.
                    let raw_request = unsafe {
                        host_compile_request::RawNapiHostCompileRequest::from_napi_value(
                            env.raw(),
                            raw_request,
                        )
                    }?;
                    match host_compile_request::decode_host_compile_request_with_budget(
                        raw_request,
                        &mut payload_budget,
                    ) {
                        Ok(request) => {
                            match host_compile_request::napi_host_compile_request_to_compile_request(
                                request,
                            ) {
                                Ok(request) => request,
                                Err(error) => {
                                    output[position] = Some(binding_failure_entry(
                                        canonical_id.clone(),
                                        compile_request_construction_refused(&error),
                                    ));
                                    continue;
                                }
                            }
                        }
                        Err(error) => {
                            let error = recover_pending_exception(&env, error)?;
                            let reason = if payload_budget.bytes_exhausted() {
                                batch_retained_bytes_refusal(position, &error.reason)
                            } else {
                                payload_budget.reset_decoded_values();
                                error.reason.clone()
                            };
                            output[position] =
                                Some(binding_failure_entry(canonical_id.clone(), reason));
                            continue;
                        }
                    }
                }
                None => {
                    output[position] = Some(binding_failure_entry(
                        canonical_id,
                        "compile request batch input is missing `request`".to_string(),
                    ));
                    continue;
                }
            };
            positions.push(position);
            sources.push(std::sync::Arc::clone(&source));
            expected_canonical_ids.push(canonical_id.clone());
            converted.push(host_compile::CompileRequestBatchInput {
                canonical_id,
                source,
                request,
            });
        }

        let entries = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compile_request_many(
                converted,
                host_compile::CompileRequestBatchOptions { priority },
            )
        }))?;
        // The batch executor is trusted to answer one entry per input in
        // order, but a silent-corruption regression there — a dropped,
        // duplicated, or transposed entry — must not silently pair a
        // diagnostic with the wrong file's source text or leave an output
        // slot unfilled. A dropped or duplicated entry cannot be attributed
        // to a position at all, so it fails the call.
        if let Some(mismatch) = batch_entry_count_mismatch(&entries, &expected_canonical_ids) {
            return Err(Error::new(Status::GenericFailure, mismatch));
        }

        for (((entry, source), position), expected) in entries
            .into_iter()
            .zip(sources)
            .zip(positions)
            .zip(expected_canonical_ids)
        {
            // A transposition IS attributable: fail the position it landed
            // on rather than every sibling that did land where its input
            // asked. The response is dropped rather than paired with the
            // wrong input's source.
            if entry.canonical_id != expected {
                output[position] = Some(binding_failure_entry(
                    entry.canonical_id.clone(),
                    batch_entry_position_mismatch(&entry.canonical_id, &expected),
                ));
                continue;
            }
            output[position] = Some(match entry.outcome {
                Ok(response) => match compile_request_response_to_napi(response, source.as_ref()) {
                    Ok(response) => NapiCompileRequestsEntry {
                        canonicalId: entry.canonical_id,
                        response: Some(response),
                        failure: None,
                    },
                    Err(error) => NapiCompileRequestsEntry {
                        canonicalId: entry.canonical_id.clone(),
                        response: None,
                        failure: Some(binding_failure_to_napi(
                            entry.canonical_id,
                            error.reason.clone(),
                        )),
                    },
                },
                Err(failure) => NapiCompileRequestsEntry {
                    canonicalId: entry.canonical_id.clone(),
                    response: None,
                    failure: Some(compile_request_failure_to_napi(
                        failure,
                        entry.canonical_id,
                        Some(source.as_ref()),
                    )),
                },
            });
        }
        output
            .into_iter()
            .map(|entry| {
                entry.ok_or_else(|| {
                    Error::new(
                        Status::GenericFailure,
                        "typed compile batch did not produce an entry for every input",
                    )
                })
            })
            .collect()
    }

    /// Replaces one or more blocks with preprocessed content (e.g. the output
    /// of Pug, CoffeeScript, SCSS, or custom block preprocessors) and
    /// recompiles affected virtual nodes.
    ///
    /// This is the unified API for template, script, style, and custom-block
    /// preprocessing. Every result echoes the sealed identity and source stamps
    /// from its corresponding preprocessor request; the host admits bytes only
    /// after validating those stamps and the code/map hashes.
    ///
    /// Returns the same changeset structure as [`upsert`](Self::upsert).
    #[napi(js_name = "applyBlockOverrides")]
    pub fn apply_block_overrides(&self, request: Object) -> Result<NapiUpdateResult> {
        let request: NapiBlockOverrideRequest =
            reconstruct_with_validated_compile_profile(request)?;
        let canonical_for_source = request.canonicalId.clone();
        let overrides = request
            .overrides
            .into_iter()
            .map(|e| {
                Ok(FfiBlockOverrideEntry {
                    correlation_token: e.correlationToken,
                    block_token: e.blockToken,
                    owner_revision: e.ownerRevision,
                    artifact_token: e.artifactToken,
                    expected_language: e.expectedLanguage,
                    prior_basis_token: e.priorBasisToken,
                    basis_token: e.basisToken,
                    source_space_token: e.sourceSpaceToken,
                    code: buffer_to_string(e.code)?,
                    code_hash: e.codeHash,
                    source_map: e.sourceMap,
                    source_map_hash: e.sourceMapHash,
                    dependencies: e.dependencies.unwrap_or_default(),
                    diagnostics: e
                        .diagnostics
                        .unwrap_or_default()
                        .into_iter()
                        .map(|d| FfiPreprocessorDiagnostic {
                            severity: d.severity,
                            message: d.message,
                            line: d.line,
                            column: d.column,
                        })
                        .collect(),
                    processor_identity: e.processorIdentity,
                    processor_version: e.processorVersion,
                    config_fingerprint: e.configFingerprint,
                    // Superseded field, forwarded for parity with the
                    // JSON/WASM path's `FfiBlockOverrideEntry` (never read
                    // downstream by `ffi_block_override_to_host`) rather
                    // than discarded here — an un-migrated caller's value
                    // is preserved on the wire even though nothing
                    // interprets it yet.
                    supplied_provenance: e.suppliedProvenance,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let ffi_req = FfiBlockOverrideRequest {
            canonical_id: request.canonicalId,
            compile_profile: request.compileProfile.map(Into::into),
            overrides,
        };
        let host_req = ffi_block_override_to_host(ffi_req).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.apply_block_overrides(host_req)
        }))?
        .map_err(host_error)?;
        let source = self.inner.get_source(&canonical_for_source);
        Ok(host_update_to_napi(result, source.as_deref()))
    }

    /// Retrieves a single compiled virtual file (script, template, or style).
    ///
    /// The query can identify the file by raw import ID or by canonical ID +
    /// node kind. A compile profile may be provided to control production
    /// mode, SSR, source maps, etc.
    ///
    /// Returns the compiled code, optional source map, language hint, and
    /// any compilation diagnostics.
    ///
    /// Returns `null` if the virtual node does not exist (e.g. no `<script>` block).
    /// Returns an error if the query is invalid or the source file is not found.
    #[napi(js_name = "getVirtualFile")]
    pub fn get_virtual_file(&self, query: Object) -> Result<Option<NapiVirtualFileResponse>> {
        let query: NapiVirtualQuery = reconstruct_with_validated_compile_profile(query)?;
        let canonical_for_source = if let Some(canonical) = query.canonicalId.as_ref() {
            Some(canonical.clone())
        } else if let Some(raw_id) = query.rawId.as_ref() {
            self.inner.resolve(raw_id).map(|r| r.canonical_id)
        } else {
            None
        };
        let ffi_query: FfiVirtualQuery = query.into();
        let host_query = ffi_virtual_query_to_host(ffi_query).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_virtual_file(host_query)
        }))?;
        match classify_host_virtual_file(result) {
            VirtualFileOutcome::Published(vf) => {
                let source = canonical_for_source
                    .as_deref()
                    .and_then(|canonical| self.inner.get_source(canonical));
                Ok(Some(host_virtual_file_to_napi(vf, source.as_deref())))
            }
            VirtualFileOutcome::Absent => Ok(None),
            VirtualFileOutcome::Failed(e) => Err(host_error(e)),
        }
    }

    /// Lists all virtual node kinds for a given canonical file ID.
    ///
    /// Returns an array of node kinds (e.g. `main`, `script`, `template`,
    /// `style[0]`, `style[1]`, ...) that can be passed to
    /// [`get_virtual_file`](Self::get_virtual_file). Returns an empty array
    /// if the canonical ID is not tracked by the host.
    #[napi(js_name = "listVirtualFiles")]
    pub fn list_virtual_files(&self, canonical_id: String) -> Result<Vec<NapiVirtualNodeKind>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .list_virtual_files(&canonical_id)
                .iter()
                .map(host_node_kind_to_napi)
                .collect()
        }))
    }

    /// Removes a file from the host by its canonical ID or any registered alias.
    ///
    /// All associated virtual nodes and cached compilations are discarded.
    /// Returns `None` if no file matched the given ID.
    #[napi]
    pub fn remove(&self, canonical_or_alias: String) -> Result<Option<NapiRemoveResult>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .remove(&canonical_or_alias)
                .map(|r| NapiRemoveResult {
                    canonicalId: r.canonical_id,
                })
        }))
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    ///
    /// Returns `null` if the file doesn't exist in the host.
    /// When `analysis_level` is not "full", computes analysis on demand from stored source.
    ///
    /// **Note:** Returns a JSON *string* — the caller must `JSON.parse()`.
    /// The WASM variant (`verter_wasm`) returns a native JS object instead
    /// (via `serde_wasm_bindgen`). This inconsistency is intentional:
    /// defining NAPI structs for all `verter_semantic::analysis` types is high effort
    /// for low value since `getAnalysis` is primarily used by the playground.
    #[napi(js_name = "getAnalysis")]
    pub fn get_analysis(&self, canonical_or_alias: String) -> Result<Option<String>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))
        .map(|opt| {
            opt.map(|snapshot| {
                serde_json::to_string(&snapshot).map_err(|e| {
                    Error::new(
                        Status::GenericFailure,
                        format!("analysis serialization error: {e}"),
                    )
                })
            })
            .transpose()
        })?
    }

    /// Returns the registered content-free carrier structure as JSON.
    #[napi(js_name = "getDocumentStructure")]
    pub fn get_document_structure(&self, canonical_or_alias: String) -> Result<Option<String>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .registered_file_structure(&canonical_or_alias)
                .map(|structure| {
                    let projected = registered_structure_to_ffi(&structure);
                    serde_json::to_string(&projected).map_err(|error| {
                        Error::new(
                            Status::GenericFailure,
                            format!("structure serialization error: {error}"),
                        )
                    })
                })
                .transpose()
        }))?
    }

    /// Evaluate type annotations for a file's component metadata using the
    /// lightweight native evaluator.
    ///
    /// Returns JSON `{ props, emits, slotBindings, bindings }` or `null`.
    #[napi(js_name = "evaluateTypes")]
    pub fn evaluate_types(&self, canonical_or_alias: String) -> Result<Option<String>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = self.inner.evaluate_types(&canonical_or_alias);
            let Some(result) = result else {
                return Ok(None);
            };
            let json = serde_json::to_string(&result).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("type evaluation serialization error: {e}"),
                )
            })?;
            Ok(Some(json))
        }))?
    }

    /// Returns all exports of a file, following re-export chains to their ultimate source.
    ///
    /// For barrel files like `export { default as Button } from './Button.vue'`, this
    /// resolves through the chain to return the ultimate source file and name.
    #[napi(js_name = "resolveExports")]
    pub fn resolve_exports(&self, canonical_or_alias: String) -> Result<Vec<NapiResolvedExport>> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.resolve_exports(&canonical_or_alias)
        }))
        .map(|exports| {
            exports
                .into_iter()
                .map(|e| NapiResolvedExport {
                    name: e.name,
                    isType: e.is_type,
                    sourceCanonicalId: e.source_canonical_id,
                    sourceName: e.source_name,
                })
                .collect()
        })
    }

    /// Retrieves the combined TSX output for LSP type checking.
    ///
    /// This is a dedicated API separate from virtual files. IDE output is
    /// only consumed by the LSP, never by bundlers.
    ///
    /// Returns `{ code, sourceMap?, isJsx }` or `null` if no IDE output is available.
    #[napi(js_name = "getIde")]
    pub fn get_ide(
        &self,
        canonical_id: String,
        profile: Option<Object>,
    ) -> Result<Option<NapiIdeResponse>> {
        let profile = profile.map(napi_compile_profile_from_object).transpose()?;
        let ffi_profile: Option<FfiCompileProfile> = profile.map(Into::into);
        let host_profile = ffi_profile_to_host(ffi_profile)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid profile: {e}")))?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_ide(&canonical_id, &host_profile)
        }))?;
        let sfc_source = self.inner.get_source(&canonical_id);
        Ok(result.map(|response| host_ide_to_napi(response, sfc_source.as_deref().unwrap_or(""))))
    }

    /// Ensure the IDE (`CachedTsx`) projection exists for a file + profile.
    ///
    /// The explicit IDE-ensure path: it compiles the carrier's IDE surface
    /// (never requesting the runtime `Main` node), so a Main-less carrier
    /// (Svelte) populates its `CachedTsx` and a subsequent `getIde` succeeds.
    /// `getIde` itself stays a pure cached read.
    ///
    /// The caller profile is OPTIONAL and is normalized to an IDE/TSX-bearing
    /// target INTERNALLY, so a default / bundler profile (no TSX bit) still
    /// produces the IDE surface. Returns `true` when the carrier HAS an IDE
    /// surface, and `false` ONLY for a genuine no-IDE-surface file (a
    /// non-carrier / plain script).
    ///
    /// A profile that ALSO asks for a runtime product makes this a COMBINED
    /// request identity: if the carrier fail-closes on that runtime surface the
    /// transaction publishes nothing, and this rejects the typed runtime-surface
    /// refusal rather than reporting a missing IDE surface. A real failure
    /// (missing source / compile error) rejects too.
    #[napi(js_name = "ensureIdeCompiled")]
    pub fn ensure_ide_compiled(
        &self,
        canonical_id: String,
        profile: Option<Object>,
    ) -> Result<bool> {
        let profile = profile.map(napi_compile_profile_from_object).transpose()?;
        let ffi_profile: Option<FfiCompileProfile> = profile.map(Into::into);
        let host_profile = ffi_profile_to_host(ffi_profile)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid profile: {e}")))?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.ensure_ide_compiled(&canonical_id, &host_profile)
        }))?
        .map_err(host_error)
    }

    /// Generates TSC output (minimal TypeScript declarations) for a Vue SFC.
    ///
    /// Unlike `getTsx`, this does NOT require a prior `getVirtualFile` call.
    /// It performs macro-only extraction (defineProps, defineEmits, defineModel,
    /// defineOptions) and generates a `ComponentPublicInstance`-based declaration
    /// with inline source map. This is the fast path for IDE type checking.
    ///
    /// `mode` selects the served surface: `"public"` (default when absent) —
    /// the application-facing instance shape; `"testing"` — the Vue Test
    /// Utils-like debug surface exposing `<script setup>` bindings;
    /// `"declaration"` — the declaration-only (`.d.<ext>.ts`) public surface
    /// (a valid `.d.ts` with no runtime/value code). An unknown mode string
    /// is rejected with `InvalidArg`.
    ///
    /// Returns `{ value, error }`; both are `null` for ordinary absence.
    #[napi(js_name = "getPublicApi")]
    pub fn get_public_api(
        &self,
        canonical_id: String,
        mode: Option<String>,
    ) -> Result<NapiPublicApiResult> {
        let mode = ffi_public_api_mode_to_host(mode.as_deref()).map_err(ffi_err)?;
        let result = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .get_public_api_with_mode(&canonical_id, mode, None)
        }))?;
        Ok(host_public_api_result_to_ffi(result).into())
    }

    /// Records the resolved import dependencies for a file.
    ///
    /// Called by the bundler plugin after resolving the `importSpecifiers`
    /// returned by [`upsert`](Self::upsert). This enables cross-file type
    /// resolution (e.g. following `import type { Props } from './types'`
    /// chains) when recompiling dependent files.
    ///
    /// - `canonical_or_alias` — the file whose dependencies are being set.
    /// - `resolutions` — per-specifier resolution records with exact or candidate canonical IDs.
    #[napi(js_name = "setImportDependencies")]
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: String,
        resolutions: Vec<NapiDependencyResolution>,
    ) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.set_import_dependencies(
                &canonical_or_alias,
                resolutions
                    .into_iter()
                    .map(|r| host::DependencyResolution {
                        specifier: r.specifier,
                        resolved_canonical_id: r.resolved_canonical_id,
                        possible_canonical_ids: r.possible_canonical_ids.unwrap_or_default(),
                    })
                    .collect(),
            );
        }))
    }

    /// Returns the exact and finite-set module reference candidates in encounter order.
    ///
    /// Unknown-dynamic references are skipped entirely.
    #[napi(js_name = "collectResolvableModuleReferenceSpecifiers")]
    pub fn collect_resolvable_module_reference_specifiers(
        &self,
        module_references: Vec<NapiModuleReference>,
    ) -> Result<Vec<String>> {
        let module_references = module_references
            .into_iter()
            .map(napi_module_reference_to_analysis)
            .collect::<Result<Vec<_>>>()?;
        Ok(
            verter_semantic::resolver_core::collect_resolvable_module_reference_specifiers(
                &module_references,
            ),
        )
    }

    /// Resolves exact and finite module reference candidates against a caller-provided
    /// in-memory known-file set, without reading from disk.
    #[napi(js_name = "resolveKnownModuleReferenceDependencies")]
    pub fn resolve_known_module_reference_dependencies(
        &self,
        owner_id: String,
        module_references: Vec<NapiModuleReference>,
        known_ids: Vec<String>,
        extensions: Option<Vec<String>>,
    ) -> Result<Vec<String>> {
        let module_references = module_references
            .into_iter()
            .map(napi_module_reference_to_analysis)
            .collect::<Result<Vec<_>>>()?;
        let extensions = extensions.unwrap_or_else(default_known_dependency_extensions);
        Ok(
            verter_semantic::resolver_core::resolve_known_module_reference_dependencies(
                &owner_id,
                &module_references,
                &known_ids,
                &extensions,
            ),
        )
    }

    /// Compute cross-file prop constness optimizations.
    ///
    /// Builds a render tree from all compiled files and determines which
    /// child component props are const across all call sites.
    /// Returns JSON with `constPropOverrides`, `changedFiles`, and `diagnostics`.
    ///
    /// Call after all files are compiled (e.g., after `preCompile` loop).
    /// On subsequent calls, `changedFiles` lists only files whose constness
    /// changed since the last computation.
    #[napi(js_name = "computeCrossFileOptimizations")]
    pub fn compute_cross_file_optimizations(&self) -> Result<String> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compute_cross_file_optimizations()
        }))
        .and_then(|result| {
            let ffi = host_cross_file_result_to_ffi(result);
            serde_json::to_string(&ffi).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("cross-file result serialization error: {e}"),
                )
            })
        })
    }

    /// Release all cached data (files, aliases, dependency graph).
    ///
    /// Configure project-scoped path alias resolution.
    ///
    /// Accepts a list of project configs describing tsconfig paths, workspace
    /// aliases, and project references. The host uses these to resolve aliased
    /// import specifiers (e.g. `@/components/Foo.vue`, `#imports`) without
    /// relying on external caller-provided resolutions.
    ///
    /// Pass an empty array to clear the resolver.
    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_semantic::resolver_core::IdeProjectConfig> = projects
                .into_iter()
                .map(napi_project_config_to_ide)
                .collect();
            self.inner.configure_projects(configs);
        }))
    }

    /// Call this before dropping the host to allow the Rust allocator to free
    /// backing memory immediately, rather than waiting for GC finalisation.
    /// This prevents the Node.js process from hanging on exit.
    #[napi]
    pub fn close(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.close();
        }))
    }

    /// Resolve an import specifier through the host's resolution chain.
    ///
    /// Uses VFS-first-then-fallback pattern (same as internal resolution).
    #[napi(js_name = "resolveImport")]
    pub fn resolve_import_napi(
        &self,
        importer: String,
        specifier: String,
        #[napi(ts_arg_type = "string | undefined")] _phase: Option<String>,
        #[napi(ts_arg_type = "string | undefined")] _kind: Option<String>,
    ) -> Option<String> {
        self.inner.resolve_import(&importer, &specifier)
    }

    /// Returns a snapshot of host performance metrics.
    ///
    /// Only available when the host was constructed with
    /// `NapiHostConfig::metricsEnabled = true`. Returns `null` otherwise.
    #[napi(js_name = "getMetrics")]
    pub fn get_metrics(&self) -> Option<NapiHostMetrics> {
        if !self.inner.config().metrics_enabled {
            return None;
        }
        let m = self.inner.metrics_snapshot();
        Some(NapiHostMetrics {
            upserts: m.upserts as f64,
            compileRequests: m.compile_requests as f64,
            compileCacheHits: m.compile_cache_hits as f64,
            compileCacheHitRate: m.compile_cache_hit_rate,
            virtualLoads: m.virtual_loads as f64,
            resolves: m.resolves as f64,
            sliceHashTimeUsTotal: m.slice_hash_time_us_total as f64,
            avgSliceHashTimeUs: m.avg_slice_hash_time_us,
            compileTimeUsTotal: m.compile_time_us_total as f64,
        })
    }

    /// Runs lint rules against a file's analysis data and returns diagnostics.
    ///
    /// Takes a canonical ID (or alias), retrieves its analysis data from the
    /// host, and runs the linter with the given config. Returns an array of
    /// lint diagnostics with UTF-16 spans.
    ///
    /// - `canonical_or_alias` — the file to lint.
    /// - `config` — optional JSON string with lint config. Pass `None` for defaults.
    /// Ordered SFC block facts projected from the registered carrier
    /// inventory — the sole geometry source for block-structure lint rules
    /// and block-anchored code-action edits. Empty when the file has no
    /// registered structure (fail closed).
    fn registered_block_facts(
        &self,
        canonical_or_alias: &str,
    ) -> Vec<verter_diagnostics::SfcBlockFact> {
        self.inner
            .registered_file_structure_snapshot(canonical_or_alias)
            .map(|(structure, _)| verter_diagnostics::project_block_facts(structure.inventory()))
            .unwrap_or_default()
    }

    #[napi]
    pub fn lint(
        &self,
        canonical_or_alias: String,
        config: Option<String>,
    ) -> Result<Vec<NapiLintDiagnostic>> {
        let lint_config = match config {
            Some(json) => serde_json::from_str::<verter_diagnostics::LintConfig>(&json)
                .map_err(|e| Error::new(Status::InvalidArg, format!("invalid lint config: {e}")))?,
            None => verter_diagnostics::LintConfig::default(),
        };

        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;

        let diagnostics = match analysis {
            Some(snapshot) => {
                let linter = Linter::new(lint_config);
                let script = build_script_snapshot(&snapshot);
                let blocks = self.registered_block_facts(&canonical_or_alias);
                linter
                    .lint(
                        Some(&script),
                        snapshot.template.as_deref(),
                        &snapshot.styles,
                        &blocks,
                    )
                    .into_diagnostics()
            }
            None => Vec::new(),
        };

        let source = self.inner.get_source(&canonical_or_alias);
        let ffi_diagnostics = lint_diagnostics_to_utf16(diagnostics, source.as_deref());

        Ok(ffi_diagnostics
            .into_iter()
            .map(|d| NapiLintDiagnostic {
                rule: d.rule,
                category: d.category,
                severity: match d.severity {
                    verter_diagnostics::Severity::Error => "error".to_string(),
                    verter_diagnostics::Severity::Warning => "warning".to_string(),
                    verter_diagnostics::Severity::Info => "info".to_string(),
                    verter_diagnostics::Severity::Hint => "hint".to_string(),
                },
                message: d.message,
                spanStart: d.span.start,
                spanEnd: d.span.end,
                tags: d.tags.iter().map(|t| format!("{:?}", t)).collect(),
                spanKind: format!("{:?}", d.span_kind),
            })
            .collect())
    }

    /// Returns code actions (quick fixes) available for a file at a given
    /// UTF-16 offset.
    ///
    /// Runs lint rules, then queries the action engine for fixes matching
    /// diagnostics at the given position. Returns an array of code actions
    /// with UTF-16 spans.
    ///
    /// - `canonical_or_alias` — the file to get actions for.
    /// - `offset` — UTF-16 cursor offset in the SFC source.
    #[napi(js_name = "getCodeActions")]
    pub fn get_code_actions(
        &self,
        canonical_or_alias: String,
        offset: u32,
    ) -> Result<Vec<NapiCodeAction>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let actions = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                let byte_offset = utf16_to_byte_offset(source, offset);
                let script = build_script_snapshot(&snapshot);
                let linter = Linter::default();
                let blocks = self.registered_block_facts(&canonical_or_alias);
                let diag_set = linter.lint_with_source(
                    Some(&script),
                    snapshot.template.as_deref(),
                    &snapshot.styles,
                    Some(source),
                    &blocks,
                );

                let engine = ActionEngine::default();
                let ctx = ActionContext {
                    source,
                    file_id: &canonical_or_alias,
                    diagnostics: &diag_set,
                    template: snapshot.template.as_deref(),
                    script: Some(&script),
                    styles: &snapshot.styles,
                    blocks: &blocks,
                };

                let mut actions = Vec::new();
                for diag in diag_set.iter() {
                    if diag.span.start <= byte_offset && byte_offset <= diag.span.end {
                        actions.extend(engine.fixes_for(diag, &ctx));
                    }
                }
                actions.extend(engine.actions_at(byte_offset, &ctx));

                let mut seen = std::collections::HashSet::new();
                actions.retain(|a| seen.insert(a.title.clone()));

                actions
                    .iter()
                    .map(|a| code_action_to_ffi(a, source).into())
                    .collect::<Vec<NapiCodeAction>>()
            }
            _ => Vec::new(),
        };

        Ok(actions)
    }

    /// Returns metadata for all registered lint rules.
    ///
    /// Used by the lint rule browser UI to display available rules,
    /// their categories, and default severities.
    #[napi(js_name = "getLintRuleMetadata")]
    pub fn get_lint_rule_metadata(&self) -> Vec<NapiLintRuleMetadata> {
        let registry = RuleRegistry::default();
        registry
            .rules()
            .iter()
            .map(|rule| lint_rule_to_ffi_metadata(rule.as_ref()).into())
            .collect()
    }

    /// Returns document symbols for a file (outline / Ctrl+Shift+O).
    ///
    /// Generates a hierarchical tree of symbols: SFC blocks at the top,
    /// with script bindings, template components, and style classes as
    /// children. Returns an array of document symbols with UTF-16 spans.
    #[napi(js_name = "getDocumentSymbols")]
    pub fn get_document_symbols(
        &self,
        canonical_or_alias: String,
    ) -> Result<Vec<NapiDocumentSymbol>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let symbols = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => {
                build_document_symbols_from_analysis(&snapshot, source)
                    .into_iter()
                    .map(Into::into)
                    .collect()
            }
            _ => Vec::new(),
        };

        Ok(symbols)
    }

    /// Matches CSS selectors against template elements, returning a
    /// three-valued match matrix.
    ///
    /// Each selector is tested against each template element, producing
    /// "match", "maybe", or "no" results. Used by the CSS selector
    /// matching visualization panel.
    #[napi(js_name = "matchCssSelectors")]
    pub fn match_css_selectors(
        &self,
        canonical_or_alias: String,
    ) -> Result<Vec<NapiSelectorMatchResult>> {
        let analysis = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.get_analysis(&canonical_or_alias)
        }))?;
        let source = self.inner.get_source(&canonical_or_alias);

        let results = match (analysis, source.as_deref()) {
            (Some(snapshot), Some(source)) => build_selector_match_results(&snapshot, source)
                .into_iter()
                .map(Into::into)
                .collect(),
            _ => Vec::new(),
        };

        Ok(results)
    }

    /// host-backed batch compile.
    ///
    /// Compiles a batch of carrier inputs through the production host
    /// path (scheduler + dispatch + compile_cache). Returns one
    /// [`NapiCompileBatchEntry`] per input, in the original input
    /// order.
    ///
    /// Each input's source language is derived from its `canonicalId`,
    /// so the id must carry the carrier's extension: `App.vue` compiles
    /// as Vue and `App.svelte` as Svelte, each through its own carrier.
    /// An id that names no carrier is not compiled into a module.
    ///
    /// Per-input panic isolation: if codegen panics for one input,
    /// only that input's entry receives a `compiler panic: ...`
    /// error message; the rest of the batch completes normally.
    ///
    /// This entry point never runs lint rules. Lint remains an explicit,
    /// independent [`Self::lint`] operation.
    ///
    /// `options.priority` is `"interactive"` or `"background"`;
    /// invalid strings return a NAPI error. Default is `"background"`.
    #[napi(js_name = "compileMany")]
    pub fn compile_many(
        &self,
        files: Vec<NapiCompileBatchInput>,
        options: Option<NapiCompileBatchOptions>,
    ) -> Result<Vec<NapiCompileBatchEntry>> {
        use verter_scheduler::stage::Priority;
        let opts = options.unwrap_or_default();
        let priority = match opts.priority.as_deref() {
            None | Some("background") => Some(Priority::Background),
            Some("interactive") => Some(Priority::Interactive),
            Some(other) => {
                return Err(ffi_err(format!(
                    "invalid priority '{other}', expected 'interactive' or 'background'"
                )));
            }
        };
        let inputs: Vec<host_compile::CompileBatchInput> = files
            .into_iter()
            .map(|f| {
                let requested_mode = f
                    .requestedMode
                    .map(|m| ffi_compile_cache_mode_to_host(&m))
                    .transpose()
                    .map_err(ffi_err)?;
                Ok(host_compile::CompileBatchInput {
                    canonical_id: f.canonicalId,
                    source: std::sync::Arc::from(buffer_to_string(f.source)?),
                    requested_mode,
                    component_id: f.componentId,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let default_mode = opts
            .defaultMode
            .map(|m| ffi_compile_cache_mode_to_host(&m))
            .transpose()
            .map_err(ffi_err)?;
        // The compile lane. Default `host-backed`. FAIL-CLOSED: the
        // RuntimeRender lane REQUIRES an explicit render profile carried on
        // the variant — the host must never substitute production/client
        // defaults for a bundler render. Every output-affecting field of the
        // JS `compileProfile` is threaded so the render output reproduces the
        // `getVirtualFile` path byte-for-byte; an unknown `hmrStrategy` is an
        // error, never a silent drop. HostBacked ignores the profile (its
        // profile is the frozen bundler preset).
        let target = match opts.target.as_deref() {
            None | Some("host-backed") => host_compile::CompileManyTarget::HostBacked,
            Some("runtime-render") => {
                let p = opts.compileProfile.ok_or_else(|| {
                    ffi_err(
                        "compileProfile is required for target 'runtime-render' \
                         (the output-affecting build profile must be explicit; \
                         the host does not substitute defaults)"
                            .to_string(),
                    )
                })?;
                let delimiters =
                    match (p.delimiterOpen, p.delimiterClose) {
                        (Some(open), Some(close)) => Some((open, close)),
                        (None, None) => None,
                        _ => return Err(ffi_err(
                            "compileProfile.delimiterOpen and delimiterClose must be set together"
                                .to_string(),
                        )),
                    };
                let style_processing = match p.styleProcessing.as_deref() {
                    None | Some("complete") => {
                        verter_compiler::compile_request::RuntimeStyleProcessing::Complete
                    }
                    Some("authored-only") => {
                        verter_compiler::compile_request::RuntimeStyleProcessing::AuthoredOnly
                    }
                    Some(other) => {
                        return Err(ffi_err(format!(
                            "invalid compileProfile.styleProcessing '{other}', expected 'complete' or 'authored-only'"
                        )));
                    }
                };
                host_compile::CompileManyTarget::RuntimeRender {
                    profile: host_compile::CompileBatchRenderProfile {
                        style_processing,
                        filename: p.filename,
                        is_production: p.isProduction,
                        custom_element: p.customElement,
                        ssr: p.ssr,
                        force_js: p.forceJs,
                        force_vapor: p.forceVapor,
                        source_map: p.sourceMap,
                        // Tri-state pass-through: an omitted `comments`
                        // stays `None` (compiler default `!isProduction`).
                        comments: p.comments,
                        hmr_strategy: ffi_hmr_strategy_to_host(&p.hmrStrategy).map_err(ffi_err)?,
                        runtime_module_name: p.runtimeModuleName,
                        types_module_name: p.typesModuleName,
                        delimiters,
                        custom_elements: p.customElements,
                        ssr_module_id: p.ssrModuleId,
                    },
                }
            }
            Some(other) => {
                return Err(ffi_err(format!(
                    "invalid target '{other}', expected 'host-backed' or 'runtime-render'"
                )));
            }
        };
        let entries = catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.compile_many(
                inputs,
                host_compile::CompileBatchOptions {
                    priority,
                    default_mode,
                },
                target,
            )
        }))?;
        Ok(entries
            .into_iter()
            .map(|e| {
                // Flatten the typed outcome ARM BY ARM, exhaustively (no
                // wildcard — a new variant is a compile error here, not a
                // silently mis-serialized entry). The JS-visible shape is
                // unchanged: success emits the product with an empty error
                // list, failure emits an empty product with the errors.
                let (code, source_map, lang, errors, diagnostics) = match e.outcome {
                    host_compile::CompileBatchOutcome::Produced {
                        code,
                        lang,
                        source_map,
                        diagnostics,
                    } => (
                        code.to_string(),
                        source_map.map(|s| s.to_string()),
                        lang,
                        Vec::new(),
                        diagnostics.iter().map(napi_diagnostic_from_host).collect(),
                    ),
                    host_compile::CompileBatchOutcome::Failed { errors } => {
                        (String::new(), None, None, errors.into_vec(), Vec::new())
                    }
                };
                NapiCompileBatchEntry {
                    canonicalId: e.canonical_id,
                    code,
                    sourceMap: source_map,
                    lang,
                    errors,
                    diagnostics,
                    durationMs: e.duration_ms,
                    cacheHit: e.cache_hit,
                    requestedMode: e.requested_mode.to_string(),
                    actualMode: e.actual_mode.to_string(),
                    downgradeReason: e.downgrade_reason.map(|r| r.to_string()),
                }
            })
            .collect())
    }

    // =========================================================================
    // Typed audit entry-points
    //
    // Each entry-point wraps a `VerterHost::*_with_audit` Rust producer
    // and returns the produced `RequestAuditRecord` as a JSON Buffer.
    // Helper types and parsing free functions live in `crate::audit`.
    //
    // The methods MUST live in this `impl NapiVerterHost` block (not a
    // sibling module) so the napi-derive class registration picks up
    // the `js_name = "VerterHost"` rename declared on the struct in
    // this same file.
    // =========================================================================

    /// Run a single type-resolution query through the shared dispatch
    /// and return the produced `RequestAuditRecord` as a JSON
    /// `Buffer`. The query resolves `decl_name` in the top-level
    /// ordinary module scope of `canonical_id`. Returns `null` when audit is
    /// disabled.
    #[napi(js_name = "resolveTypeWithAudit")]
    pub fn resolve_type_with_audit(
        &self,
        canonical_id: String,
        decl_name: String,
    ) -> Result<Option<Buffer>> {
        use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: ScopeId::file(
                    std::sync::Arc::<str>::from(canonical_id.as_str()),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: std::sync::Arc::<str>::from(decl_name.as_str()),
            });
            let record = host
                .resolve_type_with_audit(key, &canonical_id)
                .audit()
                .clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Compile `canonical_id` for the requested codegen target and
    /// return the produced `RequestAuditRecord` as a JSON `Buffer`.
    /// Accepted target names: `BUNDLER`, `IDE`, `ANALYSIS`, `META`,
    /// `TSX`, `TSC`. Returns `null` when audit is disabled.
    #[napi(js_name = "compileWithAudit")]
    pub fn compile_with_audit(
        &self,
        canonical_id: String,
        target: String,
    ) -> Result<Option<Buffer>> {
        let target = audit::parse_compile_target(&target)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host
                .compile_with_audit(&canonical_id, target)
                .audit()
                .clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Materialise the `AnalysisReady` artifact for `canonical_id`
    /// under audit and return the produced `RequestAuditRecord` as a
    /// JSON `Buffer`. Returns `null` when audit is disabled or the
    /// canonical does not exist.
    #[napi(js_name = "analyzeWithAudit")]
    pub fn analyze_with_audit(&self, canonical_id: String) -> Result<Option<Buffer>> {
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host.analyze_with_audit(&canonical_id).audit().clone();
            audit::encode_stored_record(&record)
        }))?
    }

    /// Drive a workspace operation under audit and return the
    /// produced `RequestAuditRecord` as a JSON `Buffer`. The `op`
    /// argument is shaped as `{ type: "AuditResolve", specifier, from
    /// }` / `{ type: "DepGraphTraverse", root }` / `{ type:
    /// "ResolverWalk", specifier }`. Always returns a record.
    #[napi(js_name = "auditWorkspaceOp")]
    pub fn audit_workspace_op(&self, op: audit::NapiWorkspaceOp) -> Result<Buffer> {
        let arg = op.try_into_workspace_op()?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let record = host.audit_workspace_op(arg);
            audit::encode_record(&record)
        }))?
    }

    /// Drain the most-recent `RequestAuditRecord` from the host's
    /// audit store. Returns `null` when the store is empty. Drains
    /// the entry: a second call after a single insert returns null.
    #[napi(js_name = "getLastAuditRecord")]
    pub fn get_last_audit_record(&self) -> Result<Option<Buffer>> {
        use verter_audit::batch::AuditRecordSource;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut latest_id: Option<u64> = None;
            let mut latest_at: Option<std::time::Instant> = None;
            store.for_each_record(&mut |inserted_at, record| {
                let is_newer = match latest_at {
                    None => true,
                    Some(prev) => inserted_at > prev,
                };
                if is_newer {
                    latest_at = Some(inserted_at);
                    latest_id = Some(record.request_id);
                }
            });
            let Some(id) = latest_id else {
                return Ok(None);
            };
            match store.take(id) {
                Some(rec) => audit::encode_record(&rec).map(Some),
                None => Ok(None),
            }
        }))?
    }

    /// Non-destructive filtered query over the host's audit store.
    /// Returns a JSON-serialised array of records (`Buffer`).
    #[napi(js_name = "getAuditRecords")]
    pub fn get_audit_records(
        &self,
        filter: Option<audit::NapiAuditRecordFilter>,
    ) -> Result<Buffer> {
        use verter_audit::batch::AuditRecordSource;
        let filter = filter.unwrap_or_default();
        let kind_filter = filter.kind;
        let since = match filter.since_request_id.as_deref() {
            Some(s) => Some(audit::parse_request_id_str(s)?),
            None => None,
        };
        let limit = filter.limit.map(|n| n as usize);
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            let mut collected: Vec<verter_audit::RequestAuditRecord> = Vec::new();
            store.for_each_record(&mut |_inserted_at, record| {
                if let Some(filter_kind) = kind_filter.as_deref() {
                    if !audit::kind_matches(filter_kind, &record.kind) {
                        return;
                    }
                }
                if let Some(since_id) = since {
                    if record.request_id <= since_id {
                        return;
                    }
                }
                collected.push(record.clone());
            });
            if let Some(n) = limit {
                collected.truncate(n);
            }
            audit::encode_record_list(&collected)
        }))?
    }

    /// Run the bundler-batch aggregator over the host's audit store
    /// and return the produced `BundlerBatchPayload` as a JSON
    /// `Buffer`. The summary tags the payload with the requested
    /// bundler kind (defaults to `Vite`).
    #[napi(js_name = "getBundlerBatchSummary")]
    pub fn get_bundler_batch_summary(
        &self,
        args: Option<audit::NapiBundlerBatchSummaryArgs>,
    ) -> Result<Buffer> {
        use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator};
        let args = args.unwrap_or_default();
        let kind = audit::parse_bundler_kind(args.kind.as_deref());
        let since_id = match args.since_request_id.as_deref() {
            Some(s) => Some(audit::parse_request_id_str(s)?),
            None => None,
        };
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let store = host.host_audit_runtime().audit_records_store();
            // The aggregator keys its `since` filter by `Instant`,
            // but we accept a request-id watermark from JS callers
            // (instants do not survive a JSON round-trip). Walk the
            // store once to find the most-recent `inserted_at` whose
            // request_id is `<= since_id`; an unmatched watermark
            // (id newer than anything in the store) yields `None` —
            // equivalent to "no records pass the filter".
            let since_instant: Option<std::time::Instant> = match since_id {
                None => None,
                Some(target_id) => {
                    let mut best: Option<std::time::Instant> = None;
                    store.for_each_record(&mut |inserted_at, record| {
                        if record.request_id <= target_id {
                            best = match best {
                                None => Some(inserted_at),
                                Some(prev) if inserted_at > prev => Some(inserted_at),
                                Some(prev) => Some(prev),
                            };
                        }
                    });
                    best
                }
            };
            let aggregator = BatchAuditAggregator::new(store.as_ref(), kind);
            let payload = aggregator.summarize(since_instant);
            let bytes = serde_json::to_vec(&payload).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("bundler batch summary serialization error: {e}"),
                )
            })?;
            Ok(Buffer::from(bytes))
        }))?
    }

    // =========================================================================
    // Typeinfo entry-points (typeinfo public host substrate)
    //
    // Wrap the host substrate methods
    // (`list_file_symbols`, `resolve_named_symbol_with_audit`,
    // `evaluate_type_expression_with_audit`) and project the host
    // outputs back across the FFI boundary.
    //
    // - `listSymbols` returns a JSON Buffer carrying a `Vec<FfiSymbolEntry>`.
    // - `resolveSymbolWithAudit` and `evaluateTypeExpressionWithAudit`
    //   return a `NapiTypeInfoResolveResult { typeExpr, auditRecord }`
    //   — both are JSON Buffers; consumers decode whichever they need.
    //
    // Audit emission follows the typeinfo contract: when
    // `auditEnabled = true` the underlying host method publishes
    // exactly one `RequestAuditRecord` to the host's audit store and
    // also returns the cloned record on the call stack. The
    // `auditRecord` field on `NapiTypeInfoResolveResult` carries that
    // record without polling the audit store; the store-based
    // `getLastAuditRecord` continues to work too.
    // =========================================================================

    /// Return the top-level symbol inventory for `canonical_id`.
    ///
    /// JSON Buffer carrying a `Vec<FfiSymbolEntry>` per the FFI mirror
    /// in `verter_protocol::typeinfo`. The call is bounded by the
    /// shallow-state size and does not emit an audit record (per §17
    /// "no audit; pure shallow read").
    #[napi(js_name = "listSymbols")]
    pub fn list_symbols(&self, canonical_id: String) -> Result<Buffer> {
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let entries = host.list_file_symbols(&canonical_id);
            let ffi: Vec<verter_protocol::typeinfo::FfiSymbolEntry> = entries
                .into_iter()
                .map(verter_ffi::convert::host_to_ffi_symbol_entry)
                .collect();
            typeinfo::encode_symbol_list(&ffi)
        }))?
    }

    /// Resolve `name` in `canonical_id`'s top-level scope and return
    /// the raised `TypeExpr` plus the produced `RequestAuditRecord`.
    ///
    /// `type_args` is an optional JSON Buffer carrying an array of
    /// `TypeExpr` values (the wire form of `TypeExprList`). Empty /
    /// missing means "no generic instantiation".
    ///
    /// `mode` is one of the canonical projection-mode tags
    /// (`"identity" | "navigate" | "shallow" | "expanded" |
    /// "skeleton"`). Pass `null` to take the host's default per §5.2.
    ///
    /// `typeExpr` is `null` when the symbol could not be resolved
    /// (unknown decl, boundary lowering miss, suppressed by host
    /// policy). `auditRecord` is `null` when `auditEnabled = false`;
    /// a boundary lowering miss resolves INSIDE the audited request,
    /// so it still carries the audit record — an absent record is
    /// reserved for failures before a semantic request exists (a
    /// malformed wire payload fails decoding and surfaces as an
    /// error, not a result).
    #[napi(js_name = "resolveSymbolWithAudit")]
    pub fn resolve_symbol_with_audit(
        &self,
        canonical_id: String,
        name: String,
        type_args: Option<Buffer>,
        mode: Option<String>,
    ) -> Result<typeinfo::NapiTypeInfoResolveResult> {
        let exprs = typeinfo::decode_type_expr_list(type_args)?;
        let resolve_mode = typeinfo::parse_resolve_mode(mode)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let arc_args: Vec<std::sync::Arc<TypeExpr>> =
                exprs.into_iter().map(std::sync::Arc::new).collect();
            // Wire-boundary resolution: the symbolic `TypeExpr` payloads
            // lower to semantic-graph node ids INSIDE the audited request,
            // under the SAME store view the resolution runs against (the
            // transient symbolic IR stops at this boundary). A lowering miss
            // surfaces as a `null` typeExpr WITH its audit record.
            let (outcome, record) = host
                .resolve_named_symbol_wire_with_audit(&canonical_id, &name, &arc_args, resolve_mode)
                .into_parts();
            let (resolved, error) = typeinfo::split_resolve_outcome(outcome);
            // The session-owned bytes facade encodes the `TypeExpr` to wire
            // JSON internally (the reverse materialization runs through the
            // sealed output capability inside `verter_session`); the FFI
            // adapter only wraps the bytes in a `Buffer`.
            let type_expr_buf = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr_json_bytes(node_id)
                    .map(Buffer::from),
                None => None,
            };
            let audit_buf = typeinfo::encode_stored_audit_record(&record)?;
            Ok(typeinfo::NapiTypeInfoResolveResult {
                typeExpr: type_expr_buf,
                auditRecord: audit_buf,
                error,
            })
        }))?
    }

    /// Evaluate a synthetic type expression in a file scope and return
    /// the raised `TypeExpr` plus the produced `RequestAuditRecord`.
    ///
    /// `request` is a JSON Buffer carrying a
    /// `verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest`.
    /// See `EvaluateTypeExpressionRequest` for the host shape.
    ///
    /// `typeExpr` is `null` when the expression could not be resolved.
    /// `auditRecord` is `null` when audit is disabled.
    #[napi(js_name = "evaluateTypeExpressionWithAudit")]
    pub fn evaluate_type_expression_with_audit(
        &self,
        request: Buffer,
    ) -> Result<typeinfo::NapiTypeInfoResolveResult> {
        let req = typeinfo::decode_evaluate_request(request)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let (outcome, record) = host.evaluate_type_expression_with_audit(req).into_parts();
            let (resolved, error) = typeinfo::split_resolve_outcome(outcome);
            // Bytes facade: the `TypeExpr` is wire-encoded inside
            // `verter_session` through the sealed output capability; the FFI
            // adapter only wraps the bytes in a `Buffer`.
            let type_expr_buf = match resolved {
                Some(node_id) => host
                    .project_node_to_type_expr_json_bytes(node_id)
                    .map(Buffer::from),
                None => None,
            };
            let audit_buf = typeinfo::encode_stored_audit_record(&record)?;
            Ok(typeinfo::NapiTypeInfoResolveResult {
                typeExpr: type_expr_buf,
                auditRecord: audit_buf,
                error,
            })
        }))?
    }

    /// Resolve a component's framework surfaces and return the wire
    /// `TypeInfoGraphResponse` plus the per-request audit record.
    ///
    /// `request` is a protobuf-encoded
    /// `verter_protocol::typeinfo::graph::TypeInfoGraphRequest` envelope
    /// carrying the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation (the
    /// framework-surface operation rides the existing graph envelope — no
    /// dedicated request type). The host runs the envelope validator FIRST,
    /// so a malformed envelope returns the typed wire `error` arm in
    /// `response` BEFORE any registry lookup or semantic dispatch.
    ///
    /// `response` is the protobuf-encoded `TypeInfoGraphResponse` — the
    /// `framework_surface` arm on success, the `error` arm on a typed
    /// rejection — and is ALWAYS present (the validation-first executor
    /// always produces a typed response). `auditRecord` is `null` when
    /// audit is disabled / filtered; the audit envelope rides BOTH the
    /// success AND the rejection outcome.
    #[napi(js_name = "resolveFrameworkSurfaceWithAudit")]
    pub fn resolve_framework_surface_with_audit(
        &self,
        request: Buffer,
    ) -> Result<typeinfo::NapiFrameworkSurfaceResult> {
        let envelope = typeinfo::decode_type_info_graph_request(request)?;
        let host = std::sync::Arc::clone(&self.inner);
        catch_panic(std::panic::AssertUnwindSafe(move || {
            let (outcome, record) = host
                .resolve_framework_surface_with_audit(envelope)
                .into_parts();
            // The validation-first executor always yields a typed wire
            // response: the `framework_surface` arm on success, the
            // `error` arm on a typed rejection. The `AuditedResult` Err
            // arm drops the (error-arm) response, so re-form it here so
            // the JS side always decodes a `TypeInfoGraphResponse`.
            let response = match outcome {
                Ok(response) => response,
                Err(error) => typeinfo::framework_error_response(error),
            };
            let response_buf = typeinfo::encode_type_info_graph_response(&response);
            let audit_buf = typeinfo::encode_stored_audit_record(&record)?;
            Ok(typeinfo::NapiFrameworkSurfaceResult {
                response: response_buf,
                auditRecord: audit_buf,
            })
        }))?
    }
}

// =============================================================================
// Shared helpers (parity with verter_wasm)
// =============================================================================

/// Build a `ScriptAnalysisSnapshot` from a `FileAnalysisSnapshot`.
///
/// Extracts all script-related fields, preserving `vue_api_calls` and
/// `dom_query_calls` from the snapshot.
fn build_script_snapshot(
    snapshot: &host::FileAnalysisSnapshot,
) -> verter_semantic::analysis::types::ScriptAnalysisSnapshot {
    verter_semantic::analysis::types::ScriptAnalysisSnapshot {
        imports: snapshot.imports.clone(),
        module_references: snapshot.module_references.to_vec(),
        bindings: snapshot.bindings.clone(),
        macros: snapshot.macros.to_vec(),
        macro_type_deps: snapshot.macro_type_deps.to_vec(),
        flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
            snapshot.script_flags,
        ),
        exported_functions: Vec::new(),
        vue_api_calls: snapshot.vue_api_calls.to_vec(),
        dom_query_calls: snapshot.dom_query_calls.to_vec(),
        css_var_manipulations: snapshot.css_var_manipulations.to_vec(),
        script_binding_occurrences: snapshot.script_binding_occurrences.to_vec(),
        macro_usage: snapshot.macro_usage.clone(),
        style_vbind_roots: snapshot.style_vbind_roots.clone(),
        store_usages: snapshot.store_usages.to_vec(),
        store_definitions: snapshot.store_definitions.to_vec(),
        first_await_offset: None,
        type_enhancements: None,
        options_api: snapshot.options_api.clone(),
        nested_macro_calls: Vec::new(),
        is_typescript: snapshot.is_typescript,
        declaration_entries: Vec::new(),
    }
}

/// Convert a UTF-16 offset to a UTF-8 byte offset.
fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> u32 {
    verter_ffi::convert::utf16_to_byte_offset(source, utf16_offset)
}

/// Safe UTF-16 conversion that handles 0 as identity.
fn byte_offset_to_utf16_safe(source: &str, byte_offset: u32) -> u32 {
    verter_ffi::convert::byte_offset_to_utf16(source, byte_offset)
}

/// Monaco SymbolKind constants (subset used for document symbols).
mod symbol_kind {
    pub const MODULE: u32 = 1;
    pub const VARIABLE: u32 = 12;
    pub const FUNCTION: u32 = 11;
    pub const CLASS: u32 = 4;
    pub const STRUCT: u32 = 22;
    pub const PROPERTY: u32 = 6;
    pub const KEY: u32 = 19;
}

/// Build document symbols from analysis data.
fn build_document_symbols_from_analysis(
    snapshot: &host::FileAnalysisSnapshot,
    source: &str,
) -> Vec<FfiDocumentSymbol> {
    let mut symbols = Vec::new();

    if !snapshot.bindings.is_empty() || !snapshot.imports.is_empty() || !snapshot.macros.is_empty()
    {
        let mut children = Vec::new();

        for imp in &snapshot.imports {
            children.push(FfiDocumentSymbol {
                name: imp.source.clone(),
                detail: if imp.is_type_only {
                    Some("type import".to_string())
                } else {
                    None
                },
                kind: symbol_kind::MODULE,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                children: Vec::new(),
            });
        }

        for binding in &snapshot.bindings {
            let kind = match binding.kind {
                verter_semantic::analysis::AnalyzedBindingKind::Function
                | verter_semantic::analysis::AnalyzedBindingKind::AsyncFunction => {
                    symbol_kind::FUNCTION
                }
                verter_semantic::analysis::AnalyzedBindingKind::Class => symbol_kind::CLASS,
                _ => symbol_kind::VARIABLE,
            };
            children.push(FfiDocumentSymbol {
                name: binding.name.clone(),
                detail: binding.type_annotation.clone(),
                kind,
                span_start: byte_offset_to_utf16_safe(source, binding.span.start),
                span_end: byte_offset_to_utf16_safe(source, binding.span.end),
                selection_start: byte_offset_to_utf16_safe(source, binding.span.start),
                selection_end: byte_offset_to_utf16_safe(source, binding.span.end),
                children: Vec::new(),
            });
        }

        for m in snapshot.macros.iter() {
            children.push(FfiDocumentSymbol {
                name: format!("{:?}", m.kind),
                detail: if m.is_type_based {
                    Some("type-based".to_string())
                } else {
                    None
                },
                kind: symbol_kind::FUNCTION,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "script".to_string(),
            detail: Some(format!(
                "{} binding(s), {} import(s)",
                snapshot.bindings.len(),
                snapshot.imports.len()
            )),
            kind: symbol_kind::MODULE,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    if let Some(template) = &snapshot.template {
        let mut children = Vec::new();

        for comp in &template.components {
            children.push(FfiDocumentSymbol {
                name: comp.name.clone(),
                detail: Some(format!("{} prop(s)", comp.props.len())),
                kind: symbol_kind::CLASS,
                span_start: byte_offset_to_utf16_safe(source, comp.span.start),
                span_end: byte_offset_to_utf16_safe(source, comp.span.end),
                selection_start: byte_offset_to_utf16_safe(source, comp.span.start),
                selection_end: byte_offset_to_utf16_safe(source, comp.span.end),
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "template".to_string(),
            detail: Some(format!("{} component(s)", template.components.len())),
            kind: symbol_kind::STRUCT,
            span_start: 0,
            span_end: source.encode_utf16().count() as u32,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    for (i, style) in snapshot.styles.iter().enumerate() {
        let mut children = Vec::new();

        if let Some(css) = &style.css {
            for class in &css.classes {
                children.push(FfiDocumentSymbol {
                    name: format!(".{}", class.name),
                    detail: None,
                    kind: symbol_kind::PROPERTY,
                    span_start: byte_offset_to_utf16_safe(source, class.span.start),
                    span_end: byte_offset_to_utf16_safe(source, class.span.end),
                    selection_start: byte_offset_to_utf16_safe(source, class.span.start),
                    selection_end: byte_offset_to_utf16_safe(source, class.span.end),
                    children: Vec::new(),
                });
            }
        }

        symbols.push(FfiDocumentSymbol {
            name: format!(
                "style{}{}",
                if i > 0 {
                    format!(" {}", i)
                } else {
                    String::new()
                },
                if style.scoped { " (scoped)" } else { "" }
            ),
            detail: None,
            kind: symbol_kind::KEY,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            children,
        });
    }

    symbols
}

/// Build CSS selector match results for visualization.
fn build_selector_match_results(
    snapshot: &host::FileAnalysisSnapshot,
    source: &str,
) -> Vec<FfiSelectorMatchResult> {
    let template = match &snapshot.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut results = Vec::new();

    for style in snapshot.styles.iter() {
        let css = match &style.css {
            Some(c) => c,
            None => continue,
        };

        for selector in &css.selectors {
            let parsed = match &selector.structure {
                Some(s) => s.clone(),
                None => continue,
            };

            let mut matches = Vec::new();
            for (idx, element) in template.elements.iter().enumerate() {
                let result = verter_semantic::analysis::selector_match::match_selector(
                    &parsed,
                    idx,
                    &template.elements,
                );
                matches.push(FfiElementMatch {
                    tag: element.tag.clone(),
                    span_start: byte_offset_to_utf16_safe(source, element.span.start),
                    span_end: byte_offset_to_utf16_safe(source, element.span.end),
                    result: match result {
                        verter_semantic::analysis::selector_match::MatchResult::Matches => {
                            "match".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::MaybeMatches => {
                            "maybe".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::NoMatch => {
                            "no".to_string()
                        }
                    },
                });
            }

            results.push(FfiSelectorMatchResult {
                selector_text: selector.text.clone(),
                selector_start: byte_offset_to_utf16_safe(source, selector.span.start),
                selector_end: byte_offset_to_utf16_safe(source, selector.span.end),
                matches,
            });
        }
    }

    results
}

// =============================================================================
// host-backed batch compile (NAPI surface)
//
// `NapiVerterHost::compile_many` is the canonical batch-compile
// entry point. It routes through the host's scheduler + dispatch +
// compile_cache and preserves the read/parse/process-once
// invariant.
// =============================================================================

use verter_session::host_compile;

/// One file in a batch compile call.
#[napi(object)]
pub struct NapiCompileBatchInput {
    pub canonicalId: String,
    pub source: Buffer,
    /// Requested compile cache mode ("stateless" / "content" /
    /// "session"). `None` inherits the batch `defaultMode`.
    pub requestedMode: Option<String>,
    /// Explicit per-component scoped-style / HMR id. Threaded into this
    /// input's compile profile ONLY for a public RuntimeRender request
    /// (scoped-style / HMR identity is per-component, not per-build). Vue uses
    /// it in the private render worker; Svelte uses it on the effective
    /// host-backed route. `None` lets codegen auto-generate the id.
    pub componentId: Option<String>,
}

/// The batch-level render profile for a public RuntimeRender request (JS
/// mirror of [`host_compile::CompileBatchRenderProfile`]). Every field is
/// output-affecting and uniform across a single bundler build. This carries
/// the full output-affecting projection of the JS `HostCompileProfile`: Vue's
/// private render worker reproduces the `getVirtualFile` output byte-for-byte,
/// while Svelte keeps the effective host-backed path.
#[napi(object)]
pub struct NapiCompileBatchRenderProfile {
    /// Style stages owned by this render: `"complete"` (default) or
    /// `"authored-only"` when the bundler's separate style-module lane owns
    /// preprocessing and every plain-CSS-only continuation.
    pub styleProcessing: Option<String>,
    /// Codegen filename override (component-name extraction, scope-id
    /// derivation, source-map `source`/`file`). Absent falls back to the
    /// canonical id — same semantics as `HostCompileProfile.filename`.
    pub filename: Option<String>,
    pub isProduction: bool,
    /// Vue custom-element script policy. Independent of `customElements`.
    pub customElement: bool,
    pub ssr: bool,
    pub forceJs: bool,
    pub forceVapor: bool,
    pub sourceMap: bool,
    /// Preserve template comments. TRI-STATE: absent keeps the compiler
    /// default (`!isProduction` — dev preserves, prod strips), same
    /// semantics as an absent `HostCompileProfile.comments`. Do NOT
    /// collapse an omitted value to `false`.
    pub comments: Option<bool>,
    /// HMR strategy: "none" | "vite" | "webpack".
    pub hmrStrategy: String,
    /// Runtime module import specifier (e.g. "vue").
    pub runtimeModuleName: Option<String>,
    /// Types module import specifier.
    pub typesModuleName: Option<String>,
    /// Custom interpolation delimiters — open. Must be set together with
    /// `delimiterClose`.
    pub delimiterOpen: Option<String>,
    /// Custom interpolation delimiters — close.
    pub delimiterClose: Option<String>,
    /// Custom-element tag names (affect template codegen).
    pub customElements: Option<Vec<String>>,
    /// SSR asset-collection module id registered on `ssrContext.modules`.
    /// Vite's ssr-manifest keys are ROOT-RELATIVE — the plugin supplies
    /// `normalizePath(relative(root, filename))`; absent falls back to the
    /// canonical id.
    pub ssrModuleId: Option<String>,
}

/// Caller-configurable options for [`NapiVerterHost::compile_many`].
#[napi(object)]
#[derive(Default)]
pub struct NapiCompileBatchOptions {
    /// Scheduler priority for batch upserts. Default: `"background"`.
    /// Use `"interactive"` when there is no concurrent interactive
    /// work (benchmarks / CI cold-start measurement).
    pub priority: Option<String>,
    /// Default compile cache mode for inputs whose `requestedMode` is
    /// unset. `None` resolves to "session" (the host default).
    pub defaultMode: Option<String>,
    /// The compile request: `"host-backed"` (default) runs the full session
    /// wrapper; `"runtime-render"` uses Vue's private render-only worker or a
    /// non-Vue carrier's effective host-backed route. RuntimeRender REQUIRES
    /// `compileProfile` (fail-closed).
    pub target: Option<String>,
    /// The batch-level render profile for the `"runtime-render"` lane. It
    /// is REQUIRED for that lane (the NAPI conversion fails closed when it
    /// is absent) — every output-affecting field must be explicit; the
    /// host must not substitute production/client defaults. Ignored by the
    /// `"host-backed"` lane.
    pub compileProfile: Option<NapiCompileBatchRenderProfile>,
}

/// Result for a single original input position.
#[napi(object)]
pub struct NapiCompileBatchEntry {
    pub canonicalId: String,
    pub code: String,
    pub sourceMap: Option<String>,
    /// The compiled `Main` module language ("ts" / "js" / "jsx"), or
    /// `None` on an error/panic outcome. Bundler consumers (vite
    /// sub-request routing) read it.
    pub lang: Option<String>,
    /// All compilation errors for this file. Empty on success.
    pub errors: Vec<String>,
    /// Non-fatal WARNING-severity diagnostics surfaced on a SUCCESSFUL
    /// compile, separate from the fatal `errors`. Populated by the private Vue
    /// render worker's soft-macro contract (an unresolved imported macro type
    /// renders successfully and reports a warning here). Always empty on the
    /// HostBacked lane, the effective Svelte host-backed route, and any fatal
    /// outcome.
    pub diagnostics: Vec<NapiDiagnostic>,
    pub durationMs: f64,
    /// `true` iff this input was served from a warm cache entry under its
    /// classified mode — the fact-validated session slot (`Session`) or
    /// the content-addressed store (`Content`) — as decided by the single
    /// mode classifier. A request that a reason downgraded to `Stateless`
    /// never warm-hits and reports `false`. Sourced directly from the
    /// Rust `CompileBatchEntry.cache_hit` on the compile response.
    pub cacheHit: bool,
    /// Requested compile cache mode ("stateless" / "content" / "session").
    pub requestedMode: String,
    /// Actual compile cache mode the runtime ran under.
    pub actualMode: String,
    /// Highest-priority downgrade reason, or `None` when none fired.
    pub downgradeReason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch_entry(canonical_id: &str) -> host_compile::CompileRequestBatchEntry {
        host_compile::CompileRequestBatchEntry {
            canonical_id: canonical_id.to_string(),
            outcome: Ok(host::CompileRequestResponse {
                canonical_id: canonical_id.to_string(),
                diagnostics: host::DiagnosticsSnapshot::default(),
                products: Vec::new(),
            }),
        }
    }

    // Mutation recipe: replace the count comparison in
    // `batch_entry_count_mismatch` with `false`. `zip` truncates to the
    // shorter side, so the dropped and duplicated cases below both report
    // "no mismatch" while the caller's output slots go unfilled.
    #[test]
    fn batch_count_check_catches_a_lost_or_duplicated_entry() {
        let expected = vec![
            "/a.vue".to_string(),
            "/b.vue".to_string(),
            "/c.vue".to_string(),
        ];

        assert_eq!(
            batch_entry_count_mismatch(
                &[
                    batch_entry("/a.vue"),
                    batch_entry("/b.vue"),
                    batch_entry("/c.vue")
                ],
                &expected,
            ),
            None,
            "the executor's own contract must not be reported as a mismatch"
        );

        // A transposition has the right count: it is attributable to the
        // position it landed on, so it is NOT a whole-call failure.
        assert_eq!(
            batch_entry_count_mismatch(
                &[
                    batch_entry("/a.vue"),
                    batch_entry("/c.vue"),
                    batch_entry("/b.vue")
                ],
                &expected,
            ),
            None,
            "a same-count transposition is failed per entry, not per call"
        );

        // Dropped: the prefix still matches at every overlapping position.
        let dropped =
            batch_entry_count_mismatch(&[batch_entry("/a.vue"), batch_entry("/b.vue")], &expected)
                .expect("a batch short one entry is a mismatch");
        assert!(dropped.contains("2 entries for 3 inputs"), "{dropped}");

        // Duplicated: likewise, the expected side is what runs out first.
        let duplicated = batch_entry_count_mismatch(
            &[
                batch_entry("/a.vue"),
                batch_entry("/b.vue"),
                batch_entry("/c.vue"),
                batch_entry("/c.vue"),
            ],
            &expected,
        )
        .expect("a batch with an extra entry is a mismatch");
        assert!(
            duplicated.contains("4 entries for 3 inputs"),
            "{duplicated}"
        );
    }

    /// A transposed position names both ids, so a caller can tell a
    /// mispairing from a compile failure.
    ///
    /// Mutation recipe: swap the two arguments at the
    /// `batch_entry_position_mismatch` call site in `compile_requests`.
    /// The message then blames the input for the executor's answer and
    /// this case goes red.
    #[test]
    fn a_transposed_position_names_the_answered_and_the_expected_id() {
        let message = batch_entry_position_mismatch("/c.vue", "/b.vue");
        assert!(
            message.contains("'/c.vue' at the position expected for '/b.vue'"),
            "{message}"
        );
    }

    // @ai-generated - A runtime envelope carrying a non-runtime kind must fail closed.
    #[test]
    fn runtime_product_refuses_a_kind_without_a_runtime_wire_arm() {
        let response = host::CompileRequestResponse {
            canonical_id: "/src/App.vue".to_string(),
            diagnostics: host::DiagnosticsSnapshot::default(),
            products: vec![host::CompiledProduct::Runtime {
                kind: verter_compiler::compile_request::ProductKind::Analysis,
                nodes: Vec::new(),
            }],
        };

        let error = match compile_request_response_to_napi(response, "") {
            Ok(_) => panic!("a non-runtime kind cannot be published as a runtime product"),
            Err(error) => error,
        };
        assert!(error.reason.contains("analysis"), "{}", error.reason);
    }

    // Mutation recipe: set `hasErrors: true` on `empty_diagnostics_snapshot`.
    // This case then fails because a framework mismatch carries no diagnostics.
    #[test]
    fn empty_terminal_failure_snapshot_does_not_claim_errors() {
        let failure = host::CompileRequestFailure::FrameworkMismatch {
            canonical_id: "/src/App.vue".to_string(),
            requested: "svelte",
            registered: "vue".to_string(),
        };
        let projected = compile_request_failure_to_napi(failure, "/src/App.vue".to_string(), None);
        assert!(!projected.diagnostics.hasErrors);
        assert!(projected.diagnostics.diagnostics.is_empty());
    }

    // Mutation recipe: format construction refusals with `{error:?}`.
    // The public message then contains `DuplicateProduct(RuntimeClient)`.
    #[test]
    fn construction_refusal_uses_the_wire_product_name() {
        let message = compile_request_construction_refused(
            &verter_compiler::compile_request::CompileRequestError::DuplicateProduct(
                verter_compiler::compile_request::ProductKind::RuntimeClient,
            ),
        );
        assert!(
            message.contains("duplicate product 'runtimeClient'"),
            "{message}"
        );
        assert!(
            !message.contains("DuplicateProduct") && !message.contains("RuntimeClient"),
            "{message}"
        );
    }

    /// The refusal a real caller can actually provoke names the field
    /// they wrote on the request object.
    ///
    /// Both options here are genuinely reachable: they are the two
    /// `compatConfig` rows the request carries as the two DISTINCT slots
    /// `compatConfig` and `transformCompatConfig`, and both are refused
    /// on presence by `VueOptionAttempt::into_request`. A case built on an
    /// option no refusal can name would assert nothing about what a caller
    /// is ever shown.
    ///
    /// Mutation recipes:
    /// - Derive the path from the option INVENTORY row (strip the
    ///   surface, keep any dotted tail): `transformCompatConfig` renders
    ///   as `vue:compatConfig`, so the second assertion pair goes red and
    ///   the two options stop being distinguishable.
    /// - Case-lower `format!("{option:?}")` in `FrameworkOption`'s
    ///   `Display`: the message reads `vue:parserOptionsCompatConfig`,
    ///   naming a field no request object has, and every assertion here
    ///   goes red.
    #[test]
    fn unsupported_option_refusal_names_the_request_field() {
        use verter_compiler::compile_request::{CompileRequestError, FrameworkOption, VueOption};
        let refusal = |option| {
            compile_request_construction_refused(&CompileRequestError::UnsupportedOption {
                option: FrameworkOption::Vue(option),
                capability: None,
            })
        };

        let parser = refusal(VueOption::ParserOptionsCompatConfig);
        assert!(
            parser.contains("unsupported option 'vue:compatConfig'"),
            "{parser}"
        );

        // The other inventory surface's `compatConfig` is a DIFFERENT
        // request field; telling a caller to remove `compatConfig` when
        // they wrote `transformCompatConfig` names a field they never set.
        let transform = refusal(VueOption::TransformOptionsCompatConfig);
        assert!(
            transform.contains("unsupported option 'vue:transformCompatConfig'"),
            "{transform}"
        );
        assert_ne!(parser, transform);

        for message in [&parser, &transform] {
            assert!(
                !message.contains("ParserOptions")
                    && !message.contains("TransformOptions")
                    && !message.contains("compiler-core"),
                "{message}"
            );
        }
    }

    /// The typed FAILURE projection maps its diagnostic spans to UTF-16
    /// against the paired source, as the success projection does — a
    /// separate call path, so a separate proof.
    ///
    /// Mutation recipe: pass `None` instead of `Some("a😀b")` as the
    /// source below. The span stays at its UTF-8 byte offsets (1, 5) and
    /// this case goes red.
    #[test]
    fn a_typed_failure_maps_its_diagnostic_spans_to_utf16() {
        let failure = host::CompileRequestFailure::Host(host::HostError::CompileError(
            host::CompileFailure {
                diagnostics: host::DiagnosticsSnapshot {
                    diagnostics: vec![host::HostDiagnostic {
                        severity: host::HostSeverity::Error,
                        code: "E_SPAN".to_string(),
                        message: "spans a surrogate pair".to_string(),
                        arguments: Vec::new(),
                        span: verter_span::Span::new(1, 5),
                    }],
                    has_errors: true,
                },
                requested_mode: host::CompileCacheMode::Stateless,
                actual_mode: host::CompileCacheMode::Stateless,
                downgrade_reason: None,
            },
        ));
        let projected =
            compile_request_failure_to_napi(failure, "/src/A.vue".to_string(), Some("a😀b"));
        let diagnostic = &projected.diagnostics.diagnostics[0];
        assert_eq!((diagnostic.spanStart, diagnostic.spanEnd), (1, 3));
    }

    #[test]
    fn compile_many_boundary_delegates_directly_without_linting() {
        let source = include_str!("lib.rs");
        let start = source
            .find("pub fn compile_many(")
            .expect("compileMany NAPI entry point must exist");
        let end = source[start..]
            .find("// Typed audit entry-points")
            .map(|offset| start + offset)
            .expect("compileMany must end before the typed audit entry points");
        let body = &source[start..end];

        assert_eq!(
            body.matches("self.inner.compile_many(").count(),
            1,
            "the native boundary must delegate exactly once to the host batch compiler"
        );
        for forbidden in ["Linter::", "lint_with_source(", ".lint("] {
            assert!(
                !body.contains(forbidden),
                "compileMany must never enter the lint subsystem; found `{forbidden}`"
            );
        }
    }

    /// The per-file `ssrModuleId` used to have no channel on
    /// `NapiCompileProfile` at all — honored on the batch `runtime-render`
    /// lane (`NapiCompileBatchRenderProfile.ssrModuleId`) but silently
    /// dropped for every per-file call (`get`/`getIde`/`compileWithAudit`).
    /// This is the exact `NapiCompileProfile` -> `FfiCompileProfile` hop the
    /// route-inventory named as the fix site.
    #[test]
    fn per_file_ssr_module_id_survives_the_napi_to_ffi_hop() {
        let napi_profile = NapiCompileProfile {
            ssrModuleId: Some("assets/Comp.vue".to_string()),
            ..Default::default()
        };
        let ffi_profile: FfiCompileProfile = napi_profile.into();
        assert_eq!(
            ffi_profile.ssr_module_id,
            Some("assets/Comp.vue".to_string())
        );
    }

    #[test]
    fn absent_ssr_module_id_stays_none() {
        let napi_profile = NapiCompileProfile::default();
        let ffi_profile: FfiCompileProfile = napi_profile.into();
        assert_eq!(ffi_profile.ssr_module_id, None);
    }

    /// Boundary-decode equivalence: the SAME logical compile intent
    /// expressed through NAPI's `NapiCompileProfile -> FfiCompileProfile`
    /// hop (this crate's `From` impl) and through WASM's route (a
    /// `FfiCompileProfile` decoded directly from JSON — simulated here by
    /// constructing it by hand, since `serde_wasm_bindgen`'s decode is
    /// itself a straight field-for-field `Deserialize`, not a second
    /// conversion authority) must converge on the EXACT SAME
    /// `verter_session::CompileProfile` once both reach the shared
    /// `ffi_profile_to_host` convergence point. A field the NAPI `From`
    /// impl silently dropped, or mapped to a different host field than the
    /// WASM-shaped input would, surfaces here as a `CompileProfile`
    /// inequality — not a per-field spot check.
    #[test]
    fn napi_and_wasm_boundary_decode_converge_on_the_same_compile_profile() {
        let napi_profile = NapiCompileProfile {
            filename: Some("Comp.vue".to_string()),
            isProduction: Some(true),
            customElement: Some(true),
            ssr: Some(true),
            ssrModuleId: Some("assets/Comp.vue".to_string()),
            hmrStrategy: Some("vite".to_string()),
            componentId: Some("comp-id".to_string()),
            delimiters: Some(vec!["[[".to_string(), "]]".to_string()]),
            customElements: Some(vec!["my-".to_string()]),
            comments: Some(true),
            runtimeModuleName: Some("vue".to_string()),
            typesModuleName: Some("$verter/types".to_string()),
            forceVapor: Some(true),
            forceJs: Some(true),
            sourceMap: Some(true),
            target: Some("ide".to_string()),
            inline: Some(true),
            strictSlots: Some(true),
            requestedMode: Some("content".to_string()),
        };
        // Same logical intent, expressed as the raw `FfiCompileProfile`
        // WASM's `serde_wasm_bindgen` decode would produce from the
        // equivalent JS object.
        let wasm_shaped_ffi_profile = FfiCompileProfile {
            filename: Some("Comp.vue".to_string()),
            is_production: Some(true),
            custom_element: Some(true),
            ssr: Some(true),
            ssr_module_id: Some("assets/Comp.vue".to_string()),
            hmr_strategy: Some("vite".to_string()),
            component_id: Some("comp-id".to_string()),
            delimiters: Some(vec!["[[".to_string(), "]]".to_string()]),
            custom_elements: Some(vec!["my-".to_string()]),
            comments: Some(true),
            runtime_module_name: Some("vue".to_string()),
            types_module_name: Some("$verter/types".to_string()),
            force_vapor: Some(true),
            force_js: Some(true),
            source_map: Some(true),
            target: Some("ide".to_string()),
            inline: Some(true),
            strict_slots: Some(true),
            requested_mode: Some("content".to_string()),
        };

        let via_napi: FfiCompileProfile = napi_profile.into();
        let napi_host_profile =
            ffi_profile_to_host(Some(via_napi)).expect("NAPI-shaped profile must decode");
        let wasm_host_profile = ffi_profile_to_host(Some(wasm_shaped_ffi_profile))
            .expect("WASM-shaped profile must decode");

        assert_eq!(
            napi_host_profile, wasm_host_profile,
            "NAPI and WASM boundary decode must converge on an identical CompileProfile \
             for the same logical compile intent"
        );
        // Discriminating: a regression that silently drops a field in
        // either hop would still pass a bare equality check if BOTH sides
        // drop it identically (both regress to the same default). Assert
        // representative fields explicitly reached their non-default
        // values so the test cannot pass by both sides going quietly to
        // `CompileProfile::default()`.
        assert!(napi_host_profile.ssr);
        assert!(napi_host_profile.force_vapor);
        assert_eq!(napi_host_profile.component_id.as_deref(), Some("comp-id"));
        assert_eq!(
            napi_host_profile.delimiters,
            Some(("[[".to_string(), "]]".to_string()))
        );
        assert_eq!(
            napi_host_profile.requested_mode,
            verter_session::CompileCacheMode::Content
        );
    }

    #[test]
    fn default_dependency_resolution_extensions_include_svelte_carriers_once() {
        let extensions = default_known_dependency_extensions();
        assert_eq!(
            extensions
                .iter()
                .filter(|ext| ext.as_str() == ".svelte")
                .count(),
            1
        );
    }

    /// The typed unsupported-language failure surfaces at the NAPI
    /// boundary in the SAME status family as the classify errors
    /// (`InvalidArg` — the request named a language the host cannot
    /// serve), not as a generic failure. DISCRIMINATING: the catch-all
    /// arm maps it to `GenericFailure`.
    #[test]
    fn unsupported_language_maps_to_invalid_arg_status() {
        let err = host_error(host::HostError::Scheduler(
            verter_scheduler::job::SchedulerError::UnsupportedLanguage {
                file_id: "/src/Box.svelte".to_string(),
                adapter_id: verter_session::FrameworkAdapterId::svelte(),
            },
        ));
        assert_eq!(err.status, Status::InvalidArg);
        assert!(
            err.reason.contains("svelte"),
            "the message names the adapter: {}",
            err.reason
        );
    }

    /// Throwaway instrumentation: measure NAPI async VFS future sizes.
    /// Run: cargo test -p verter_napi measure_napi_async -- --nocapture --ignored
    #[tokio::test]
    #[ignore = "throwaway instrumentation — run manually"]
    async fn measure_napi_async_future_sizes() {
        use std::mem::size_of_val;
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        eprintln!("=== napi async VFS futures profile={profile} ===");
        let ws = NapiWorkspace::new(vec!["/tmp/verter-future-size".into()]);
        {
            let fut = ws.read_file("/tmp/verter-future-size/x.ts".into());
            eprintln!(
                "[future-size] NapiWorkspace::read_file: {} B ({:.1} KiB)",
                size_of_val(&fut),
                size_of_val(&fut) as f64 / 1024.0
            );
            drop(fut);
        }
        {
            let fut = ws.file_exists("/tmp/verter-future-size/x.ts".into());
            eprintln!(
                "[future-size] NapiWorkspace::file_exists: {} B ({:.1} KiB)",
                size_of_val(&fut),
                size_of_val(&fut) as f64 / 1024.0
            );
            drop(fut);
        }
        {
            let fut = ws.write_file("/tmp/x.ts".into(), "const x = 1".into());
            eprintln!(
                "[future-size] NapiWorkspace::write_file: {} B ({:.1} KiB)",
                size_of_val(&fut),
                size_of_val(&fut) as f64 / 1024.0
            );
            drop(fut);
        }
        {
            let fut = ws.walk(
                "/tmp/verter-future-size".into(),
                vec!["node_modules".into()],
                Some(vec![".ts".into()]),
            );
            eprintln!(
                "[future-size] NapiWorkspace::walk: {} B ({:.1} KiB)",
                size_of_val(&fut),
                size_of_val(&fut) as f64 / 1024.0
            );
            drop(fut);
        }
        {
            let fut = ws.resolve_import("/tmp/a.ts".into(), "./b".into(), None, None);
            eprintln!(
                "[future-size] NapiWorkspace::resolve_import: {} B ({:.1} KiB)",
                size_of_val(&fut),
                size_of_val(&fut) as f64 / 1024.0
            );
            drop(fut);
        }
    }

    #[test]
    fn host_update_to_napi_exposes_module_references() {
        let result = host_update_to_napi(
            host::HostUpdateResult {
                module_references: vec![host::ScriptModuleReference {
                    syntax: verter_semantic::analysis::ModuleReferenceSyntax::DynamicImport,
                    semantics: verter_semantic::analysis::ModuleReferenceSemantics::Import,
                    is_type_only: false,
                    raw_text: "`./${name}.vue`".to_string(),
                    literal_specifier: None,
                    finite_specifiers: vec!["./Foo.vue".to_string()],
                    static_prefix: Some("./".to_string()),
                    analyzability:
                        verter_semantic::analysis::ModuleReferenceAnalyzability::FiniteSet,
                    span: verter_span::Span::new(4, 22),
                    expr_span: verter_span::Span::new(11, 21),
                }],
                ..host::HostUpdateResult::no_change("/test/App.vue".to_string())
            },
            Some("const x = import(`./${name}.vue`)"),
        );

        assert_eq!(result.moduleReferences.len(), 1);
        assert_eq!(result.moduleReferences[0].syntax, "dynamicImport");
        assert_eq!(result.moduleReferences[0].analyzability, "finiteSet");
        assert_eq!(result.moduleReferences[0].exprSpanStart, 11);
        assert_eq!(
            result.moduleReferences[0].finiteSpecifiers,
            vec!["./Foo.vue"]
        );
    }

    #[test]
    fn host_update_to_napi_exposes_export_signatures() {
        // Use the host to produce real export signatures from a barrel file
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let host_result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/barrel.ts".to_string()),
                input_id: "/src/barrel.ts".to_string(),
                source: std::sync::Arc::from(
                    "export { default as Button } from './Button.vue';\nexport type { Props } from './types';",
                ),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();

        assert!(
            !host_result.export_signatures.is_empty(),
            "barrel file must produce export signatures"
        );

        let result = host_update_to_napi(host_result, None);

        // Positive: re-export signatures mapped with camelCase fields
        let button = result.exportSignatures.iter().find(|s| s.name == "Button");
        assert!(button.is_some(), "Button re-export must be present");
        let button = button.unwrap();
        assert!(!button.isType);
        assert_eq!(button.reexportSource, Some("./Button.vue".to_string()));
        assert_eq!(button.reexportLocal, Some("default".to_string()));

        let props = result.exportSignatures.iter().find(|s| s.name == "Props");
        assert!(props.is_some(), "Props type re-export must be present");
        assert!(props.unwrap().isType);
    }

    #[test]
    fn host_update_to_napi_export_signatures_local_exports() {
        let h = host::VerterHost::new_standalone(host::HostConfig::default());
        let host_result = h
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/utils.ts".to_string()),
                input_id: "/src/utils.ts".to_string(),
                source: std::sync::Arc::from(
                    "export function greet() {}\nexport type Color = string;",
                ),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();

        let result = host_update_to_napi(host_result, None);

        let greet = result.exportSignatures.iter().find(|s| s.name == "greet");
        assert!(greet.is_some(), "local export must be present");
        // Negative: local exports must not have reexport fields
        assert!(greet.unwrap().reexportSource.is_none());
        assert!(greet.unwrap().reexportLocal.is_none());

        let color = result.exportSignatures.iter().find(|s| s.name == "Color");
        assert!(color.is_some(), "type export must be present");
        assert!(color.unwrap().isType);
    }

    #[test]
    fn host_update_to_napi_export_signatures_empty_on_no_change() {
        let result = host_update_to_napi(
            host::HostUpdateResult::no_change("/src/Empty.vue".to_string()),
            None,
        );
        assert!(
            result.exportSignatures.is_empty(),
            "no-change result must have empty exportSignatures"
        );
    }

    // the inline `compile_batch_files` helper smoke tests
    // were deleted along with the helper itself (host-bypassing
    // free-fn `compileBatch` is now `host.compileMany`). The
    // host-backed batch path is fully exercised by the host_compile
    // tests in verter_session and the JS-side E2E tests in
    // packages/native/index.spec.ts.

    /// A NAPI host preloaded with a Vue SFC whose props type lives in a
    /// sibling `.ts` file — the same fixture shape the verter_session
    /// public-API mode pins use, exercised here THROUGH the NAPI binding.
    fn public_api_mode_fixture_host() -> NapiVerterHost {
        let napi_host = NapiVerterHost {
            inner: std::sync::Arc::new(host::VerterHost::new_standalone(
                host::HostConfig::default(),
            )),
        };
        let _ = napi_host
            .inner
            .upsert(host::UpsertRequest {
                canonical_id: None,
                input_id: "/src/Cap.vue".to_string(),
                source: std::sync::Arc::from(
                    "<script setup lang=\"ts\">\nimport type { CapProps } from './cap-types';\nconst count = 1;\ndefineProps<CapProps>();\n</script>\n<template><div>{{ count }}</div></template>",
                ),
                file_language: host::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("upsert Cap.vue");
        let _ = napi_host
            .inner
            .upsert(host::UpsertRequest {
                canonical_id: None,
                input_id: "/src/cap-types.ts".to_string(),
                source: std::sync::Arc::from(
                    "export interface CapProps { label: string; n: number }\n",
                ),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert cap-types.ts");
        napi_host
    }

    /// The `getPublicApi` NAPI binding accepts `"declaration"` and routes
    /// it to `PublicApiMode::Declaration` — the declaration-only
    /// `.d.<ext>.ts` surface — while `"public"` keeps the runtime-instance
    /// surface. DISCRIMINATING: the pre-change allow-list rejected
    /// `"declaration"` at the binding boundary (InvalidArg), so this test
    /// fails RED on the old binding even though the host already serves
    /// `PublicApiMode::Declaration`.
    #[test]
    fn get_public_api_declaration_mode_is_accepted_and_distinct_from_public() {
        let napi_host = public_api_mode_fixture_host();

        let decl = napi_host
            .get_public_api("/src/Cap.vue".to_string(), Some("declaration".to_string()))
            .expect("the NAPI binding must accept mode 'declaration'")
            .value
            .expect("declaration-mode output for a Vue SFC");
        let public = napi_host
            .get_public_api("/src/Cap.vue".to_string(), Some("public".to_string()))
            .expect("mode 'public' stays accepted")
            .value
            .expect("public-mode output for a Vue SFC");

        // Declaration-specific shape: a valid `.d.ts` — declares the
        // component value, carries NO runtime/value code.
        assert!(
            decl.code.contains("declare const Cap"),
            "declaration output declares the component value, got:\n{}",
            decl.code
        );
        assert!(
            decl.code.contains("export default Cap"),
            "declaration output default-exports the component, got:\n{}",
            decl.code
        );
        assert!(
            !decl.code.contains("const __comp"),
            "declaration output must not carry the runtime __comp const, got:\n{}",
            decl.code
        );
        assert!(
            !decl.code.contains("defineComponent("),
            "declaration output must not call defineComponent, got:\n{}",
            decl.code
        );
        // Control: the public surface DOES carry the runtime const, so the
        // negative assertions above discriminate mode routing (a binding
        // that silently served Public for "declaration" fails here).
        assert!(
            public.code.contains("const __comp = defineComponent"),
            "public output keeps the runtime __comp const (control), got:\n{}",
            public.code
        );
        assert_ne!(
            decl.code, public.code,
            "declaration-mode output must differ from public-mode output"
        );
    }

    /// Absent mode stays the Public surface (backward-compatible with the
    /// existing modeless callers) and an unknown mode string is still a
    /// typed `InvalidArg` rejection — never a silent default.
    #[test]
    fn get_public_api_mode_defaults_to_public_and_rejects_unknown() {
        let napi_host = public_api_mode_fixture_host();

        let absent = napi_host
            .get_public_api("/src/Cap.vue".to_string(), None)
            .expect("absent mode stays accepted")
            .value
            .expect("default-mode output for a Vue SFC");
        let public = napi_host
            .get_public_api("/src/Cap.vue".to_string(), Some("public".to_string()))
            .expect("mode 'public' stays accepted")
            .value
            .expect("public-mode output for a Vue SFC");
        assert_eq!(
            absent.code, public.code,
            "absent mode must serve the Public surface (backward-compatible)"
        );

        let err =
            match napi_host.get_public_api("/src/Cap.vue".to_string(), Some("bogus".to_string())) {
                Err(e) => e,
                Ok(_) => panic!("an unknown mode must be rejected, not silently defaulted"),
            };
        assert_eq!(
            err.status,
            Status::InvalidArg,
            "unknown mode maps to InvalidArg, got {:?}: {}",
            err.status,
            err.reason
        );
        assert!(
            err.reason.contains("bogus"),
            "the rejection names the offending mode string: {}",
            err.reason
        );
        assert!(
            err.reason.contains("declaration"),
            "the rejection lists 'declaration' among the accepted modes: {}",
            err.reason
        );
    }

    #[test]
    fn get_public_api_binding_preserves_unsafe_enum_error_and_absence_controls() {
        let napi_host = NapiVerterHost {
            inner: std::sync::Arc::new(host::VerterHost::new_standalone(
                host::HostConfig::default(),
            )),
        };
        let _unsafe_update = napi_host
            .inner
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/UnsafeEnum.vue".to_string()),
                input_id: "/src/UnsafeEnum.vue".to_string(),
                source: std::sync::Arc::from(
                    r#"<script setup lang="ts">
enum Unsafe { Value = Math.random() }
defineProps<{ value: Unsafe }>()
</script>"#,
                ),
                file_language: host::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("upsert unsafe enum");
        let _plain_update = napi_host
            .inner
            .upsert(host::UpsertRequest {
                canonical_id: Some("/src/plain.ts".to_string()),
                input_id: "/src/plain.ts".to_string(),
                source: std::sync::Arc::from("export const value = 1"),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert non-carrier control");

        let failure = napi_host
            .get_public_api(
                "/src/UnsafeEnum.vue".to_string(),
                Some("declaration".to_string()),
            )
            .expect("projection failure uses the result error rail");
        assert!(failure.value.is_none());
        let error = failure.error.expect("structured projection error");
        assert_eq!(error.code, "tsc-generation");
        assert_eq!(error.detailCode, "unsupported-declaration-shape");
        assert!(matches!(
            error.subject,
            Either::A(NapiTscMacroFailureSubject { syntaxIndex: 0, .. })
        ));
        assert_eq!(
            error.declarationShapeReason.as_deref(),
            Some("unsupported-enum-shape")
        );
        assert_eq!(error.memberOrdinal, None);
        assert_eq!(error.outcomeKind, None);
        assert_eq!(error.outcomeReason, None);
        assert_eq!(error.outcomeDiagnostic, None);

        for canonical in ["/src/Missing.vue", "/src/plain.ts"] {
            let absent = napi_host
                .get_public_api(canonical.to_string(), Some("declaration".to_string()))
                .expect("ordinary absence is a successful binding result");
            assert!(absent.value.is_none(), "{canonical}: value must be null");
            assert!(absent.error.is_none(), "{canonical}: error must be null");
        }
    }

    #[test]
    fn public_api_binding_preserves_all_unavailable_outcome_arms() {
        let cases = [
            ("partial", "incomplete-traversal", "partial detail"),
            ("unresolved", "ambiguous-reference", "unresolved detail"),
            ("unsupported", "semantic-construct", "unsupported detail"),
            ("invalid", "non-object-root", "invalid detail"),
        ];

        for (syntax_index, (kind, reason, diagnostic)) in cases.into_iter().enumerate() {
            let error: NapiPublicApiProjectionError = FfiPublicApiProjectionError {
                code: "tsc-generation".to_string(),
                detail_code: "unavailable-outcome".to_string(),
                subject: PublicApiProjectionSubject::Macro {
                    syntax_index: syntax_index as u32,
                },
                declaration_shape_reason: None,
                member_ordinal: None,
                outcome_kind: Some(kind.to_string()),
                outcome_reason: Some(reason.to_string()),
                outcome_diagnostic: Some(diagnostic.to_string()),
            }
            .into();

            assert_eq!(error.code, "tsc-generation");
            assert_eq!(error.detailCode, "unavailable-outcome");
            assert!(matches!(
                error.subject,
                Either::A(NapiTscMacroFailureSubject {
                    syntaxIndex,
                    ..
                }) if syntaxIndex == syntax_index as u32
            ));
            assert_eq!(error.declarationShapeReason, None);
            assert_eq!(error.memberOrdinal, None);
            assert_eq!(error.outcomeKind.as_deref(), Some(kind));
            assert_eq!(error.outcomeReason.as_deref(), Some(reason));
            assert_eq!(error.outcomeDiagnostic.as_deref(), Some(diagnostic));
        }

        let attrs_error: NapiPublicApiProjectionError = FfiPublicApiProjectionError {
            code: "tsc-generation".to_string(),
            detail_code: "unavailable-outcome".to_string(),
            subject: PublicApiProjectionSubject::ScriptSetupAttrs {
                source_range: verter_span::Span::new(31, 37),
            },
            declaration_shape_reason: None,
            member_ordinal: None,
            outcome_kind: Some("invalid".to_string()),
            outcome_reason: Some("malformed-or-recovered-type-syntax".to_string()),
            outcome_diagnostic: None,
        }
        .into();
        assert!(matches!(
            attrs_error.subject,
            Either::B(NapiTscScriptSetupAttrsFailureSubject {
                sourceRange: NapiSourceRange { start: 31, end: 37 },
                ..
            })
        ));
    }

    /// Route-level proof: a runtime-object macro's real member presence
    /// must publish through the SOLE audited wire entry for
    /// `GRAPH_OPERATION_FRAMEWORK_SURFACES`
    /// (`resolve_framework_surface_with_audit`, `CLAUDE.md` → "Framework
    /// Adapter Substrate"), exercised through the FULL encode → NAPI call →
    /// decode round-trip a real JS caller drives — not just the internal
    /// `resolve_vue_macro_surface`/direct-host-call route.
    ///
    /// `verter_lsp` has no framework-surface / typeinfo-graph request
    /// handler at all today (confirmed by inspection — no reference to
    /// `resolve_framework_surface_with_audit`, `FrameworkSurfacePayload`, or
    /// `GRAPH_OPERATION_FRAMEWORK_SURFACES` anywhere under
    /// `crates/verter_lsp/src/`), so there is no SEPARATE LSP route to prove
    /// this through without inventing a new LSP wire surface. This crate
    /// (`verter_napi`) is the audited entry's actual currently-wired
    /// production consumer (alongside `verter_wasm`), so the wire
    /// round-trip proof lives here instead, for BOTH runtime-object macro
    /// forms below.
    fn runtime_object_members_via_napi_wire_round_trip(
        canonical_id: &str,
        source: &str,
        kind: verter_protocol::typeinfo::graph::FrameworkSurfaceKind,
    ) -> Vec<String> {
        use prost::Message;
        use verter_protocol::typeinfo::graph as wire;
        use verter_protocol::verter::v1::{
            type_info_graph_request as wire_request, type_info_graph_response,
        };

        let napi_host = NapiVerterHost {
            inner: std::sync::Arc::new(host::VerterHost::new_standalone(
                host::HostConfig::default(),
            )),
        };
        let _ = napi_host
            .inner
            .upsert(host::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: std::sync::Arc::from(source),
                file_language: host::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("Vue fixture must upsert");

        let envelope = wire::TypeInfoGraphRequest {
            schema_version: 3,
            operation: wire::Operation::FrameworkSurfaces as i32,
            payload: Some(wire_request::Payload::FrameworkSurface(
                wire::FrameworkSurfaceRequest {
                    selector: Some(wire::ComponentSelector {
                        canonical_id: canonical_id.to_string(),
                        export_name: String::new(),
                        has_export_name: false,
                        framework_adapter_id: "vue".to_string(),
                    }),
                    context: Some(wire::ProjectionReductionContext {
                        mode: wire::ProjectionMode::Expanded as i32,
                        demand: wire::ReductionDemand::Published as i32,
                    }),
                    closure: Some(wire::ClosurePolicy {
                        kind: Some(
                            verter_protocol::verter::v1::graph_closure_policy::Kind::OneLevel(
                                wire::ClosureOneLevel {},
                            ),
                        ),
                    }),
                    display_policy: Some(wire::DisplayPolicy {
                        qualification: wire::DisplayQualification::Qualified as i32,
                        branding: wire::DisplayBranding::On as i32,
                        budgets: Some(wire::DisplayBudgets {
                            max_string_length: 4096,
                            max_depth: 16,
                        }),
                    }),
                    include_provenance: false,
                    include_diagnostics: false,
                    include_projection: vec![],
                    schema_version: 3,
                },
            )),
        };
        let request_buf = Buffer::from(envelope.encode_to_vec());

        let result = napi_host
            .resolve_framework_surface_with_audit(request_buf)
            .expect("the NAPI wire adapter must decode/dispatch/encode successfully");

        let response = wire::TypeInfoGraphResponse::decode(result.response.as_ref())
            .expect("response must be a valid protobuf TypeInfoGraphResponse");
        let payload = match &response.kind {
            Some(type_info_graph_response::Kind::FrameworkSurface(payload)) => payload,
            other => panic!("expected a framework_surface response arm, got: {other:?}"),
        };
        let strings: Vec<String> = payload
            .graph
            .as_ref()
            .and_then(|graph| graph.strings.as_ref())
            .map(|table| table.entries.clone())
            .unwrap_or_default();
        let mut names: Vec<String> = payload
            .surfaces
            .iter()
            .find(|surface| surface.kind == kind as i32)
            .map(|surface| {
                surface
                    .members
                    .iter()
                    .filter_map(|member| strings.get(member.name_id as usize).cloned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn resolve_framework_surface_with_audit_publishes_real_runtime_object_expose_members() {
        let names = runtime_object_members_via_napi_wire_round_trip(
            "/w/RuntimeExposeNapi.vue",
            "<script setup lang=\"ts\">\n\
             import { ref } from 'vue'\n\
             const count = ref(0)\n\
             function bump() { count.value++ }\n\
             defineExpose({ count, bump })\n\
             </script>\n",
            verter_protocol::typeinfo::graph::FrameworkSurfaceKind::Expose,
        );

        assert_eq!(
            names,
            vec!["bump".to_string(), "count".to_string()],
            "runtime-object defineExpose members must publish through the NAPI wire round-trip, got: {names:?}"
        );
    }

    #[test]
    fn resolve_framework_surface_with_audit_publishes_real_runtime_object_props_members() {
        let names = runtime_object_members_via_napi_wire_round_trip(
            "/w/RuntimePropsNapi.vue",
            "<script setup lang=\"ts\">\n\
             defineProps({ title: String, count: Number })\n\
             </script>\n",
            verter_protocol::typeinfo::graph::FrameworkSurfaceKind::Props,
        );

        assert_eq!(
            names,
            vec!["count".to_string(), "title".to_string()],
            "runtime-object defineProps members must publish through the NAPI wire round-trip, got: {names:?}"
        );
    }
}
