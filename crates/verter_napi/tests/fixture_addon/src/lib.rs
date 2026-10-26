//! The native host compile request, reachable from real JS.
//!
//! The request's `FromNapiValue` is what the repair changed, and its
//! input is a live V8 object graph: a property STATED as `undefined`, a
//! key that lives on the prototype rather than the object, and an array
//! whose length is declared rather than held are all distinctions a Rust
//! fixture can only model. This addon puts the real thing in front of
//! them.
//!
//! Its main entry decodes a JS request and converts it, returning a
//! deterministic rendering of the result. A caller therefore observes both
//! halves: which payloads are refused, and that two accepted payloads
//! decoded to the same request.
//!
//! The `fixture_*` entries report the image's allocator, which it inherits
//! from `verter_napi` exactly as the shipped addon installs it, for
//! `addon_allocator_coexistence`: whether this image's own mimalloc copy
//! serves its Rust allocations, the purge delay the load-time configuration
//! set on that copy, and the addresses that show two loaded images are two
//! copies (a static of the image, and a block its allocator holds, which only
//! that image's mimalloc owns).
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

/// Whether this image's own mimalloc copy served a fresh Rust allocation.
#[napi]
pub fn fixture_mimalloc_serves_this_image() -> bool {
    verter_alloc::mimalloc_serves_global_allocations()
}

/// The purge delay this image's mimalloc copy runs with, or `null`
/// without mimalloc.
#[napi]
pub fn fixture_mimalloc_purge_delay_ms() -> Option<i64> {
    verter_alloc::purge_delay_ms()
}

/// The address of a static of this image, in decimal. Two loaded images
/// report different addresses.
#[napi]
pub fn fixture_static_address() -> String {
    static MARKER: u8 = 0;
    (std::ptr::from_ref(&MARKER) as usize).to_string()
}

/// The address, in decimal, of a block this image allocated through its
/// global allocator and keeps for the process's lifetime.
#[napi]
pub fn fixture_held_block_address() -> String {
    static HELD: std::sync::OnceLock<Box<u64>> = std::sync::OnceLock::new();
    let block = HELD.get_or_init(|| Box::new(0));
    (std::ptr::from_ref::<u64>(block) as usize).to_string()
}

/// Whether this image's mimalloc owns `address` (decimal). Compares the
/// address only; never dereferences it.
#[napi]
pub fn fixture_mimalloc_owns(address: String) -> bool {
    address
        .parse::<usize>()
        .is_ok_and(|address| verter_alloc::mimalloc_owns(std::ptr::without_provenance(address)))
}
