//! The prohibited workspace resolver surface must remain absent.
//!
//! This harness lives in `verter_workspace`, so the generated crate can name
//! the owning crate and compiler diagnostics must fail at the prohibited path
//! tail. The checked stderr makes a crate-segment failure or a restored legacy
//! item a mismatch rather than an accepted compile failure.

#[test]
fn legacy_resolver_surface_is_absent() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/legacy_resolver_surface_is_absent.rs");
}
