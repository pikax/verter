use static_assertions::{assert_impl_all, assert_not_impl_any};
use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroPartialReason, MacroRuntimeBundle,
    MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscBundle, MacroTscEntry,
    MacroTscOutcome, MacroTscProjection, ModelRuntimeShape, OrderedRuntimeConstructors,
    PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeEmit, RuntimeProp,
    RuntimePropType, SynthesizedRowKind, TscSpliceText, UnresolvedReason, UnsupportedReason,
};
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

fn assert_markers<T: NoTypeExpr + NoStoredSpan>() {}

#[test]
fn every_public_carrier_is_typeexpr_free_and_span_free() {
    assert_markers::<MacroRuntimeBundle>();
    assert_markers::<MacroRuntimeEntry>();
    assert_markers::<MacroRuntimeOutcome>();
    assert_markers::<MacroRuntimeShape>();
    assert_markers::<PropsRuntimeShape>();
    assert_markers::<PropsDefaultsAssociation>();
    assert_markers::<ModelRuntimeShape>();
    assert_markers::<RuntimeProp>();
    assert_markers::<RuntimeEmit>();
    assert_markers::<RuntimePropType>();
    assert_markers::<OrderedRuntimeConstructors>();
    assert_markers::<RuntimeConstructor>();
    assert_markers::<MacroAnchor>();
    assert_markers::<AuthoredMemberOrdinal>();
    assert_markers::<SynthesizedRowKind>();
    assert_markers::<MacroFailure<MacroPartialReason>>();
    assert_markers::<MacroPartialReason>();
    assert_markers::<UnresolvedReason>();
    assert_markers::<UnsupportedReason>();
    assert_markers::<MacroTscBundle>();
    assert_markers::<MacroTscEntry>();
    assert_markers::<MacroTscOutcome>();
    assert_markers::<MacroTscProjection>();
    assert_markers::<TscSpliceText>();
}

#[test]
fn forbidden_payloads_do_not_implement_the_boundary_markers() {
    assert_not_impl_any!(verter_type_expr::TypeExpr: NoTypeExpr);
    assert_not_impl_any!(verter_span::Span: NoStoredSpan);
    assert_impl_all!(String: NoTypeExpr, NoStoredSpan);
}
