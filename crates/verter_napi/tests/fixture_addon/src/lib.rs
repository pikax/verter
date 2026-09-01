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
