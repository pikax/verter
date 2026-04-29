//! Phase 6b sub-plan §6b.D2b — T11 (REGRESSION) compile-fail driver.
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
//! Classified REGRESSION (matches T6/T12/T13) — the test asserts a
//! constraint that holds at the destination commit, not a pre/post
//! discrimination, so the red-then-green-within-commit invariant
//! doesn't apply.

#[test]
fn workspace_accessor_visibility() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/workspace_accessor_visibility.rs");
}
