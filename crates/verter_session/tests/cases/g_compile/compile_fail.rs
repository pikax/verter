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
    t.compile_fail("tests/cases/compile-fail/workspace_accessor_visibility.rs");
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
    t.compile_fail("tests/cases/compile-fail/carrier_access_token_struct_literal.rs");
}

/// The compiler enforcement of the no-transitive-`TypeExpr` hot-carrier
/// invariant. A field that owns a `verter_type_expr::TypeExpr` — directly, via
/// an aliased `use TypeExpr as Body`, or through a nested owner like `ValueRef`
/// — makes `#[derive(NoTypeExpr)]` fail to compile. The aliased fixture is THE
/// RED-PROOF: the exact launder the deleted source-spelling scanner could not
/// catch. The good fixture proves the marker does not over-reject a sound
/// handle-native carrier.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn no_typeexpr_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/no_typeexpr_direct_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_typeexpr_aliased_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_typeexpr_nested_owner_field.rs");
    t.pass("tests/cases/compile-fail/no_typeexpr_good_carrier.rs");
}

/// Out-of-crate half of the output-materialization fence: the sealed
/// `OutputProjector` capability is `pub(crate)` (and sealed against its private
/// `sealed::Sealed` supertrait), so an external crate cannot NAME the trait to
/// write an `impl OutputProjector for X`. The `pub(crate)` visibility error IS
/// the compile-fail — the public-API boundary documentation for the fence. The
/// in-crate / owner-descendant cases are pinned by the crate-wide
/// `assert_not_impl_any!` assertions in
/// `src/project_semantic_dispatch/output_materialization_guards.rs` and the
/// `output_projector_owner_registration_inventory` closed-leaf check.
// `#[test]` is placed as the attribute immediately above `fn` (after the
// `#[cfg_attr]` gate) so the R6 registry validity scanner's backward adjacency
// walk reaches the test attribute without stopping on a multi-line
// `#[cfg_attr(...)]` continuation line.
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
#[test]
fn output_projector_non_owner_impl_is_compiler_sealed() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/output_projector_not_impl_outside_crate.rs");
}
