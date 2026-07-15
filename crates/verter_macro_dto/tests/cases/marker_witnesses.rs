//! Structural marker witnesses for the neutral macro-codegen vocabulary.
//!
//! POSITIVE: every public aggregate the crate exports satisfies BOTH
//! compiler-enforced markers — `NoTypeExpr` (owns no transitive symbolic
//! `verter_type_expr::TypeExpr`) and `NoStoredSpan` (owns no transitive
//! `verter_span::Span`). The witness fn below has the marker bounds, so a
//! DTO that loses a derive — or grows a field whose type is not a witness —
//! fails to COMPILE here, not merely at runtime.
//!
//! NEGATIVE: the bounds genuinely discriminate. `verter_span::Span` IS
//! `NoTypeExpr` but is NOT `NoStoredSpan` (the deliberate leaf omission that
//! makes the span marker mean something), `verter_type_expr::TypeExpr`
//! satisfies NEITHER marker, and the container impls FORWARD the failure —
//! so a DTO field storing either, at any nesting depth, would fail its
//! derive. Both facts are asserted with `static_assertions` against the
//! REAL foreign types (dev-dependencies only; the production closure stays
//! marker-crates-only).

use static_assertions::{assert_impl_all, assert_not_impl_any};
use verter_macro_dto::{
    MacroCodegenEntry, MacroCodegenKind, MacroCodegenOutcome, MacroCodegenSurface,
    MacroEmitCodegen, MacroEmitPayload, MacroEmitsCodegenSurface, MacroPropCodegen,
    MacroPropsCodegenSurface, MacroRootShape, MacroSyntaxAnchor, ResolvedMacroCodegenBundle,
    RuntimeCtorKind,
};
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// Compile-time witness: `T` satisfies BOTH structural markers. Bounds, not
/// assertions — instantiating it for a type that owns a transitive
/// `TypeExpr` or `Span` is a compile error.
fn assert_markers<T: NoTypeExpr + NoStoredSpan>() {}

/// EVERY public aggregate in the crate is a witness for both markers. A new
/// public DTO must be added here; a marker-violating field on any listed
/// type stops this test binary from compiling.
#[test]
fn every_public_aggregate_satisfies_no_typeexpr_and_no_storedspan() {
    assert_markers::<ResolvedMacroCodegenBundle>();
    assert_markers::<MacroCodegenEntry>();
    assert_markers::<MacroCodegenKind>();
    assert_markers::<MacroCodegenOutcome>();
    assert_markers::<MacroCodegenSurface>();
    assert_markers::<MacroPropsCodegenSurface>();
    assert_markers::<MacroRootShape>();
    assert_markers::<MacroPropCodegen>();
    assert_markers::<MacroEmitsCodegenSurface>();
    assert_markers::<MacroEmitCodegen>();
    assert_markers::<MacroEmitPayload>();
    assert_markers::<RuntimeCtorKind>();
    assert_markers::<MacroSyntaxAnchor>();
}

/// NEGATIVE witnesses: the marker bounds reject exactly the two forbidden
/// ownership classes, so the positive test above cannot be vacuous.
///
/// A stored `verter_span::Span` fails `NoStoredSpan` while PASSING
/// `NoTypeExpr` — the two markers discriminate independently, and it is the
/// span marker (not the typeexpr one) that keeps this vocabulary span-free.
/// A stored `verter_type_expr::TypeExpr` fails `NoTypeExpr`. The container
/// impls forward both failures, so `Option<_>` / `Vec<_>` wrapping does not
/// launder a forbidden field.
#[test]
fn span_and_type_expr_ownership_fails_the_markers() {
    // `Span` is a NoTypeExpr witness (the trusted leaf impl lives in
    // `verter_no_typeexpr`)…
    assert_impl_all!(verter_span::Span: NoTypeExpr);
    // …but deliberately NOT a NoStoredSpan witness: a DTO field of this type
    // (at any container depth) would fail `#[derive(NoStoredSpan)]`.
    assert_not_impl_any!(verter_span::Span: NoStoredSpan);
    assert_not_impl_any!(Option<verter_span::Span>: NoStoredSpan);
    assert_not_impl_any!(Vec<verter_span::Span>: NoStoredSpan);

    // The symbolic typed IR satisfies NEITHER marker: a DTO field of this
    // type (at any container depth) would fail `#[derive(NoTypeExpr)]`.
    assert_not_impl_any!(verter_type_expr::TypeExpr: NoTypeExpr);
    assert_not_impl_any!(Option<verter_type_expr::TypeExpr>: NoTypeExpr);
    assert_not_impl_any!(Vec<verter_type_expr::TypeExpr>: NoTypeExpr);
    assert_not_impl_any!(verter_type_expr::TypeExpr: NoStoredSpan);
}

// » would-not-compile witness — the derive-level shape the negative asserts
// above certify. Un-commenting EITHER struct fails this test binary's build
// with an unsatisfied hidden-witness bound on the offending field
// (verified by temporarily un-commenting; kept commented because a
// compile-fail cannot live inline in a passing binary):
//
// #[derive(verter_no_storedspan::NoStoredSpan)]
// struct StoresASpan {
//     // ERROR: `verter_span::Span: NoStoredSpanWitness` is not satisfied
//     span: verter_span::Span,
// }
//
// #[derive(verter_no_typeexpr::NoTypeExpr)]
// struct StoresATypeExpr {
//     // ERROR: `verter_type_expr::TypeExpr: NoTypeExprWitness` is not satisfied
//     body: verter_type_expr::TypeExpr,
// }
