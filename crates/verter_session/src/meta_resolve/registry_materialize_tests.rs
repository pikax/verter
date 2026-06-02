//! Tests for the registry-materialisation structure-preserving route
//! preservers — specifically that a bare constructor type
//! (`TypeExpr::ConstructorType`) is walked function-like AND reconstructed as a
//! `ConstructorType` (never flattened to a plain `Function`) while still
//! preserving nested public member routes inside its signature.
//!
//! Both `preserve_registry_callable_param_member_routes` and
//! `preserve_nested_symbolic_member_routes` carry a `(materialized, raw)` pair.
//! After the query-time dispatch raises a constructor type function-like, the
//! realistic shape is `(materialised: Function, raw: ConstructorType)` — a pair
//! the original `(Function, Function)`-only arms fell through to
//! `_ => materialized.clone()`, SILENTLY DROPPING the preserved route AND the
//! constructor-ness.
//!
//! REGRESSION — discriminating: against the pre-fix arms, the result is the
//! materialised plain `Function` with the inlined parameter (route gone, wrong
//! variant), so both the `ConstructorType` assertion and the preserved-route
//! assertion FAIL. Mutation-probe verified by reverting the function-like arm.

use std::sync::Arc;

use verter_type_expr::{
    empty_type_args, FunctionExpr, FunctionParam, LiteralValue, PrimitiveName, TypeExpr,
};

use crate::meta_resolve::preserve_registry_callable_param_member_routes;

/// Build `Foo['bar']` — an indexed-access public member route that
/// `component_meta_registry_public_indexed_access_route` recognises (so the
/// param-level preservation returns the raw route verbatim).
fn indexed_access_route() -> TypeExpr {
    TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: empty_type_args(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("bar".to_string()))),
    }
}

/// A function-like signature carrying ONE parameter typed `ty`.
fn function_expr_with_param(ty: TypeExpr) -> FunctionExpr {
    FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("x".to_string()),
            ty,
            false,
            false,
        )],
        None,
        Vec::new(),
    )
}

#[test]
fn preserve_routes_reconstructs_constructor_type_and_keeps_param_route() {
    // RAW: a bare constructor type whose parameter is the public route
    // `Foo['bar']` (this is the route the preserver must keep symbolic).
    let raw = TypeExpr::ConstructorType(Arc::new(function_expr_with_param(indexed_access_route())));

    // MATERIALISED: the post-dispatch function-like form — a plain `Function`
    // (constructor raised function-like at query time) whose parameter was
    // already inlined to a primitive (`string`). This is the pair the original
    // `(Function, Function)`-only arm could not match.
    let materialized = TypeExpr::Function(Arc::new(function_expr_with_param(TypeExpr::Primitive(
        PrimitiveName::String,
    ))));

    let preserved = preserve_registry_callable_param_member_routes(&materialized, &raw);

    // (1) The reconstructed variant is a `ConstructorType` (raw is
    // authoritative) — NOT flattened to a plain `Function`, NOT
    // `materialized.clone()`.
    let function = match &preserved {
        TypeExpr::ConstructorType(function) => function,
        TypeExpr::Function(_) => {
            panic!("constructor type was flattened to a plain Function — the raw variant must be reconstructed")
        }
        other => panic!("expected TypeExpr::ConstructorType, got {other:?}"),
    };

    // (2) The parameter's public route survived — it is the raw `Foo['bar']`
    // indexed access, NOT the inlined `string` primitive from the materialised
    // side.
    assert_eq!(function.parameters.len(), 1, "single-parameter signature");
    assert_eq!(
        function.parameters[0].ty,
        indexed_access_route(),
        "the constructor-type parameter's public member route Foo['bar'] must be \
         preserved verbatim, not replaced by the materialised `string` primitive",
    );
    assert_ne!(
        function.parameters[0].ty,
        TypeExpr::Primitive(PrimitiveName::String),
        "guards against the pre-fix arm returning materialized.clone() (route lost)",
    );
}

#[test]
fn preserve_routes_constructor_on_both_sides_stays_constructor() {
    // When BOTH sides are constructor types, the result stays a constructor
    // type and the param route is still preserved.
    let raw = TypeExpr::ConstructorType(Arc::new(function_expr_with_param(indexed_access_route())));
    let materialized = TypeExpr::ConstructorType(Arc::new(function_expr_with_param(
        TypeExpr::Primitive(PrimitiveName::String),
    )));

    let preserved = preserve_registry_callable_param_member_routes(&materialized, &raw);

    match &preserved {
        TypeExpr::ConstructorType(function) => {
            assert_eq!(
                function.parameters[0].ty,
                indexed_access_route(),
                "param route preserved through a (ConstructorType, ConstructorType) pair",
            );
        }
        other => panic!("expected TypeExpr::ConstructorType, got {other:?}"),
    }
}

#[test]
fn preserve_routes_plain_function_pair_unaffected() {
    // Negative control: a plain `(Function, Function)` pair still reconstructs a
    // plain `Function` (the constructor-ness flag is false for both), so the
    // function-like merge did not regress the original behaviour.
    let raw = TypeExpr::Function(Arc::new(function_expr_with_param(indexed_access_route())));
    let materialized = TypeExpr::Function(Arc::new(function_expr_with_param(TypeExpr::Primitive(
        PrimitiveName::String,
    ))));

    let preserved = preserve_registry_callable_param_member_routes(&materialized, &raw);

    match &preserved {
        TypeExpr::Function(function) => {
            assert_eq!(
                function.parameters[0].ty,
                indexed_access_route(),
                "plain function param route still preserved",
            );
        }
        other => panic!("expected TypeExpr::Function (plain function unaffected), got {other:?}"),
    }
}
