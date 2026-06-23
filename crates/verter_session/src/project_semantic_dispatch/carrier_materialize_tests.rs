//! Carrier-contract foundation tests.
//!
//! `materialize_output_type_expr` is the plain shell-only reverse boundary:
//! every graph carrier round-trips `SemanticNodeId` → `TypeExpr` here,
//! including the unresolved carriers (`BareRef` / `ImportType`), the
//! raw-fallback text carrier, the structural fidelity carrier
//! (`ConstructorType`), the synthetic-binding carrier, the tuple-element
//! `rest` flag (standalone `Rest` has NO graph carrier — tuple-rest fidelity
//! rides on [`TupleElement::rest`]), and the demand-time-minted `RecursiveRef`
//! back-edge (carried as `Opaque(QueryError::RecursiveRef)`).
//!
//! These tests are discriminating: each asserts the EXACT projected
//! `TypeExpr` shape, so a materialiser that dropped a carrier to `Unknown`
//! (or lost ctor-ness / the synthetic `value_node` provenance) would fail.

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr};

use super::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_query_engine::{
    semantic_query_error_raw, BUDGET_EXCEEDED_SENTINEL_PREFIX, SEMANTIC_OBJECT_SURFACE,
    SEMANTIC_SURFACE_MEMBER,
};
use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::{
    HotTypeRef, NodeScopeId, PrimitiveKind, QueryError, ScopeId, SemanticNodeData, SemanticNodeId,
    SyntheticBindingId, TupleElement, ValueRootKey,
};
use crate::VerterHost;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn hot_type_ref_is_send_sync_and_round_trips_node() {
    // `HotTypeRef` must be host-cache-safe (`Send + Sync`).
    assert_send_sync::<HotTypeRef>();

    let id = SemanticNodeId(7);
    let handle = HotTypeRef::new(id);
    assert_eq!(
        handle.node(),
        id,
        "HotTypeRef must round-trip the wrapped SemanticNodeId"
    );
}

#[test]
fn synthetic_binding_id_is_content_free_and_rehydrates() {
    let key = SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/Comp.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::Binding,
        slot_name: None,
        binding_name: Arc::from("row"),
        value_node: 99,
    };
    let id = SyntheticBindingId::from_carrier_key(&key);

    // The identity carries the four content-free fields, NOT `value_node`.
    assert_eq!(id.scope_canonical_id.as_ref(), "/Comp.vue");
    assert_eq!(id.binding_name.as_ref(), "row");

    // Re-hydration with the original ordinal reconstructs the full key.
    assert_eq!(id.to_carrier_key(99), key);

    // A DIFFERENT ordinal yields a different carrier key, but the SAME
    // content-free identity — proving the ordinal is not part of identity.
    let other = id.to_carrier_key(123);
    assert_ne!(other, key);
    assert_eq!(SyntheticBindingId::from_carrier_key(&other), id);
}

/// Shared harness: intern `data`, materialise it through the plain
/// shell-only output boundary, return the `TypeExpr`. A miss maps to the
/// `"<materialize miss>"` sentinel (none of these carriers miss — the
/// shapes are asserted exactly below).
fn materialize(host: &VerterHost, data: SemanticNodeData) -> TypeExpr {
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let node = graph.intern_node(data);
    let dispatch = ProjectSemanticDispatch::new(host);
    dispatch
        .materialize_output_type_expr(node)
        .unwrap_or(TypeExpr::Unknown {
            raw: "<materialize miss>".to_string(),
        })
}

#[test]
fn materialize_bare_ref_round_trips_to_bare_ref() {
    let host = VerterHost::new_standalone(Default::default());
    let expr = materialize(
        &host,
        SemanticNodeData::new_bare_ref(
            Arc::from("Foo"),
            NodeScopeId::Global,
            Arc::from(Vec::new().into_boxed_slice()),
        ),
    );
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "Foo");
            assert!(type_arguments.is_empty(), "bare ref carries no type args");
        }
        other => panic!("expected Ref, got {other:?}"),
    }
}

#[test]
fn materialize_bare_ref_round_trips_type_args() {
    // A `BareRef` with NON-EMPTY `type_args` must round-trip the args onto
    // `Ref.type_arguments`. Locks the structural materialize/raise path against
    // dropping the carrier's `type_args` — a raise arm that ignored the field
    // would yield an empty arg list and FAIL this fixture (discriminating).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let node = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![arg].into_boxed_slice()),
    ));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .materialize_output_type_expr(node)
        .expect("carrier must raise through the plain output boundary");
    match &expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(name.as_ref(), "Foo");
            assert_eq!(
                type_arguments.len(),
                1,
                "BareRef.type_args must round-trip onto Ref.type_arguments"
            );
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::Primitive(PrimitiveName::Number)
            ));
        }
        other => panic!("expected Ref, got {other:?}"),
    }
}

#[test]
fn materialize_typeof_round_trips_type_args() {
    // There was no TypeOf materialize fixture, so raise.rs could have dropped
    // the `type_args` field and stayed green. A `TypeOf` with NON-EMPTY
    // `type_args` must round-trip the instantiation args onto
    // `ValueRef.type_args` (and the root + path onto `ValueRef.path`).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/m.ts"),
                local_scope: None,
            },
            name: Arc::from("factory"),
        },
        Arc::from(vec![Arc::<str>::from("make")].into_boxed_slice()),
        Arc::from(vec![arg].into_boxed_slice()),
    ));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .materialize_output_type_expr(node)
        .expect("carrier must raise through the plain output boundary");
    match &expr {
        TypeExpr::TypeOf(value_ref) => {
            assert_eq!(
                value_ref.path,
                vec!["factory".to_string(), "make".to_string()],
                "the value root + projected path round-trip onto ValueRef.path"
            );
            assert_eq!(
                value_ref.type_args.len(),
                1,
                "TypeOf.type_args must round-trip onto ValueRef.type_args"
            );
            assert!(matches!(
                &value_ref.type_args[0],
                TypeExpr::Primitive(PrimitiveName::String)
            ));
        }
        other => panic!("expected TypeOf, got {other:?}"),
    }
}

#[test]
fn materialize_import_type_round_trips() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let node = graph.intern_node(SemanticNodeData::new_import_type(
        Arc::from("./module"),
        Arc::from(vec![Arc::<str>::from("A"), Arc::<str>::from("B")].into_boxed_slice()),
        Arc::from(vec![arg].into_boxed_slice()),
        true,
    ));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .materialize_output_type_expr(node)
        .expect("carrier must raise through the plain output boundary");

    match &expr {
        TypeExpr::ImportType {
            specifier,
            qualifier,
            typeof_query,
            type_arguments,
        } => {
            assert_eq!(specifier.as_ref(), "./module");
            assert_eq!(qualifier.len(), 2);
            assert_eq!(qualifier[0].as_ref(), "A");
            assert_eq!(qualifier[1].as_ref(), "B");
            assert!(*typeof_query, "typeof import must round-trip");
            assert_eq!(type_arguments.len(), 1);
            assert!(matches!(
                &type_arguments[0],
                TypeExpr::Primitive(PrimitiveName::Number)
            ));
        }
        other => panic!("expected ImportType, got {other:?}"),
    }
}

#[test]
fn materialize_raw_fallback_round_trips_to_unknown() {
    let host = VerterHost::new_standalone(Default::default());
    let expr = materialize(
        &host,
        SemanticNodeData::RawFallback {
            raw: Arc::from("Weird<& Type>"),
        },
    );
    match &expr {
        // The raw-fallback carrier is the ONLY carrier that holds raw text;
        // it round-trips verbatim to `Unknown { raw }`.
        TypeExpr::Unknown { raw } => assert_eq!(raw, "Weird<& Type>"),
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn materialize_tuple_preserves_element_rest_flag() {
    // Standalone `Rest` has NO graph carrier — tuple-rest fidelity rides on
    // `TupleElement.rest`. This asserts the tuple materialize arm carries the
    // `rest` flag (and the operand) through the reverse boundary; it FAILS if
    // the arm drops `rest` to `false`.
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                value,
                optional: false,
                rest: true,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .materialize_output_type_expr(node)
        .expect("carrier must raise through the plain output boundary");

    match &expr {
        TypeExpr::Tuple { elements, .. } => {
            assert_eq!(elements.len(), 1, "one tuple element");
            assert!(
                elements[0].rest,
                "the tuple-element `rest` flag must round-trip through materialize"
            );
            assert!(
                !elements[0].optional,
                "a non-optional rest element stays non-optional"
            );
            assert!(
                matches!(&elements[0].ty, TypeExpr::Primitive(PrimitiveName::String)),
                "the rest operand value must materialize correctly, got {:?}",
                elements[0].ty
            );
        }
        other => panic!("expected Tuple, got {other:?}"),
    }
}

#[test]
fn materialize_constructor_type_preserves_ctor_ness() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let signature = graph.intern_node(SemanticNodeData::Function {
        params: Arc::from(Vec::new().into_boxed_slice()),
        return_type: ret,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    let node = graph.intern_node(SemanticNodeData::ConstructorType { signature });
    let dispatch = ProjectSemanticDispatch::new(&host);
    let expr = dispatch
        .materialize_output_type_expr(node)
        .expect("carrier must raise through the plain output boundary");

    // Must round-trip as a CONSTRUCTOR type, not a plain function — the
    // whole reason the carrier exists is to keep `new () => R` distinct
    // from `() => R`.
    assert!(
        matches!(&expr, TypeExpr::ConstructorType(_)),
        "expected ConstructorType, got {expr:?}"
    );
    assert!(
        !matches!(&expr, TypeExpr::Function(_)),
        "constructor-ness must NOT collapse to a plain Function"
    );
}

#[test]
fn materialize_synthetic_binding_round_trips_with_value_node() {
    let host = VerterHost::new_standalone(Default::default());
    let id = SyntheticBindingId {
        scope_canonical_id: Arc::from("/Comp.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("row"),
    };
    let expr = materialize(
        &host,
        SemanticNodeData::SyntheticBinding { id, value_node: 42 },
    );
    match &expr {
        TypeExpr::SyntheticSlotBinding(key) => {
            assert_eq!(key.scope_canonical_id.as_ref(), "/Comp.vue");
            assert_eq!(key.binding_name.as_ref(), "row");
            assert_eq!(key.slot_name.as_deref(), Some("default"));
            assert!(matches!(
                key.surface_kind,
                SyntheticCarrierSurfaceKind::SlotBinding
            ));
            // The value-side provenance ordinal is re-attached at the compat
            // boundary.
            assert_eq!(key.value_node, 42);
        }
        other => panic!("expected SyntheticSlotBinding, got {other:?}"),
    }
}

#[test]
fn materialize_recursive_ref_back_edge_round_trips() {
    let host = VerterHost::new_standalone(Default::default());
    // `RecursiveRef` is demand-time-minted as `Opaque(QueryError::RecursiveRef)`
    // and the reverse boundary projects it to `TypeExpr::RecursiveRef`.
    let expr = materialize(
        &host,
        SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::from("Tree"),
        }),
    );
    match &expr {
        TypeExpr::RecursiveRef { name, .. } => assert_eq!(name.as_ref(), "Tree"),
        other => panic!("expected RecursiveRef, got {other:?}"),
    }
}

/// The `HotTypeRef`-shaped test harness `materialize_type_expr` projects the
/// SAME `TypeExpr` as the plain `materialize_output_type_expr` boundary for the
/// same node (behavioral equivalence of the harness wrapper); it does not
/// assert the harness routes through that boundary internally.
///
/// It also pins the harness's `None`-miss mapping: a node ABSENT from the live
/// graph store makes the plain boundary return `None`, and the harness maps that
/// miss to the `"<materialize miss>"` sentinel.
#[test]
fn hot_type_ref_harness_matches_plain_output_boundary() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let node = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    let dispatch = ProjectSemanticDispatch::new(&host);

    let via_harness = dispatch.materialize_type_expr(HotTypeRef::new(node));
    let via_plain = dispatch
        .materialize_output_type_expr(node)
        .expect("the bare-ref carrier raises through the plain boundary");

    // Both routes land the same `Ref { name: "Foo" }` — the harness wrapper is
    // behaviorally equivalent to the plain boundary for the same node.
    assert_eq!(
        via_harness, via_plain,
        "materialize_type_expr(HotTypeRef) must project the SAME TypeExpr as \
         materialize_output_type_expr(node) for the same node"
    );
    assert!(
        matches!(&via_harness, TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"),
        "expected the bare-ref to raise to Ref{{name=Foo}}, got {via_harness:?}"
    );

    // Miss case: a node ordinal never interned into this fresh graph store is
    // absent, so the plain boundary returns `None` and the harness maps the miss
    // to the `"<materialize miss>"` sentinel. This pins the harness's own
    // miss-unwrap behavior (the only behavior it adds over the plain boundary).
    let absent = SemanticNodeId(u64::MAX);
    assert!(
        dispatch.materialize_output_type_expr(absent).is_none(),
        "an un-interned node ordinal must miss the plain boundary"
    );
    assert!(
        matches!(
            &dispatch.materialize_type_expr(HotTypeRef::new(absent)),
            TypeExpr::Unknown { raw } if raw == "<materialize miss>"
        ),
        "the harness must map a plain-boundary miss to the `<materialize miss>` sentinel"
    );
}

/// Each typed semantic-sentinel `QueryError` must serialize to the
/// BYTE-IDENTICAL legacy raw string, and the `BudgetExceeded` fuse must keep
/// its prefix (the must-not-regress). A future stage that swaps a raw
/// `Unknown { raw: "X" }` construction for `Opaque(QueryError::…)` then
/// raises must produce the same text.
#[test]
fn typed_query_error_sentinels_round_trip_to_legacy_raw() {
    assert_eq!(
        semantic_query_error_raw(&QueryError::RaiseAliasCycle),
        "semanticAliasCycle"
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::TypeParamCycle),
        "semanticTypeParamCycle"
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::RaiseMiss),
        "<raise miss>"
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::UnrepresentableSurface),
        SEMANTIC_OBJECT_SURFACE
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::UnrepresentableSurfaceMember),
        SEMANTIC_SURFACE_MEMBER
    );
    assert_eq!(
        semantic_query_error_raw(&QueryError::VueMacroElementsPlaceholder),
        "VueMacroElements"
    );

    // must-not-regress: the budget-exceeded fuse keeps its sentinel prefix.
    let budget = QueryError::BudgetExceeded(BudgetExceededFailure {
        domain: BudgetDomain::ProjectionOperation,
        limit: 1,
        actual: 2,
        context: "pls1-budget-sentinel".to_string(),
    });
    assert!(
        semantic_query_error_raw(&budget).starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX),
        "BUDGET_EXCEEDED_SENTINEL must not regress"
    );
}
