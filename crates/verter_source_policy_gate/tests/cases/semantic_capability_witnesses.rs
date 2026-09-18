//! Compile-time capability witnesses for the sealed output fence.
//!
//! These modules have no `#[test]` bodies: they fail to compile on
//! regression. The compile-fail half lives in the session trybuild
//! fixtures (`no_typeexpr_*`, `output_projector_not_impl_outside_crate`)
//! run by `scripts/compile-contracts.mjs`.

/// The symbolic IR and its common owner shapes must never implement
/// `NoTypeExpr`. If any ever gained the marker, every hot-carrier
/// `NoTypeExpr` field bound would silently stop rejecting `TypeExpr`
/// ownership.
mod hot_structural_rail_not_impl_asserts {
    use static_assertions::assert_not_impl_any;
    use verter_no_typeexpr::NoTypeExpr;
    use verter_type_expr::TypeExpr;

    assert_not_impl_any!(TypeExpr: NoTypeExpr);
    assert_not_impl_any!(Option<TypeExpr>: NoTypeExpr);
    assert_not_impl_any!(Vec<TypeExpr>: NoTypeExpr);
    assert_not_impl_any!(std::sync::Arc<TypeExpr>: NoTypeExpr);
    assert_not_impl_any!(Box<TypeExpr>: NoTypeExpr);
}

/// The internal same-view carrier alias `TypeArgList` is a slice of
/// already-lowered `SemanticNodeId`s. No public `resolve_named_symbol*`
/// entry takes it: the bare `resolve_named_symbol` / `_with_audit`
/// entries accept no type-argument parameter, and the sole type-argument
/// path is `VerterHost::resolve_named_symbol_wire_with_audit`, which
/// accepts symbolic `TypeExpr` payloads and lowers them to node ids
/// inside its audited request under the request's one store view before
/// resolving. `TypeArgList` survives only as the request body's
/// post-lowering same-view carrier.
///
/// The identity coercion below only compiles while `TypeArgList<'a>` IS
/// `&'a [SemanticNodeId]`. A `NoTypeExpr` bound is not usable here:
/// `SemanticNodeId` is a raw keyable arena ordinal that must never
/// satisfy the hot-carrier marker, and a shared reference is never a
/// witness — type identity is the strictly stronger statement.
mod semantic_api_wire_input_witness {
    use verter_session::semantic_query::SemanticNodeId;
    use verter_session::typeinfo::types::TypeArgList;

    const _: fn(TypeArgList<'static>) -> &'static [SemanticNodeId] = |args| args;
}
