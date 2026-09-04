//! The native host compile request, reachable from real JS.
//!
//! The request's `FromNapiValue` is what the repair changed, and its
//! input is a live V8 object graph: a property STATED as `undefined`, a
//! key that lives on the prototype rather than the object, and an array
//! whose length is declared rather than held are all distinctions a Rust
//! fixture can only model. This addon puts the real thing in front of
//! them.
//!
//! It exposes one entry, which decodes a JS request and converts it,
//! returning a deterministic rendering of the result. A caller therefore
//! observes both halves: which payloads are refused, and that two
//! accepted payloads decoded to the same request.
//!
//! ## Why `addon.VerterHost` works without being named here
//!
//! `real_js_host_request_boundary`'s driver also constructs
//! `addon.VerterHost` and calls `compileRequest`/`compileRequests` on it —
//! the real callable routes under test, not this crate's own
//! [`decode_host_compile_request`]. Nothing in this crate names
//! `NapiVerterHost`. That class still reaches JS because this package's
//! `crate-type = ["cdylib", "rlib"]` links `verter_napi` as a normal Rust
//! dependency into a `cdylib`: `rustc` statically links a `cdylib`'s whole
//! Rust dependency closure, not just the symbols this crate's own code
//! references, so `verter_napi`'s `#[napi]` class registrations for
//! `VerterHost` survive into this addon's `.node` unreferenced. The driver
//! asserts `typeof addon.VerterHost === "function"` before relying on it,
//! so a future build/link configuration that stops preserving this
//! (aggressive `--gc-sections`/LTO/strip, for example) fails loudly by
//! name instead of producing a confusing "not a constructor" error deep
//! into the suite.

use napi_derive::napi;

use verter_napi::{napi_host_compile_request_to_ffi, NapiHostCompileRequest};

/// Decodes `request` and renders the converted FFI request.
///
/// The rendering is `Debug`, which is exhaustive over the converted
/// value: two payloads render identically exactly when they decoded to
/// the same request. A refusal propagates as the JS exception the
/// decoder produced, message intact.
#[napi]
pub fn decode_host_compile_request(request: NapiHostCompileRequest) -> String {
    format!("{:?}", napi_host_compile_request_to_ffi(request))
}
