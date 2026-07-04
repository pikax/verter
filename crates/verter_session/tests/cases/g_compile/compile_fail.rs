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
// `verter_session`), which dominates this test's ~100s runtime. Every fixture
// here is `#[ignore]`d unless the `compile-fail` feature is on, and that
// feature is NOT wired into the default gate (`node scripts/gate.mjs`) or any
// CI workflow — run it LOCALLY with
// `cargo nextest run -p verter_session --features compile-fail`. The
// underlying constraint is also enforced structurally by the normal build
// (a private/`pub(crate)` accessor called from an external unit fails the
// ordinary compile), so this fixture is a belt-and-braces assertion, not the
// sole rail.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
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
    ignore = "run with --features compile-fail"
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
    ignore = "run with --features compile-fail"
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
    ignore = "run with --features compile-fail"
)]
fn no_storedspan_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/no_storedspan_direct_span_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_storedspan_option_span_field.rs");
    t.compile_fail("tests/cases/compile-fail/no_storedspan_nested_owner_field.rs");
    t.pass("tests/cases/compile-fail/no_storedspan_good_carrier.rs");
}

/// The recursive-self derive escape (`#[no_typeexpr(recursive_self)]` /
/// `#[no_storedspan(recursive_self)]`) proves the approved `Arc<[Self]>` self
/// field with a COMPILER-RESOLVED `RecursiveSelfArc<Self>` proof-bound (only the
/// genuine `std::sync::Arc<[Self]>` satisfies it) instead of a syntactic omit.
/// The syntactic matcher only PICKS candidate fields; the proof-bound verifies
/// them:
///
/// - A carrier using the escape that ALSO grows a NEW non-recursive arm owning a
///   `TypeExpr` / `Span` still FAILS the derive — the future-arm proof for the
///   `ClosednessRecipe` fixpoint.
/// - `Arc<[some_mod::Recipe]>` — a DIFFERENT type sharing the container's
///   LAST-segment name — keeps its bound (the last-segment-match false-witness
///   red-proof).
/// - A MULTI-segment `shadow::Arc<[Self]>` wrapper keeps its witness bound (the
///   custom-wrapper false-witness red-proof — rejected by the syntactic matcher).
/// - A BARE re-imported `Arc<[Self]>` (`use shadow::Arc;`) that owns a `TypeExpr`
///   / `Span` — the field the syntactic matcher ACCEPTS but the proof-bound
///   REJECTS — fails the derive. This is the bare-shadow hole the proof-bound
///   closes: under the old syntactic-omit escape it COMPILED (a false witness).
///
/// The pass fixture proves the escape does not over-reject a sound recursive
/// carrier.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn recursive_self_derive_escape_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/recursive_self_rejects_typeexpr_arm.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_rejects_span_arm.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_foreign_slice_element.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_shadowed_arc_wrapper.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_bare_shadow_arc_typeexpr.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_bare_shadow_arc_span.rs");
    t.pass("tests/cases/compile-fail/recursive_self_good_recursive_carrier.rs");
}

/// The `RecursiveSelfArc<Owner>` proof-trait is SEALED: it is impl'd ONLY inside
/// the marker crate (the derive merely EMITS it as a bound, never impls it
/// downstream), so it carries a PRIVATE `recursive_self_sealed::Sealed` supertrait
/// that is unreachable downstream. A hostile downstream/shadow crate therefore
/// CANNOT hand-write `impl RecursiveSelfArc<Recipe> for shadow::Arc<[Recipe]>` to
/// FORGE the proof-bound and re-open the bare-shadow hole — the private `Sealed`
/// supertrait is unsatisfiable and unnameable, so the hand-impl fails (E0277).
/// DISCRIMINATING: the same hand-impl COMPILES against the unsealed trait and
/// FAILS against the sealed one; both markers are covered.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn recursive_self_proof_trait_is_sealed_against_hand_impl() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/recursive_self_hand_impl_sealed_typeexpr.rs");
    t.compile_fail("tests/cases/compile-fail/recursive_self_hand_impl_sealed_span.rs");
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
    ignore = "run with --features compile-fail"
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
    ignore = "run with --features compile-fail"
)]
#[test]
fn output_projector_non_owner_impl_is_compiler_sealed() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/output_projector_not_impl_outside_crate.rs");
}

/// The sealed-context negative for the locator lowering path:
/// `LocatorShapeCtx` neither contains nor converts to a
/// `ProjectionReductionContext` (no `From`/`Into`, no `AsRef`, no `Deref` —
/// each bound fails E0277), and the reducing lowering entry
/// (`shallow_lower_type_expr_with_context`, which REQUIRES a
/// `ProjectionReductionContext`) lives on a crate-private dispatch module an
/// external unit cannot even name (E0603). Together they prove BY TYPE that
/// the locator path cannot reach the reducing lowerer — capability, not
/// convention.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn locator_shape_ctx_is_sealed_against_the_reducing_lowerer() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/locator_shape_ctx_no_prc_conversion.rs");
    t.compile_fail("tests/cases/compile-fail/locator_reducing_lowerer_not_nameable.rs");
}

/// The role-free shape-node payload witness: the member role stamps
/// (`MacroOwnBodyStamp` / `MergeRoleStamp`) carried by `SurfaceMember` have
/// PRIVATE inner fields, so a non-neutral stamp cannot be minted from raw
/// values (E0423) and the payload cannot be reached directly (E0616) — the
/// only producers are the neutral consts and the
/// `ProjectionReductionContext` / analyzed-macro-kind witness methods.
/// Combined with the sealed-context fixtures above (the locator lowering
/// entry accepts only `LocatorShapeCtx`, which neither contains nor
/// converts to a reduction context), this proves BY TYPE that cached
/// `LowerLocator` shape-node identity cannot carry a caller-relative role
/// stamp.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn member_role_stamps_are_not_mintable_without_a_witness() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/member_role_stamps_not_mintable_without_witness.rs");
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

/// The sealed `InstantiateBodySource` construction: the source-kind
/// constructors `InstantiateContext::file_backed` / `::non_file` are
/// `pub(crate)` AND require the `BodySourceWitness` mintable only inside
/// the dispatch module, so an out-of-factory production construction of a
/// source kind fails to compile — the
/// `ProjectSemanticDispatch::instantiate_context_for` choke point stays
/// the SOLE production builder and owns the deterministic
/// non-file/file-backed mapping. If a constructor were widened back to
/// `pub` (witness-less), the fixture would COMPILE and trybuild would
/// fail this test.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn instantiate_body_source_is_sealed_to_the_dispatch_factory() {
    let t = trybuild::TestCases::new();
    t.compile_fail(
        "tests/cases/compile-fail/instantiate_body_source_sealed_to_dispatch_factory.rs",
    );
}

/// The KEY-shape seal (not just the ctor): `SemanticQueryKey::Instantiate`
/// carries the opaque `InstantiateKey` (private fields) whose
/// `InstantiateContext` embeds the `pub(crate)` `InstantiateBodySource` axis,
/// so a `NonFile`-context-on-a-real-file-base transplant is UNREPRESENTABLE
/// from outside the crate. Two fail-closed fixtures:
///
/// 1. `instantiate_key_seal_no_features` (DEV profile, no features): an
///    external consumer cannot call the DELETED `*_for_tests` mints, name the
///    `pub(crate)` `InstantiateBodySource::NonFile`, struct-literal
///    `InstantiateContext` / `InstantiateKey`, or use struct-variant syntax on
///    the `Instantiate` tuple variant.
/// 2. `instantiate_key_context_not_extractable`: even a consumer that OBTAINED
///    a key through the `test-support` `instantiate_key_for_tests` helper
///    cannot extract or transplant the raw `InstantiateContext` — the reveal
///    accessors are `pub(crate)` and the payload fields private.
///
/// If any seal regressed (a ctor widened, the axis enum widened, the fields
/// exposed, the variant reverted to a struct variant, or the reveal accessors
/// widened), the corresponding line would COMPILE and trybuild would fail.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn instantiate_key_shape_is_sealed_against_forgery_and_extraction() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/instantiate_key_seal_no_features.rs");
    t.compile_fail("tests/cases/compile-fail/instantiate_key_context_not_extractable.rs");
}
