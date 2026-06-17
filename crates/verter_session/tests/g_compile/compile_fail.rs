//! Compile-fail driver.
//!
//! Cargo's normal `[[test]]` target cannot express "this test passes when
//! it fails to compile" — a normal integration test that fails to compile
//! breaks the entire test build. The mechanical fix is the `trybuild`
//! crate (standard Rust idiom for compile-fail assertions).
//!
//! Verifies that `VerterHost::workspace()` is gated behind `pub(crate)`
//! and is NOT callable from external compilation units. The fixture
//! `tests/compile-fail/workspace_accessor_visibility.rs` contains code
//! that calls `verter_session::VerterHost::workspace(&host)` from
//! outside the crate; trybuild captures the visibility error.
//!
//! This is a regression test — it asserts a constraint that holds in
//! the final state, not a pre/post discrimination.

// trybuild spawns a full `cargo build` of the fixture crate (linking
// `verter_session`), which dominates this test's ~100s runtime. It is gated
// out of the default inner-loop run and runs in CI via
// `cargo nextest run -p verter_session --features compile-fail`. The
// visibility constraint is still enforced on every CI push — only local
// `cargo nextest run` skips it.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn workspace_accessor_visibility() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/workspace_accessor_visibility.rs");
}

/// API-surface half of `carrier_access_token_minted_only_in_verter_language`:
/// an out-of-crate `CarrierAccessToken` struct literal must fail
/// to compile — the `_private: ()` field is the in-language forging
/// barrier; the static guard is the cross-crate enforcement authority.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn carrier_access_token_not_constructible_outside_verter_language() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/carrier_access_token_struct_literal.rs");
}
