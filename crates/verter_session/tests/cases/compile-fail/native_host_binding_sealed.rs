//! The request-scoped native host binding is sealed: not clonable or
//! copyable, not serializable or deserializable, and the opaque
//! framework-specific host binding inside a variant payload is not
//! reachable from outside the crate — consumption is the single by-value
//! seam only. Every line below must fail to compile; if any seal
//! regressed (a derive added, a field widened), the line would compile
//! and trybuild would fail the driving test.

use verter_session::BoundNativeHostRequest;

fn requires_clone<T: Clone>() {}
fn requires_copy<T: Copy>() {}
fn requires_serialize<T: serde::Serialize>() {}
fn requires_deserialize<T: for<'de> serde::Deserialize<'de>>() {}

fn seal(binding: BoundNativeHostRequest) {
    requires_clone::<BoundNativeHostRequest>();
    requires_copy::<BoundNativeHostRequest>();
    requires_serialize::<BoundNativeHostRequest>();
    requires_deserialize::<BoundNativeHostRequest>();
    if let BoundNativeHostRequest::Vue(vue) = binding {
        // The host binding is a private field: never fetchable as a service.
        let _ = vue.backend;
    }
}

fn main() {}
