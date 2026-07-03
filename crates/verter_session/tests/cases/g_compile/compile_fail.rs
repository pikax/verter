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

/// The compiler enforcement of the no-stored-`Span` fact invariant — the
/// deliberate inverse of [`no_typeexpr_compile_fail`]. A field that owns a
/// `verter_span::Span` — directly, wrapped in an `Option`, or through a nested
/// owner like `verter_type_expr::MemberSpans` — makes `#[derive(NoStoredSpan)]`
/// fail to compile, because the marker provides no `Span` leaf witness. The
/// direct-span fixture is THE proof that `NoStoredSpan` catches what
/// `NoTypeExpr` cannot: the same `Span` field passes `NoTypeExpr` and fails
/// `NoStoredSpan`. The good fixture proves the marker does not over-reject a
/// sound content-free fact carrier.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn no_storedspan_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/no_storedspan_direct_span_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_storedspan_option_span_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_storedspan_nested_owner_field.rs");
    t.pass("tests/cases/compile-fail/no_storedspan_good_carrier.rs");
}

/// The recursive-self derive escape (`#[no_typeexpr(recursive_self)]` /
/// `#[no_storedspan(recursive_self)]`) omits ONLY the `Arc<[Self]>` self-bound.
/// A carrier using the escape that ALSO grows a NEW non-recursive arm owning a
/// `TypeExpr` / `Span` still FAILS the derive — the future-arm proof for the
/// `ClosednessRecipe` fixpoint (the escape closes the recursive arm without
/// opening a hole for a forbidden payload). The pass fixture proves the escape
/// does not over-reject a sound recursive carrier.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn recursive_self_derive_escape_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/recursive_self_rejects_typeexpr_arm.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_rejects_span_arm.rs");
    t.pass("tests/cases/compile-fail/recursive_self_good_recursive_carrier.rs");
}

/// The remaining substrate structural negatives: (a) a closed-fact carrier with
/// a `TypeExpr` field fails `NoTypeExpr`; (c) an R6-forbidden dimension
/// (`SemanticNodeId`) cannot satisfy the sealed `R6KeyDimension` bound, so it
/// cannot be a session key dimension; (d) a `#[derive(Hash)]` DTO naming the
/// session hot handle `HotTypeRef` fails (the handle deliberately is not `Hash`).
/// The sealed trait + the not-`Hash` handle + the marker derive are the landed
/// enforcement; these fixtures are the discriminating supplement.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail (CI)"
)]
fn fact_and_key_substrate_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/fact_carrier_with_typeexpr_field.rs");
    t.compile_fail("tests/cases/compile-fail/r6_key_forbidden_dimension.rs");
    // A forbidden dimension NESTED inside a composite key position (a container)
    // is also rejected — the `R6KeySafe` witness forwards through containers.
    t.compile_fail("tests/cases/compile-fail/r6_key_nested_forbidden_dimension.rs");
    t.compile_fail("tests/cases/compile-fail/dto_names_session_hot_handle.rs");
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

/// ALWAYS-ON structural-rail smoke — the load-bearing proof that the
/// hot-materialize STRUCTURAL rails reject their regression shapes in the
/// DEFAULT gate (no feature flag; the full compile-fail suite above stays
/// feature-gated as-is). Scoped to exactly the hot-materialize regressions,
/// reusing the existing fixtures:
///
/// 1. a carrier owning a DIRECT `TypeExpr` field cannot derive `NoTypeExpr`;
/// 2. a carrier owning an ALIASED `TypeExpr` field (type-alias laundering)
///    cannot derive `NoTypeExpr`;
/// 3. an out-of-crate `impl OutputProjector` fails the seal (the trait is not
///    even nameable outside `verter_session`).
///
/// The compile-fail fixture IS the discrimination: if a rail went hollow (the
/// derive stopped recursing into fields, the alias resolution broke, the seal
/// visibility widened), the fixture would COMPILE and trybuild would fail this
/// test. Together with the in-memory scanner revert-probe
/// (`hot_materialize_scanner_flags_in_memory_injected_offender`) this pins both
/// hot-materialize rails as live in every default-gate run.
#[test]
fn hot_materialize_structural_rails_smoke() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/no_typeexpr_direct_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_typeexpr_aliased_field.rs");
    t.compile_fail("tests/cases/compile-fail/output_projector_not_impl_outside_crate.rs");
}
