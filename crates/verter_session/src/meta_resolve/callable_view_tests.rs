//! Parity / oracle tests for the shared node-domain callable/signature view.
//!
//! Each test is DISCRIMINATING: it asserts a SPECIFIC node fact and FAILS on a
//! wrong projection. Where a `TypeExpr`-domain helper still exists (it does
//! until §5a SP4), the strongest tests materialize the view's node ONCE via the
//! test cap and assert it equals the legacy helper's output for the same
//! fixture (`single_callable_arm` ↔ `callable_arm_from_raised`).

use std::sync::Arc;

use verter_type_expr::{MemberVisibility, PrimitiveName, TypeExpr};

use super::{ArmCombineNode, CallableNodeView};
use crate::meta_resolve::dispatch_helpers::{callable_arm_from_raised, realize_callable_member};
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext, ResolverContext};
use crate::resolver_store::CurrentHostStoreView;
use crate::semantic_query::{
    DeclIdentity, FunctionParam, LiteralValue, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, SemanticNodeData, SemanticNodeId, SurfaceMember,
    SurfaceView, TupleElement,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::typeinfo::framework_surface::vue_exec::navigate_param_to_object_surface;
use crate::types::HostConfig;
use crate::VerterHost;

// ───────────────────────────── node builders ─────────────────────────────

fn prim(graph: &SemanticGraphStore, kind: PrimitiveKind) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Primitive(kind))
}

fn string_literal(graph: &SemanticGraphStore, value: &str) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        value.to_string(),
    )))
}

fn param(name: Option<&str>, ty: SemanticNodeId, optional: bool, rest: bool) -> FunctionParam {
    FunctionParam::synthetic(name.map(Arc::from), ty, optional, rest)
}

fn function(
    graph: &SemanticGraphStore,
    params: Vec<FunctionParam>,
    return_type: SemanticNodeId,
) -> SemanticNodeId {
    function_with_return_span(graph, params, return_type, None)
}

fn function_with_return_span(
    graph: &SemanticGraphStore,
    params: Vec<FunctionParam>,
    return_type: SemanticNodeId,
    return_type_span: Option<verter_span::Span>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Function {
        params: Arc::from(params.into_boxed_slice()),
        return_type,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        signature_span: None,
        return_type_span,
    })
}

fn union(graph: &SemanticGraphStore, arms: Vec<SemanticNodeId>) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Union(Arc::from(arms.into_boxed_slice())))
}

fn intersection(graph: &SemanticGraphStore, arms: Vec<SemanticNodeId>) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        arms.into_boxed_slice(),
    )))
}

fn alias(graph: &SemanticGraphStore, inner: SemanticNodeId) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Alias(inner))
}

fn tuple(graph: &SemanticGraphStore, elements: Vec<TupleElement>) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(elements.into_boxed_slice()),
        readonly: false,
    })
}

fn tuple_element(label: Option<&str>, value: SemanticNodeId) -> TupleElement {
    TupleElement {
        label: label.map(Arc::from),
        value,
        optional: false,
        rest: false,
    }
}

fn object_surface(
    graph: &SemanticGraphStore,
    members: &[(&str, SemanticNodeId)],
) -> SemanticNodeId {
    let members: Vec<SurfaceMember> = members
        .iter()
        .map(|(name, value)| SurfaceMember {
            name: Arc::from(*name),
            value: *value,
            optional: false,
            readonly: false,
            is_method: false,
            visibility: MemberVisibility::Public,
            spans: Default::default(),
            declaration_origin: None,
            declared_in_macro_type_arg: false,
            merge_role: Default::default(),
        })
        .collect();
    let view = SurfaceView {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    };
    graph.intern_node(SemanticNodeData::Object(view))
}

fn navigate() -> ProjectionReductionContext {
    ProjectionReductionContext::published(ProjectionMode::Navigate)
}

fn shallow() -> ProjectionReductionContext {
    ProjectionReductionContext::published(ProjectionMode::Shallow)
}

// ───────────────────────── single_callable_arm ───────────────────────────

#[test]
fn single_callable_arm_returns_bare_function() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);

    let view = CallableNodeView::new(&dispatch, f);
    assert_eq!(
        view.single_callable_arm(navigate()),
        Some(f),
        "a bare Function root IS its own single callable arm"
    );
}

#[test]
fn single_callable_arm_unwraps_alias_to_function() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let f = function(&graph, vec![], void);
    let aliased = alias(&graph, f);

    let view = CallableNodeView::new(&dispatch, aliased);
    assert_eq!(
        view.single_callable_arm(navigate()),
        Some(f),
        "an Alias(Function) realizes (composing realize_callable_member) to the Function node"
    );
}

#[test]
fn single_callable_arm_strips_nullish_undefined() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);
    let undefined = prim(&graph, PrimitiveKind::Undefined);
    let nullish = union(&graph, vec![f, undefined]);

    let view = CallableNodeView::new(&dispatch, nullish);
    assert_eq!(
        view.single_callable_arm(navigate()),
        Some(f),
        "`((r) => void) | undefined` yields the callable arm (undefined stripped)"
    );
}

#[test]
fn single_callable_arm_strips_nullish_null() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let f = function(&graph, vec![], void);
    let null = prim(&graph, PrimitiveKind::Null);
    let nullish = union(&graph, vec![null, f]);

    let view = CallableNodeView::new(&dispatch, nullish);
    assert_eq!(
        view.single_callable_arm(navigate()),
        Some(f),
        "`(() => void) | null` yields the callable arm (null stripped)"
    );
}

#[test]
fn single_callable_arm_ambiguous_two_callables_refuses() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let string = prim(&graph, PrimitiveKind::String);
    let fn_a = function(&graph, vec![param(Some("a"), row, false, false)], void);
    let fn_b = function(&graph, vec![param(Some("b"), string, false, false)], void);
    assert_ne!(fn_a, fn_b, "the two callables are distinct interned nodes");
    let ambiguous = union(&graph, vec![fn_a, fn_b]);

    let view = CallableNodeView::new(&dispatch, ambiguous);
    assert_eq!(
        view.single_callable_arm(navigate()),
        None,
        "two distinct callable arms are ambiguous — refuse rather than pick one"
    );
}

#[test]
fn single_callable_arm_non_nullish_non_callable_union_refuses() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let a = string_literal(&graph, "a");
    let b = string_literal(&graph, "b");
    let non_callable = union(&graph, vec![a, b]);

    let view = CallableNodeView::new(&dispatch, non_callable);
    assert_eq!(
        view.single_callable_arm(navigate()),
        None,
        "`'a' | 'b'` is a non-nullish non-callable union — not a callable"
    );
}

#[test]
fn single_callable_arm_non_callable_scalar_refuses() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let string = prim(&graph, PrimitiveKind::String);
    let view = CallableNodeView::new(&dispatch, string);
    assert_eq!(
        view.single_callable_arm(navigate()),
        None,
        "a non-callable scalar (string) is not a callable"
    );
}

#[test]
fn single_callable_arm_matches_callable_arm_from_raised() {
    // STRONGEST parity discriminator: the view's realized node, materialized
    // once via the test cap, equals `callable_arm_from_raised` on the
    // materialized union value (the legacy offender path).
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);
    let undefined = prim(&graph, PrimitiveKind::Undefined);
    let nullish = union(&graph, vec![f, undefined]);

    let view = CallableNodeView::new(&dispatch, nullish);
    let arm = view
        .single_callable_arm(navigate())
        .expect("the nullish union yields a single callable arm");
    assert_eq!(arm, f, "the view's callable arm is the Function node");

    let view_mat = dispatch
        .materialize_output_type_expr_for_test(arm)
        .expect("the view node materializes");
    let TypeExpr::Function(view_func) = &view_mat else {
        panic!("the view node materializes to a Function, got {view_mat:?}");
    };

    let union_mat = dispatch
        .materialize_output_type_expr_for_test(nullish)
        .expect("the union value materializes");
    let helper_func = callable_arm_from_raised(&union_mat)
        .expect("callable_arm_from_raised extracts the single callable");

    assert_eq!(
        view_func.as_ref(),
        helper_func.as_ref(),
        "the view's node-domain callable equals callable_arm_from_raised's materialized callable"
    );
}

#[test]
fn single_callable_arm_intersection_with_nullish_refuses() {
    // SOUNDNESS (#2): `Fn & undefined` = `never` — NOT callable. Pre-fix the
    // view stripped nullish arms UNIFORMLY for both `Union` and `Intersection`,
    // so an `Intersection(Function, undefined)` wrongly returned `Some(f)`.
    // Post-fix only a `Union` narrows a nullish arm away; an `Intersection` with
    // a nullish arm refuses (`None`).
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);
    let undefined = prim(&graph, PrimitiveKind::Undefined);
    let isect = intersection(&graph, vec![f, undefined]);

    assert_eq!(
        CallableNodeView::new(&dispatch, isect).single_callable_arm(navigate()),
        None,
        "`Fn & undefined` = never — an Intersection with a nullish arm is not callable"
    );

    // DISCRIMINATING contrast: the SAME two arms as a UNION still narrow the
    // nullish arm away and yield the callable — so the refusal above is the
    // Intersection-specific `never` collapse, NOT a blanket reject of the arms.
    let as_union = union(&graph, vec![f, undefined]);
    assert_eq!(
        CallableNodeView::new(&dispatch, as_union).single_callable_arm(navigate()),
        Some(f),
        "`Fn | undefined` narrows the nullish arm away to the surviving callable"
    );
}

#[test]
fn single_callable_arm_never_intersection_under_union_refuses() {
    // DECIDED behavior (codex-architect adjudication, fix cycle 3): the view's
    // `(Fn & undefined) | Fn` resolves to `None`, NOT the surviving `Fn`. The
    // `Fn & undefined` arm is `never` (non-callable), so the union carries no
    // single unambiguous callable.
    //
    // This documents the REFUTED tri-state proposal: a view-only tri-state that
    // elided the `never` intersection and returned `Fn` would make the view
    // DIVERGE from the single shared resolver `realize_callable_member` (which
    // ALSO returns `None` here — asserted below). Canonical-consistency with the
    // shared resolver is the invariant; tri-state precision, if ever wanted, must
    // change the shared resolver FIRST (out of §5a scope). FLIP: a tri-state
    // implementation that returned `Some(f)` here fails this `None` assertion.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);
    let undefined = prim(&graph, PrimitiveKind::Undefined);
    let fn_and_undefined = intersection(&graph, vec![f, undefined]);
    let composite = union(&graph, vec![fn_and_undefined, f]);

    assert_eq!(
        CallableNodeView::new(&dispatch, composite).single_callable_arm(navigate()),
        None,
        "`(Fn & undefined) | Fn` refuses: the intersection arm is `never`, so no single unambiguous callable remains"
    );

    // CANONICAL-CONSISTENCY (the adjudicated crux): the SHARED resolver
    // `realize_callable_member` ALSO returns `None` for this composite (its
    // Intersection arm `?`-fails the whole composite on the `undefined` arm). The
    // view MATCHES the shared resolver — a view-only tri-state would break this
    // parity. (The legacy TypeExpr helper `callable_arm_from_raised` is more
    // lenient and would return `Some(f)`; the view deliberately follows the
    // shared resolver, not the legacy helper.)
    assert_eq!(
        realize_callable_member(&dispatch, composite, navigate()),
        None,
        "realize_callable_member ALSO refuses `(Fn & undefined) | Fn` — the view is canonical-consistent with the shared resolver"
    );
}

// ──────────────────────────── event_names ────────────────────────────────

#[test]
fn event_names_single_string_literal() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let click = string_literal(&graph, "click");
    let f = function(&graph, vec![param(Some("e"), click, false, false)], void);

    let view = CallableNodeView::new(&dispatch, f);
    assert_eq!(
        view.event_names(navigate()),
        Some(vec![Arc::<str>::from("click")]),
        "a single string-literal first param yields one event name"
    );
}

#[test]
fn event_names_union_of_string_literals() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let save = string_literal(&graph, "save");
    let cancel = string_literal(&graph, "cancel");
    let names = union(&graph, vec![save, cancel]);
    let f = function(&graph, vec![param(Some("e"), names, false, false)], void);

    let view = CallableNodeView::new(&dispatch, f);
    assert_eq!(
        view.event_names(navigate()),
        Some(vec![Arc::<str>::from("save"), Arc::<str>::from("cancel")]),
        "a union of string-literal first params yields each event name in order"
    );
}

#[test]
fn event_names_non_literal_first_param_is_none() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let string = prim(&graph, PrimitiveKind::String);
    let f = function(&graph, vec![param(Some("e"), string, false, false)], void);

    let view = CallableNodeView::new(&dispatch, f);
    assert_eq!(
        view.event_names(navigate()),
        None,
        "a non-literal (string) first param yields no event names"
    );
}

#[test]
fn event_names_no_params_is_none() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let f = function(&graph, vec![], void);

    let view = CallableNodeView::new(&dispatch, f);
    assert_eq!(
        view.event_names(navigate()),
        None,
        "a no-param callable declares no event names"
    );
}

// ───────────────────── slot_param_and_return_by_arm ──────────────────────

#[test]
fn slot_param_and_return_single_function() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let props = prim(&graph, PrimitiveKind::Object);
    let f = function(
        &graph,
        vec![param(Some("props"), props, false, false)],
        void,
    );

    let view = CallableNodeView::new(&dispatch, f);
    let parts = view
        .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow())
        .expect("a single Function slot yields parts");
    assert_eq!(
        parts.first_param,
        Some(props),
        "the first param is the function's first param node"
    );
    assert_eq!(
        parts.return_type,
        Some(void),
        "the return type is the function's return node"
    );
}

#[test]
fn slot_param_and_return_every_arm_has_first_param_intersects() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    // DISTINCT return nodes (`number` vs `string`) so the combiner mode is
    // DISCRIMINATING: a test asserting only `is_some()` would pass even if the
    // combiner were ignored. Here we assert the exact combined return shape for
    // BOTH `Union` and `Intersection`.
    let p1 = prim(&graph, PrimitiveKind::Number);
    let p2 = prim(&graph, PrimitiveKind::String);
    let r1 = prim(&graph, PrimitiveKind::Boolean);
    let r2 = prim(&graph, PrimitiveKind::Object);
    let f1 = function(&graph, vec![param(Some("a"), p1, false, false)], r1);
    let f2 = function(&graph, vec![param(Some("b"), p2, false, false)], r2);
    let slot = union(&graph, vec![f1, f2]);

    let view = CallableNodeView::new(&dispatch, slot);

    // ── Union combiner ──
    let union_parts = view
        .slot_param_and_return_by_arm(ArmCombineNode::Union, shallow())
        .expect("a 2-arm slot yields parts");
    let first_param = union_parts
        .first_param
        .expect("every arm supplies a first param -> intersected binding");
    let first_mat = dispatch
        .materialize_output_type_expr_for_test(first_param)
        .expect("the combined first param materializes");
    let TypeExpr::Intersection(first_arms) = &first_mat else {
        panic!("the combined first param is an Intersection of both arms, got {first_mat:?}");
    };
    assert_eq!(
        first_arms.len(),
        2,
        "the intersection carries both first params"
    );
    assert!(
        first_arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Number))),
        "one arm's first param is `number`"
    );
    assert!(
        first_arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::String))),
        "the other arm's first param is `string`"
    );
    let union_ret = dispatch
        .materialize_output_type_expr_for_test(
            union_parts
                .return_type
                .expect("Union combine yields a return"),
        )
        .expect("the combined return materializes");
    let TypeExpr::Union(union_ret_arms) = &union_ret else {
        panic!("ArmCombineNode::Union must combine the DISTINCT returns into a Union, got {union_ret:?}");
    };
    assert_eq!(
        union_ret_arms.len(),
        2,
        "the Union return carries both arms' returns"
    );
    assert!(
        union_ret_arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Boolean))),
        "one arm's return is `boolean`"
    );
    assert!(
        union_ret_arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Object))),
        "the other arm's return is `object`"
    );

    // ── Intersection combiner (same fixture, different combine) ──
    let isect_parts = view
        .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow())
        .expect("a 2-arm slot yields parts");
    let isect_ret = dispatch
        .materialize_output_type_expr_for_test(
            isect_parts
                .return_type
                .expect("Intersection combine yields a return"),
        )
        .expect("the combined return materializes");
    let TypeExpr::Intersection(isect_ret_arms) = &isect_ret else {
        panic!("ArmCombineNode::Intersection must combine the DISTINCT returns into an Intersection, got {isect_ret:?}");
    };
    assert_eq!(
        isect_ret_arms.len(),
        2,
        "the Intersection return carries both arms' returns"
    );
    assert!(
        isect_ret_arms
            .iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Boolean)))
            && isect_ret_arms
                .iter()
                .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Object))),
        "the Intersection return carries both `boolean` and `object`"
    );
}

#[test]
fn slot_param_and_return_one_arm_lacks_first_param_drops_binding() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let p1 = prim(&graph, PrimitiveKind::Number);
    let with_param = function(&graph, vec![param(Some("a"), p1, false, false)], void);
    let no_param = function(&graph, vec![], void);
    let slot = union(&graph, vec![with_param, no_param]);

    let view = CallableNodeView::new(&dispatch, slot);
    let parts = view
        .slot_param_and_return_by_arm(ArmCombineNode::Union, shallow())
        .expect("a 2-arm slot (one no-param) still yields parts");
    assert_eq!(
        parts.first_param, None,
        "a no-param arm guarantees no binding -> the first param is dropped to None"
    );
    assert!(
        parts.return_type.is_some(),
        "the return type still combines across arms even when the binding is dropped"
    );
}

#[test]
fn slot_param_and_return_non_callable_arm_fails_closed() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let p1 = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("a"), p1, false, false)], void);
    let string = prim(&graph, PrimitiveKind::String);
    let slot = union(&graph, vec![f, string]);

    let view = CallableNodeView::new(&dispatch, slot);
    assert_eq!(
        view.slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow()),
        None,
        "a non-callable arm makes the member not slot-callable -> fail closed"
    );
}

#[test]
fn slot_param_and_return_intersection_with_nullish_fails_closed() {
    // SOUNDNESS (#2) on the slot-callable path: `Slot & undefined` = `never`,
    // not slot-callable. `collect_callable_arms` refuses an `Intersection` with a
    // nullish arm (matching `classify_single_callable`); a `Union` still strips
    // the nullish arm and yields parts.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let props = prim(&graph, PrimitiveKind::Object);
    let f = function(
        &graph,
        vec![param(Some("props"), props, false, false)],
        void,
    );
    let undefined = prim(&graph, PrimitiveKind::Undefined);

    let isect = intersection(&graph, vec![f, undefined]);
    assert_eq!(
        CallableNodeView::new(&dispatch, isect)
            .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow()),
        None,
        "`Slot & undefined` = never — fails closed (not slot-callable)"
    );

    // DISCRIMINATING contrast: the SAME arms as a UNION strip the nullish arm
    // and still yield parts with the surviving binding.
    let as_union = union(&graph, vec![f, undefined]);
    let parts = CallableNodeView::new(&dispatch, as_union)
        .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow())
        .expect("`Slot | undefined` strips the nullish arm and yields parts");
    assert_eq!(
        parts.first_param,
        Some(props),
        "the surviving callable supplies the first-param binding"
    );
}

// ──────────────────────────── positional_params ──────────────────────────

#[test]
fn positional_params_skips_this_and_expands_rest_tuple() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let item = prim(&graph, PrimitiveKind::Number);
    let index = prim(&graph, PrimitiveKind::String);
    let params_tuple = tuple(
        &graph,
        vec![
            tuple_element(Some("item"), item),
            tuple_element(Some("index"), index),
        ],
    );
    // Snippet-style `(this: void, ...args: [item: number, index: string])`.
    let snippet = function(
        &graph,
        vec![
            param(Some("this"), void, false, false),
            param(Some("args"), params_tuple, false, true),
        ],
        void,
    );

    let view = CallableNodeView::new(&dispatch, snippet);
    let positions = view
        .positional_params(navigate())
        .expect("the snippet callable yields positional params");
    assert_eq!(
        positions.len(),
        2,
        "`this` is skipped and the rest-tuple expands to 2 entries"
    );
    assert_eq!(positions[0].label.as_deref(), Some("item"));
    assert_eq!(
        positions[0].ty, item,
        "the first positional type is the tuple's first element node"
    );
    assert_eq!(positions[1].label.as_deref(), Some("index"));
    assert_eq!(positions[1].ty, index);
}

#[test]
fn positional_params_plain_two_params() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let a_ty = prim(&graph, PrimitiveKind::Number);
    let b_ty = prim(&graph, PrimitiveKind::String);
    let f = function(
        &graph,
        vec![
            param(Some("a"), a_ty, false, false),
            param(Some("b"), b_ty, false, false),
        ],
        void,
    );

    let view = CallableNodeView::new(&dispatch, f);
    let positions = view
        .positional_params(navigate())
        .expect("a plain callable yields its positional params");
    assert_eq!(positions.len(), 2, "both positional params are emitted");
    assert_eq!(positions[0].label.as_deref(), Some("a"));
    assert_eq!(positions[0].ty, a_ty);
    assert_eq!(positions[1].label.as_deref(), Some("b"));
    assert_eq!(positions[1].ty, b_ty);
}

// ───────────────── signature accessors / realized_callable_root ──────────

#[test]
fn signature_accessors_read_function_facts() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let row = prim(&graph, PrimitiveKind::Number);
    let ret = prim(&graph, PrimitiveKind::String);
    let span = verter_span::Span::new(5, 11);
    let f = function_with_return_span(
        &graph,
        vec![param(Some("r"), row, false, false)],
        ret,
        Some(span),
    );

    let view = CallableNodeView::new(&dispatch, f);
    let sig = view
        .signature(navigate())
        .expect("the root realizes to a signature");
    assert_eq!(
        sig.first_param(),
        Some(row),
        "first_param reads the function's first param node"
    );
    assert_eq!(
        sig.return_type(),
        Some(ret),
        "return_type reads the function's return node (fail-closed Option)"
    );
    assert_eq!(
        sig.return_type_span(),
        Some(span),
        "return_type_span reads the stored span (not a constant None)"
    );
    let positions = sig.positional_params_expanded(navigate());
    assert_eq!(positions.len(), 1, "the single positional param surfaces");
    assert_eq!(positions[0].ty, row);
}

#[test]
fn signature_is_none_for_multi_arm_composite() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let f1 = function(&graph, vec![], void);
    let p = prim(&graph, PrimitiveKind::Number);
    let f2 = function(&graph, vec![param(Some("a"), p, false, false)], void);
    let composite = union(&graph, vec![f1, f2]);

    let view = CallableNodeView::new(&dispatch, composite);
    assert!(
        view.signature(navigate()).is_none(),
        "a multi-arm composite is not a single signature"
    );
}

#[test]
fn realized_callable_root_normalizes_alias() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let f = function(&graph, vec![], void);
    let aliased = alias(&graph, f);

    let view = CallableNodeView::new(&dispatch, aliased);
    assert_eq!(
        view.realized_callable_root(navigate()),
        Some(f),
        "realized_callable_root normalizes Alias(Function) to the Function node"
    );
}

// ────────────────────── first_param_object_surface ───────────────────────

#[test]
fn first_param_object_surface_projects_param_members() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let foo = prim(&graph, PrimitiveKind::Number);
    let bar = prim(&graph, PrimitiveKind::String);
    let props = object_surface(&graph, &[("foo", foo), ("bar", bar)]);
    let f = function(
        &graph,
        vec![param(Some("props"), props, false, false)],
        void,
    );

    let view = CallableNodeView::new(&dispatch, f);
    let surface = view
        .first_param_object_surface(&host, shallow())
        .expect("the first-param object projects a one-level surface");
    let mut names: Vec<&str> = surface.members.iter().map(|m| m.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["bar", "foo"],
        "the first-param surface carries BOTH object members"
    );
}

// ──────────────── integration: declared / instantiated carriers ──────────

/// Build a WORKSPACE host carrying one `.svelte` component plus supporting VFS
/// files, mirroring the proven `svelte_exec_tests` harness so imported type
/// aliases / generics resolve through the real carrier path.
fn workspace_host_with_svelte(
    component_canonical: &str,
    component_source: &str,
    extra: &[(&str, &str)],
) -> (Arc<VerterHost>, CurrentHostStoreView) {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![verter_workspace::VfsProjectConfig {
            root: "/workspace".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ProjectMembership::MatchAll,
        }]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in extra {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    workspace.inject_file(component_canonical.into(), Arc::from(component_source));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
    ));
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let view = crate::typeinfo::current_store_view_for_query(&host).expect("current store view");
    (host, view)
}

#[test]
fn single_callable_arm_realizes_declared_and_instantiated_callbacks() {
    // Real carrier resolution: a declared alias (`FnAlias` → DeclRef) and a
    // generic instantiation (`GenFn<Row>` → InstantiationRef) BOTH realize to a
    // single callable through the composed realize_callable_member; ambiguous /
    // non-callable members refuse. Plus the strongest parity oracle against
    // callable_arm_from_raised on the materialized member value.
    let component = "/workspace/Callbacks.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { FnAlias, GenFn, Row } from './types';\n\
         interface Props {\n\
           onbare: (r: Row) => void;\n\
           ondeclared: FnAlias;\n\
           ongeneric: GenFn<Row>;\n\
           onnullish: ((r: Row) => void) | undefined;\n\
           onambiguous: ((a: Row) => void) | ((b: number) => void);\n\
           onnoncallable: 'a' | 'b';\n\
           label: string;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface Row { id: number }\n\
             export type FnAlias = (r: Row) => void;\n\
             export type GenFn<T> = (t: T) => void;\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();

    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // Declared alias + generic instantiation + bare + nullish realize to a
    // single Function callable.
    for name in ["onbare", "ondeclared", "ongeneric", "onnullish"] {
        let arm = CallableNodeView::new(&dispatch, member(name)).single_callable_arm(navigate());
        let arm = arm.unwrap_or_else(|| panic!("`{name}` realizes to a single callable arm"));
        assert!(
            matches!(
                node_data_for(dispatch.ctx, arm).as_deref(),
                Some(SemanticNodeData::Function { .. })
            ),
            "`{name}`'s callable arm is a Function node"
        );
    }

    // Ambiguous union of two distinct callables + non-callable members refuse.
    for name in ["onambiguous", "onnoncallable", "label"] {
        assert_eq!(
            CallableNodeView::new(&dispatch, member(name)).single_callable_arm(navigate()),
            None,
            "`{name}` refuses (ambiguous multi-callable / non-callable)"
        );
    }

    // PARITY ORACLE on a real nullish callback: the view's realized callable,
    // materialized once, equals callable_arm_from_raised on the materialized
    // (realized) member value — the exact legacy `callback_events` decision.
    let nullish = member("onnullish");
    let arm = CallableNodeView::new(&dispatch, nullish)
        .single_callable_arm(navigate())
        .expect("the nullish callback yields a callable arm");
    let view_mat = dispatch
        .materialize_output_type_expr_for_test(arm)
        .expect("the view node materializes");
    let TypeExpr::Function(view_func) = &view_mat else {
        panic!("the view node materializes to a Function, got {view_mat:?}");
    };
    let realized_member =
        realize_callable_member(&dispatch, nullish, navigate()).unwrap_or(nullish);
    let member_mat = dispatch
        .materialize_output_type_expr_for_test(realized_member)
        .expect("the member value materializes");
    let helper_func = callable_arm_from_raised(&member_mat)
        .expect("callable_arm_from_raised extracts a callable");
    assert_eq!(
        view_func.as_ref(),
        helper_func.as_ref(),
        "the view's node-domain callable matches callable_arm_from_raised's materialized callable"
    );
}

// ──────────── carrier-RESOLUTION: real DeclRef / InstantiationRef ──────────
//
// These exercise the demand-point structural-fact primitive over REAL carriers
// (a workspace `.svelte` + `.ts`). Each DISCRIMINATES against the pre-fix
// raw-node behaviour: pre-fix the view decided on the unresolved `DeclRef` /
// `InstantiationRef` and dropped the carrier-wrapped shape; post-fix it resolves
// through the shared primitive and surfaces the names / tuple / callable.

#[test]
fn event_names_resolves_declref_and_instantiationref_event_unions() {
    // `type Event = 'save' | 'cancel'` is a real `DeclRef` in the param
    // position; `GenEvent<'x' | 'y'>` is a real `InstantiationRef`. Pre-fix the
    // view saw the unresolved carrier and produced NO names; post-fix it
    // resolves the union and surfaces the literal names.
    let component = "/workspace/Events.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { Event, GenEvent } from './types';\n\
         interface Props {\n\
           onsave: (e: Event) => void;\n\
           ongen: (e: GenEvent<'x' | 'y'>) => void;\n\
           onplain: (e: string) => void;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export type Event = 'save' | 'cancel';\n\
             export type GenEvent<T> = T;\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // PRECONDITION (#4): assert the relevant RAW node IS the expected carrier
    // variant BEFORE the fixed reader resolves it, so a future lowering change
    // can't silently make these pass without exercising the carrier path. Here
    // the carrier sits in the FIRST-PARAM position of the realized callable.
    let first_param_raw = |name: &str| -> SemanticNodeId {
        CallableNodeView::new(&dispatch, member(name))
            .signature(navigate())
            .unwrap_or_else(|| panic!("`{name}` realizes to a signature"))
            .first_param()
            .unwrap_or_else(|| panic!("`{name}` signature has a first param"))
    };
    assert!(
        matches!(
            node_data_for(dispatch.ctx, first_param_raw("onsave")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `onsave` raw first-param node is a `DeclRef` carrier (the `Event` alias) before resolution"
    );
    assert!(
        matches!(
            node_data_for(dispatch.ctx, first_param_raw("ongen")).as_deref(),
            Some(SemanticNodeData::InstantiationRef { .. })
        ),
        "the `ongen` raw first-param node is an `InstantiationRef` carrier before resolution"
    );

    // DeclRef-aliased event-name union → its names.
    assert_eq!(
        CallableNodeView::new(&dispatch, member("onsave")).event_names(navigate()),
        Some(vec![Arc::<str>::from("save"), Arc::<str>::from("cancel")]),
        "a `DeclRef`-aliased event-name union resolves to its names"
    );
    // InstantiationRef-instantiated event-name union → its names.
    assert_eq!(
        CallableNodeView::new(&dispatch, member("ongen")).event_names(navigate()),
        Some(vec![Arc::<str>::from("x"), Arc::<str>::from("y")]),
        "a generic-instantiated (`InstantiationRef`) event-name union resolves to its names"
    );
    // A non-literal first param surfaces no names (fail-closed).
    assert_eq!(
        CallableNodeView::new(&dispatch, member("onplain")).event_names(navigate()),
        None,
        "a non-literal (`string`) first param yields no event names"
    );
}

#[test]
fn positional_params_expands_declref_and_instantiationref_rest_tuples() {
    // `type Args = [item: Item, index: number]` is a real `DeclRef` rest-tuple;
    // `GenTuple<Item, number>` is a real `InstantiationRef` rest-tuple. Pre-fix
    // the view saw the unresolved carrier and could NOT expand it; post-fix it
    // resolves the tuple and expands per element.
    let component = "/workspace/Rest.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { Item, Args, GenTuple } from './types';\n\
         interface Props {\n\
           onrest: (...args: Args) => void;\n\
           ongentuple: (...args: GenTuple<Item, number>) => void;\n\
           onopen: (...args: string[]) => void;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface Item { name: string }\n\
             export type Args = [item: Item, index: number];\n\
             export type GenTuple<A, B> = [first: A, second: B];\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // PRECONDITION (#4): the rest-param carrier IS the expected variant BEFORE
    // `positional_params_expanded` resolves it to a `Tuple`. The rest param is
    // the signature's first (and only) param, so `first_param()` is its raw
    // carrier type.
    let rest_param_raw = |name: &str| -> SemanticNodeId {
        CallableNodeView::new(&dispatch, member(name))
            .signature(navigate())
            .unwrap_or_else(|| panic!("`{name}` realizes to a signature"))
            .first_param()
            .unwrap_or_else(|| panic!("`{name}` signature has a (rest) first param"))
    };
    assert!(
        matches!(
            node_data_for(dispatch.ctx, rest_param_raw("onrest")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `onrest` raw rest-param node is a `DeclRef` carrier (the `Args` alias) before resolution"
    );
    assert!(
        matches!(
            node_data_for(dispatch.ctx, rest_param_raw("ongentuple")).as_deref(),
            Some(SemanticNodeData::InstantiationRef { .. })
        ),
        "the `ongentuple` raw rest-param node is an `InstantiationRef` carrier before resolution"
    );

    // DeclRef rest-tuple → expanded element labels.
    let rest = CallableNodeView::new(&dispatch, member("onrest"))
        .positional_params(navigate())
        .expect("the `onrest` callable yields positional params");
    let rest_labels: Vec<Option<&str>> = rest.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        rest_labels,
        vec![Some("item"), Some("index")],
        "a `DeclRef` rest-tuple expands element-wise with its labels"
    );

    // InstantiationRef rest-tuple → expanded element labels.
    let gen = CallableNodeView::new(&dispatch, member("ongentuple"))
        .positional_params(navigate())
        .expect("the `ongentuple` callable yields positional params");
    let gen_labels: Vec<Option<&str>> = gen.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        gen_labels,
        vec![Some("first"), Some("second")],
        "a generic-instantiated (`InstantiationRef`) rest-tuple expands element-wise"
    );

    // A non-tuple rest (`string[]`) carries no enumerable positional bindings.
    let open = CallableNodeView::new(&dispatch, member("onopen"))
        .positional_params(navigate())
        .expect("the `onopen` callable yields (empty) positional params");
    assert!(
        open.is_empty(),
        "a non-tuple rest param (`string[]`) contributes no positional entries"
    );
}

#[test]
fn single_callable_arm_resolves_carrier_wrapped_nullish_callable() {
    // `type MaybeFn = ((r: Row) => void) | undefined` referenced as `onmaybe:
    // MaybeFn` is a real `DeclRef` whose body is a nullish union. Pre-fix the
    // view's `_` arm called `realize_callable_member(DeclRef(MaybeFn))`, whose
    // strict whole-composite rule FAILS on the `undefined` arm → `None`.
    // Post-fix the view normalizes the `DeclRef` to `Union(Function, undefined)`
    // FIRST, strips `undefined`, then realizes the surviving `Function`.
    let component = "/workspace/Maybe.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { Row, MaybeFn } from './types';\n\
         interface Props {\n\
           onmaybe: MaybeFn;\n\
           label: string;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface Row { id: number }\n\
             export type MaybeFn = ((r: Row) => void) | undefined;\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // PRECONDITION (#4): the `onmaybe` member RAW node IS a `DeclRef` carrier
    // (the `MaybeFn` alias) BEFORE `single_callable_arm` resolves it — so the
    // test genuinely exercises the carrier-resolution path, not a pre-resolved
    // union.
    assert!(
        matches!(
            node_data_for(dispatch.ctx, member("onmaybe")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `onmaybe` member raw node is a `DeclRef` carrier before resolution"
    );

    // The carrier-wrapped nullish callback resolves to a single callable arm.
    let arm = CallableNodeView::new(&dispatch, member("onmaybe"))
        .single_callable_arm(navigate())
        .expect("a `DeclRef`-wrapped `Fn | undefined` resolves to a single callable arm");
    assert!(
        matches!(
            node_data_for(dispatch.ctx, arm).as_deref(),
            Some(SemanticNodeData::Function { .. })
        ),
        "the resolved callable arm is a `Function` node"
    );

    // PARITY ORACLE: the view's node-domain callable, materialized once, equals
    // `callable_arm_from_raised` on the materialized NORMALIZED member value.
    let arm_mat = dispatch
        .materialize_output_type_expr_for_test(arm)
        .expect("the view node materializes");
    let TypeExpr::Function(view_func) = &arm_mat else {
        panic!("the view node materializes to a Function, got {arm_mat:?}");
    };
    let normalized =
        dispatch.normalize_node_for_structural_fact_demand(member("onmaybe"), navigate());
    let mem_mat = dispatch
        .materialize_output_type_expr_for_test(normalized)
        .expect("the normalized member materializes");
    let helper_func = callable_arm_from_raised(&mem_mat)
        .expect("callable_arm_from_raised extracts the single callable");
    assert_eq!(
        view_func.as_ref(),
        helper_func.as_ref(),
        "the view's callable matches callable_arm_from_raised on the resolved member"
    );

    // A plain non-callable member still refuses.
    assert_eq!(
        CallableNodeView::new(&dispatch, member("label")).single_callable_arm(navigate()),
        None,
        "a non-callable (`string`) member refuses"
    );
}

#[test]
fn slot_param_and_return_resolves_aliased_and_nullable_slot_arms() {
    // `nullableslot: SlotAlias | undefined` DISCRIMINATES: pre-fix
    // `realize_callable_member` on the whole root failed on the `undefined` arm
    // → `None`; post-fix the view strips `undefined` and combines the surviving
    // arm. `slotcombo: SlotAlias | GenSlot<SlotProps>` exercises a 2-arm combine
    // over a `DeclRef` + an `InstantiationRef` callable.
    let component = "/workspace/Slots.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { SlotAlias, GenSlot, SlotProps } from './types';\n\
         interface Props {\n\
           nullableslot: SlotAlias | undefined;\n\
           slotcombo: SlotAlias | GenSlot<SlotProps>;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface SlotProps { id: number }\n\
             export type SlotAlias = (props: { a: number }) => void;\n\
             export type GenSlot<P> = (props: P) => void;\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // Nullable single slot: the `undefined` arm is stripped, the surviving
    // callable supplies the first-param binding.
    let nullable = CallableNodeView::new(&dispatch, member("nullableslot"))
        .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow())
        .expect("`SlotAlias | undefined` strips the nullish arm and yields parts");
    assert!(
        nullable.first_param.is_some(),
        "the surviving callable supplies a first-param binding"
    );
    // Same nullable single slot is a single callable for `single_callable_arm`.
    assert!(
        CallableNodeView::new(&dispatch, member("nullableslot"))
            .single_callable_arm(navigate())
            .is_some(),
        "`SlotAlias | undefined` is a single callable after stripping the nullish arm"
    );

    // Two distinct callable arms (DeclRef + InstantiationRef): a slot combine
    // yields a binding across both arms; `single_callable_arm` refuses
    // (ambiguous — two distinct callables).
    let combo = CallableNodeView::new(&dispatch, member("slotcombo"))
        .slot_param_and_return_by_arm(ArmCombineNode::Intersection, shallow())
        .expect("`SlotAlias | GenSlot<SlotProps>` yields a 2-arm slot combine");
    let combo_first = combo
        .first_param
        .expect("both callable arms supply a first param → intersected binding");
    let combo_mat = dispatch
        .materialize_output_type_expr_for_test(combo_first)
        .expect("the combined first param materializes");
    assert!(
        matches!(&combo_mat, TypeExpr::Intersection(arms) if arms.len() == 2),
        "the combined first param intersects both arms, got {combo_mat:?}"
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, member("slotcombo")).single_callable_arm(navigate()),
        None,
        "two distinct callable arms are ambiguous for single_callable_arm"
    );
}

#[test]
fn single_callable_arm_strips_nested_nullish_union() {
    // claude [P3-1]: `Union([Union([f, undefined]), undefined])`. Pre-fix the
    // outer arm `Union([f, undefined])` was handed to
    // `realize_callable_member`, whose strict composite rule fails on the inner
    // `undefined` → the whole classification returned `None`. Post-fix the view
    // RECURSES through the normalized nested composite, stripping the inner
    // `undefined` too, and surfaces `f`.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let row = prim(&graph, PrimitiveKind::Number);
    let f = function(&graph, vec![param(Some("r"), row, false, false)], void);
    let undefined = prim(&graph, PrimitiveKind::Undefined);
    let inner = union(&graph, vec![f, undefined]);
    let outer = union(&graph, vec![inner, undefined]);

    let view = CallableNodeView::new(&dispatch, outer);
    assert_eq!(
        view.single_callable_arm(navigate()),
        Some(f),
        "a nested nullish union surfaces the inner callable (recursive strip)"
    );
}

#[test]
fn first_param_object_surface_keeps_root_carrier_shaped() {
    // SCOPING RULE (#4): `first_param_object_surface` must NOT carrier-resolve
    // the first-param root — it stays a `DeclRef` carrier reaching the shallow
    // projector, preserving the `AppProps['avatar']` symbolic indexed-access
    // policy. We assert the signature's first-param root is a `DeclRef` (carrier,
    // NOT resolved to an `Object`) and the shallow surface still projects the
    // one-level members; the surface is context-invariant (always one-level
    // Shallow).
    let component = "/workspace/Props.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { AppProps } from './types';\n\
         interface Props {\n\
           onprops: (props: AppProps) => void;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export interface Avatar { url: string }\n\
             export interface AppProps { avatar: Avatar; label: string }\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let onprops = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "onprops")
        .expect("the `onprops` member is present")
        .value;

    // The first-param root reaching the projector is a carrier (`DeclRef`),
    // NOT a resolved `Object` — the scoping rule keeps it carrier-shaped.
    let first = CallableNodeView::new(&dispatch, onprops)
        .signature(navigate())
        .expect("onprops realizes to a signature")
        .first_param()
        .expect("the signature has a first param");
    assert!(
        matches!(
            node_data_for(dispatch.ctx, first).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the first-param root stays a `DeclRef` carrier (not resolved to an Object)"
    );

    // The shallow surface still projects the one-level members, and is
    // context-invariant (Shallow regardless of the caller's mode).
    let names = |context| -> Vec<String> {
        let mut n: Vec<String> = CallableNodeView::new(&dispatch, onprops)
            .first_param_object_surface(&ctx, context)
            .expect("the first-param object projects a one-level surface")
            .members
            .iter()
            .map(|m| m.name.to_string())
            .collect();
        n.sort();
        n
    };
    assert_eq!(
        names(shallow()),
        vec!["avatar".to_string(), "label".to_string()],
        "the shallow surface carries the one-level members"
    );
    assert_eq!(
        names(navigate()),
        names(shallow()),
        "the surface is one-level Shallow regardless of the caller's context"
    );
    assert_eq!(
        names(ProjectionReductionContext::published(
            ProjectionMode::Expanded
        )),
        names(shallow()),
        "an Expanded caller context does NOT expand the (always-Shallow) surface"
    );
}

// ───────── normalize_node_for_structural_fact_demand: direct contract ─────────
//
// Direct boundedness / fail-closed tests for the shared demand primitive
// (`normalize_node_for_structural_fact_demand`) the view composes. Each asserts
// BOUNDED termination + the contract: a resolvable chain materialises, an
// unresolvable / circular / over-deep carrier carrier-stops fail-closed
// (returns a carrier / opaque, NEVER panics, NEVER fabricates a concrete type).
// These also pin finding #1 — the same residual-carrier resolution feeds the
// view-side recursions whose fuses this cycle hardens.

/// Workspace host carrying the carrier-shape fixtures used by the primitive
/// contract tests: a 2-hop `DeclRef→DeclRef` chain that terminates at a string
/// literal, a mutual-recursion cycle, an identity generic, and a `T0..T80`
/// alias chain LONGER than `STRUCTURAL_FACT_DEMAND_FUSE` (64).
fn primitive_carrier_host() -> (Arc<VerterHost>, CurrentHostStoreView) {
    let mut deep = String::new();
    for i in 0..80u32 {
        deep.push_str(&format!("export type T{i} = T{};\n", i + 1));
    }
    deep.push_str("export type T80 = 'leaf';\n");
    let types = format!(
        "export type DeclChainA = DeclChainB;\n\
         export type DeclChainB = 'leaf';\n\
         export type MutA = MutB;\n\
         export type MutB = MutA;\n\
         export type GenIdent<T> = T;\n\
         {deep}"
    );
    let component = "/workspace/Carriers.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { DeclChainA, MutA, GenIdent, T0 } from './types';\n\
         interface Props {\n\
           chain: DeclChainA;\n\
           mutual: MutA;\n\
           geninst: GenIdent<DeclChainA>;\n\
           deepchain: T0;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    workspace_host_with_svelte(
        component,
        source,
        &[("/workspace/types.ts", types.as_str())],
    )
}

#[test]
fn normalize_node_for_fact_demand_resolves_carrier_chains() {
    // BOUNDED RESOLUTION: a `DeclRef→DeclRef` chain (`DeclChainA → DeclChainB →
    // 'leaf'`) and an `InstantiationRef` whose body is itself a carrier
    // (`GenIdent<DeclChainA>` → `DeclChainA` → the same chain) both materialise
    // through the shared primitive to the terminal `Literal('leaf')`. A
    // non-resolving primitive would return the input carrier instead.
    let (host, view) = primitive_carrier_host();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, "/workspace/Carriers.svelte")
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface = navigate_param_to_object_surface(&ctx, "/workspace/Carriers.svelte", props_type)
        .expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // Precondition: the raw nodes are genuinely carriers.
    assert!(
        matches!(
            node_data_for(dispatch.ctx, member("chain")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `chain` member raw node is a `DeclRef` carrier"
    );
    assert!(
        matches!(
            node_data_for(dispatch.ctx, member("geninst")).as_deref(),
            Some(SemanticNodeData::InstantiationRef { .. })
        ),
        "the `geninst` member raw node is an `InstantiationRef` carrier"
    );

    for name in ["chain", "geninst"] {
        let resolved = dispatch.normalize_node_for_structural_fact_demand(member(name), navigate());
        assert!(
            matches!(
                node_data_for(dispatch.ctx, resolved).as_deref(),
                Some(SemanticNodeData::Literal(LiteralValue::String(s))) if s == "leaf"
            ),
            "`{name}` resolves through its carrier chain to the terminal `'leaf'` literal, got {:?}",
            node_data_for(dispatch.ctx, resolved).as_deref()
        );
    }
}

#[test]
fn normalize_node_for_fact_demand_circular_and_deep_fail_closed() {
    // FAIL-CLOSED boundedness: a mutual-recursion cycle (`MutA = MutB; MutB =
    // MutA`) terminates via the primitive's `visited` set and carrier-stops at a
    // `DeclRef` (never a fabricated concrete type, never a hang). A `T0..T80`
    // alias chain LONGER than `STRUCTURAL_FACT_DEMAND_FUSE` (64) trips the step
    // fuse and carrier-stops at an INTERMEDIATE `DeclRef` — it does NOT reach the
    // `'leaf'` terminal, proving the fuse fired rather than running to completion.
    let (host, view) = primitive_carrier_host();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, "/workspace/Carriers.svelte")
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface = navigate_param_to_object_surface(&ctx, "/workspace/Carriers.svelte", props_type)
        .expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    // PRECONDITION (#4): the raw nodes are genuinely `DeclRef` carriers BEFORE
    // the primitive resolves them, so a future lowering change can't make these
    // boundedness assertions pass for the wrong reason (a pre-resolved node would
    // never exercise the cycle / fuse paths). `mutual: MutA` and `deepchain: T0`
    // are both alias `DeclRef`s.
    assert!(
        matches!(
            node_data_for(dispatch.ctx, member("mutual")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `mutual` member raw node is a `DeclRef` carrier (the `MutA` alias) before resolution"
    );
    assert!(
        matches!(
            node_data_for(dispatch.ctx, member("deepchain")).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "the `deepchain` member raw node is a `DeclRef` carrier (the `T0` alias) before resolution"
    );

    // Mutual recursion → carrier-stop (a `DeclRef`, NOT a fabricated concrete
    // Function / Object / Literal).
    let mutual = dispatch.normalize_node_for_structural_fact_demand(member("mutual"), navigate());
    assert!(
        matches!(
            node_data_for(dispatch.ctx, mutual).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "a mutual-recursion cycle terminates fail-closed at a `DeclRef` carrier, got {:?}",
        node_data_for(dispatch.ctx, mutual).as_deref()
    );

    // Over-fuse alias chain → carrier-stop at an intermediate `DeclRef`, NOT the
    // `'leaf'` literal terminal (the discriminator: a short chain WOULD reach the
    // leaf — see `normalize_node_for_fact_demand_resolves_carrier_chains`).
    let deep = dispatch.normalize_node_for_structural_fact_demand(member("deepchain"), navigate());
    assert!(
        matches!(
            node_data_for(dispatch.ctx, deep).as_deref(),
            Some(SemanticNodeData::DeclRef { .. })
        ),
        "a >FUSE-deep alias chain carrier-stops at an intermediate `DeclRef`, got {:?}",
        node_data_for(dispatch.ctx, deep).as_deref()
    );
    assert!(
        !matches!(
            node_data_for(dispatch.ctx, deep).as_deref(),
            Some(SemanticNodeData::Literal(_))
        ),
        "the step fuse prevented the deep chain from reaching the `'leaf'` literal terminal"
    );
}

#[test]
fn normalize_node_for_fact_demand_unresolvable_declref_fails_closed() {
    // ERROR-path fail-closed: a `DeclRef` to a non-existent declaration (a
    // synthetic identity whose `canonical_id` names no workspace file) MISSES the
    // `ResolveDecl` query; the primitive breaks fail-closed, returning the input
    // carrier (or an `Opaque` miss) — never a panic, never a fabricated concrete
    // type. Standalone host: no workspace file backs the synthetic identity.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let fake = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity::synthetic("Nonexistent"),
    });
    let resolved = dispatch.normalize_node_for_structural_fact_demand(fake, navigate());
    assert!(
        matches!(
            node_data_for(dispatch.ctx, resolved).as_deref(),
            Some(SemanticNodeData::DeclRef { .. }) | Some(SemanticNodeData::Opaque(_))
        ),
        "an unresolvable `DeclRef` fails closed to a carrier / opaque (never a fabricated type), got {:?}",
        node_data_for(dispatch.ctx, resolved).as_deref()
    );
}

#[test]
fn event_names_direct_self_reference_terminates_complete() {
    // BOUNDEDNESS via the SHARED RESOLVER's recursive-type carrier-stop (NOT the
    // view's visited set): `type SelfUnion = 'x' | SelfUnion` referenced as the
    // first param of `onself`. `normalized_fact_node` resolves the DIRECT self
    // reference to `Union(Literal('x'), Opaque(RecursiveRef { name: "SelfUnion" }))`
    // — the resolver carrier-stops the self-edge to an `Opaque(RecursiveRef)`,
    // which is a non-literal/non-union LEAF that `collect_string_literal_names`
    // treats as a COMPLETE branch contributing no name. So the enumeration
    // TERMINATES cleanly with the COMPLETE literal set `["x"]` (the recursive arm
    // adds no new literal) — `Some(["x"])`, never a hang, never a partial. This
    // is the resolver's own bound, distinct from the mutual-cycle case below
    // which DOES reach the view's active-path visited set.
    //
    // DISCRIMINATING: if `collect_string_literal_names` treated the
    // `Opaque(RecursiveRef)` leaf as a FAILURE (rather than a complete no-name
    // branch), `event_names` would wrongly return `None` here.
    let component = "/workspace/SelfUnion.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { SelfUnion } from './types';\n\
         interface Props {\n\
           onself: (e: SelfUnion) => void;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            "export type SelfUnion = 'x' | SelfUnion;\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let onself = surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == "onself")
        .expect("the `onself` member is present")
        .value;

    assert_eq!(
        CallableNodeView::new(&dispatch, onself).event_names(navigate()),
        Some(vec![Arc::<str>::from("x")]),
        "a direct self-referential union is bounded by the resolver's `Opaque(RecursiveRef)` carrier-stop and enumerates the COMPLETE literal set `[\"x\"]`"
    );
}

#[test]
fn event_names_mutual_cycle_fails_whole_via_visited_set() {
    // FAIL-CLOSED-WHOLE on a GENUINE cycle the view's visited set must catch
    // (real carrier): an INDIRECT/mutual cycle `type A = 'a' | B; type B = 'b' | A`
    // referenced as `onmut: (e: A) => void`. Unlike a DIRECT self reference (which
    // the resolver carrier-stops to `Opaque(RecursiveRef)` — see the test above),
    // the resolver RE-YIELDS the SAME hash-consed union node on the back-edge:
    // `normalize` walks `A -> Union('a', B-ref) -> Union('b', A-ref) ->
    // Union('a', B-ref)` and the third hop REVISITS the first union node. The
    // ACTIVE-PATH visited set in `collect_string_literal_names` (keyed by
    // normalized id) fires on that revisit and fails the WHOLE enumeration, so
    // `event_names` returns `None` — NOT a partial `Some(["a", "b"])` (a partial
    // enumeration presented as complete is a poisoned fact), and NOT a stack
    // overflow.
    //
    // FLIP: revert `event_names` to ignore the collect-status (the pre-fix
    // partial-return keyed only on `names.is_empty()`); the visited set still
    // returns failure but `event_names` would surface the already-collected
    // `Some(["a", "b"])`, FAILING this `None` assertion. So this discriminates the
    // fail-closed-whole behavior layered on the visited-set cycle break.
    let component = "/workspace/Mutual.svelte";
    let source = "<script lang=\"ts\">\n\
         import type { A, Ack } from './types';\n\
         interface Props {\n\
           onmut: (e: A) => void;\n\
           onack: (e: Ack) => void;\n\
         }\n\
         let props: Props = $props();\n\
         void props;\n\
         </script>\n\
         <div />";
    let (host, view) = workspace_host_with_svelte(
        component,
        source,
        &[(
            "/workspace/types.ts",
            // `A`/`B` are the mutual cycle. `Ack`/`AckTail` are an ACYCLIC 2-hop
            // carrier chain of the SAME shape and depth (a literal + a 1-hop
            // alias ref) — the visited-set proof control below.
            "export type A = 'a' | B;\n\
             export type B = 'b' | A;\n\
             export type Ack = 'ack0' | AckTail;\n\
             export type AckTail = 'ack1' | 'ack2';\n",
        )],
    );
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::from_current(&host, &view, overlay);
    let facts = host
        .resolve_svelte_script_facts_with_ctx(&ctx, component)
        .expect("svelte facts");
    let props_type = facts.props_type.as_ref().expect("props type");
    let surface =
        navigate_param_to_object_surface(&ctx, component, props_type).expect("props surface");
    let dispatch = ctx.dispatch();
    let member = |name: &str| -> SemanticNodeId {
        surface
            .members
            .iter()
            .find(|m| m.name.as_ref() == name)
            .unwrap_or_else(|| panic!("the `{name}` member is present"))
            .value
    };

    assert_eq!(
        CallableNodeView::new(&dispatch, member("onmut")).event_names(navigate()),
        None,
        "a genuine mutual cycle (`A` references `B`, `B` references `A`) re-yields the same union node on the back-edge → the active-path visited set fails the WHOLE enumeration (not a partial `Some([\"a\", \"b\"])`, not a stack overflow)"
    );

    // [P3] SAME-SHAPE ACYCLIC CONTROL (NOT a visited-vs-fuse discriminator): the
    // SAME 2-hop carrier shape (`Ack = 'ack0' | AckTail; AckTail = 'ack1' |
    // 'ack2'`) but ACYCLIC. It enumerates COMPLETELY →
    // `Some(["ack0", "ack1", "ack2"])`, which shows the mutual-cycle `None` above
    // is CYCLE-SPECIFIC — NOT a generic failure to read a 2-hop carrier chain.
    //
    // This control does NOT isolate "the visited set fired" from "the depth fuse
    // fired": both the 2-hop cycle (`onmut`) and this 2-hop acyclic chain (`onack`)
    // sit FAR below `CALLABLE_VIEW_DEPTH_FUSE` (32), and a hypothetical
    // depth-fuse-ONLY impl (no visited set) would ALSO yield `onmut = None` (the
    // cycle eventually trips the fuse@32) and `onack = Some([...])` (terminates@~2)
    // — so this pair passes under EITHER mechanism. The ACTIVE-PATH (removed-on-
    // unwind) visited-set discriminator is `event_names_finite_deep_dag_union_
    // enumerates_completely`: a deep-but-FINITE union (leaf near the fuse) with a
    // SHARED subtree reached via two sibling branches enumerates the FULL name
    // set, which succeeds ONLY because the active-path visited set is REMOVED on
    // unwind — a GLOBAL never-removed visited set would wrongly fail it, and THAT
    // is the property that test pins. It does NOT isolate visited-set-vs-depth-
    // fuse either: a depth-fuse-only impl (no visited set) would ALSO pass it (the
    // sub-fuse chain is never truncated). It pins the active-path removed-on-
    // unwind semantics this one cannot.
    assert_eq!(
        CallableNodeView::new(&dispatch, member("onack")).event_names(navigate()),
        Some(vec![
            Arc::<str>::from("ack0"),
            Arc::<str>::from("ack1"),
            Arc::<str>::from("ack2")
        ]),
        "an ACYCLIC 2-hop carrier chain of the same shape enumerates completely — showing the mutual-cycle `None` is CYCLE-SPECIFIC (not a generic 2-hop-chain failure); the active-path (removed-on-unwind) visited-set discriminator is `event_names_finite_deep_dag_union_enumerates_completely`"
    );
}

#[test]
fn event_names_over_deep_nested_union_trips_collect_fuse() {
    // FAIL-CLOSED-WHOLE (FLIP test): a union with ONE immediately-reachable
    // literal (`present`) and ONE literal buried deeper than the fuse. The depth
    // fuse trips on the buried arm; because collection FAILS-CLOSED-WHOLE the
    // whole enumeration fails and `event_names` returns `None` — it does NOT
    // return `Some(["present"])` (a partial enumeration presented as complete is
    // a poisoned fact). This FLIPS: revert `event_names` to ignore the
    // collect-status (the pre-fix partial-return that keyed only on
    // `names.is_empty()`) and it yields `Some(["present"])`, failing this `None`
    // assertion.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let present = string_literal(&graph, "present");
    // Bury a SECOND literal under more than the fuse's worth of nested unions
    // (each a distinct interned single-arm union, so the recursion truly
    // descends one level per hop and trips the fuse).
    let mut buried = string_literal(&graph, "deep");
    for _ in 0..(super::CALLABLE_VIEW_DEPTH_FUSE + 5) {
        buried = union(&graph, vec![buried]);
    }
    // `present` is FIRST so it is collected BEFORE the buried arm trips the fuse
    // — the partial set is NON-EMPTY, which is exactly what fail-closed-whole
    // must still discard.
    let names_union = union(&graph, vec![present, buried]);
    let f = function(
        &graph,
        vec![param(Some("e"), names_union, false, false)],
        void,
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, f).event_names(navigate()),
        None,
        "a fuse trip on ANY arm fails the WHOLE enumeration — never a partial `Some([\"present\"])`"
    );

    // DISCRIMINATING control: the same `present`-style literal nested only a FEW
    // levels (well under the fuse), with NO over-deep sibling, IS surfaced —
    // proving the `None` above is the fuse boundary, not a blanket failure to
    // read nested unions.
    let mut shallow_nest = string_literal(&graph, "shallow");
    for _ in 0..3 {
        shallow_nest = union(&graph, vec![shallow_nest]);
    }
    let f2 = function(
        &graph,
        vec![param(Some("e"), shallow_nest, false, false)],
        void,
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, f2).event_names(navigate()),
        Some(vec![Arc::<str>::from("shallow")]),
        "a shallowly-nested union (under the fuse) still surfaces the literal"
    );
}

#[test]
fn event_names_finite_deep_dag_union_enumerates_completely() {
    // CONTROL (#2 fail-closed-whole must NOT over-reject): a legitimate FINITE
    // union — DEEP (a chain nested nearly to the fuse) AND with a SHARED subtree
    // reached via two distinct sibling branches (a DAG, not a cycle) — enumerates
    // COMPLETELY. This discriminates BOTH boundedness hardenings:
    //   (a) the depth fuse does NOT truncate a finite-but-deep chain (< fuse), and
    //   (b) the ACTIVE-PATH visited set is removed on unwind, so a shared node
    //       reached via two branches is NOT a false cycle — a GLOBAL (never-removed)
    //       visited set would WRONGLY fail here (→ `None` → the `.expect` panics).
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    // A shared 2-literal union, reached via TWO sibling branches below (a DAG).
    let s1 = string_literal(&graph, "s1");
    let s2 = string_literal(&graph, "s2");
    let shared = union(&graph, vec![s1, s2]);
    // A deep-but-FINITE chain ending in a literal, well under the fuse (the leaf
    // lands at depth ~26 < 32).
    let mut chain = string_literal(&graph, "bottom");
    for _ in 0..24 {
        chain = union(&graph, vec![chain]);
    }
    // `shared` appears via the left AND the right branch (DAG); `chain` is the
    // deep-but-finite arm.
    let left = union(&graph, vec![shared, chain]);
    let right = union(&graph, vec![shared]);
    let outer = union(&graph, vec![left, right]);
    let f = function(&graph, vec![param(Some("e"), outer, false, false)], void);

    let names = CallableNodeView::new(&dispatch, f)
        .event_names(navigate())
        .expect(
            "a finite deep DAG union enumerates completely (no false cycle, no fuse truncation)",
        );
    // Dedup-insensitive: the shared subtree legitimately yields its names twice
    // (dedup is optional per the fix brief). Assert the SET is exactly complete.
    let mut set: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    set.sort_unstable();
    set.dedup();
    assert_eq!(
        set,
        vec!["bottom", "s1", "s2"],
        "the finite deep DAG union surfaces EVERY name (depth fuse doesn't truncate the deep chain; the active-path visited set doesn't false-trip on the shared subtree)"
    );
}

#[test]
fn event_names_residual_carrier_arm_fails_whole_not_partial() {
    // [P1] residual-carrier partial poison: a union `'present' | <residual
    // carrier>` whose second arm is a `DeclRef` the demand primitive CANNOT
    // resolve (a synthetic identity naming no workspace file — the same
    // unresolvable carrier `normalize_node_for_fact_demand_unresolvable_declref_
    // fails_closed` exercises). Pre-fix the blanket `_ => true` arm treated the
    // unresolved carrier as a COMPLETE no-name leaf, so `'present'` was pushed,
    // the carrier arm returned `true`, and `event_names` surfaced
    // `Some(["present"])` — a PARTIAL enumeration presented as COMPLETE (the
    // exact poison cycle 3 closed for depth/cycle, here reached via a residual
    // carrier that carrier-stopped on its OWN fuse / miss). Post-fix the residual
    // carrier fails-closed (`false`) → the whole union fails → `event_names`
    // yields `None`.
    //
    // FLIP: revert the residual-carrier arm to `_ => true` and the `DeclRef` arm
    // returns `true` → the union enumerates `["present"]` as complete →
    // `Some(["present"])`, FAILING this `None` assertion. So this discriminates
    // the residual-carrier fail-closed behaviour specifically (NOT the depth /
    // cycle paths the sibling fail-closed tests cover).
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let present = string_literal(&graph, "present");
    // A `DeclRef` to a non-existent declaration: the demand primitive misses the
    // `ResolveDecl` query and carrier-stops, leaving a residual carrier.
    let unresolvable = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity::synthetic("Nonexistent"),
    });

    // PRECONDITION (#4): the second arm genuinely normalizes to an UNRESOLVABLE
    // RESIDUAL CARRIER (a `DeclRef` / a non-`RecursiveRef` `Opaque` miss), NOT a
    // literal — so the test exercises the residual-carrier fail-closed arm, not a
    // pre-resolved literal that would pass regardless of the fix. The precondition
    // EXCLUDES `Opaque(RecursiveRef)`: that carrier routes to the `true` carve-out
    // (a decided recursion stop), which would yield `Some` and invalidate this
    // test's `None` premise. (Excluding it is unreachable in practice — a
    // synthetic miss yields `Opaque(Miss)` / a residual `DeclRef` — but pinned so
    // the precondition EXACTLY characterizes "unresolvable residual".)
    let normalized_arm =
        dispatch.normalize_node_for_structural_fact_demand(unresolvable, navigate());
    let arm_is_unresolvable_residual = match node_data_for(dispatch.ctx, normalized_arm).as_deref()
    {
        Some(SemanticNodeData::DeclRef { .. }) => true,
        Some(SemanticNodeData::Opaque(err)) => !matches!(err, QueryError::RecursiveRef { .. }),
        _ => false,
    };
    assert!(
        arm_is_unresolvable_residual,
        "precondition: the unresolvable `DeclRef` arm carrier-stops to an UNRESOLVABLE residual (DeclRef / non-RecursiveRef Opaque miss), not a literal and not the RecursiveRef carve-out, got {:?}",
        node_data_for(dispatch.ctx, normalized_arm).as_deref()
    );

    let names_union = union(&graph, vec![present, unresolvable]);
    let f = function(
        &graph,
        vec![param(Some("e"), names_union, false, false)],
        void,
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, f).event_names(navigate()),
        None,
        "`'present' | <unresolvable residual carrier>` fails the WHOLE enumeration (the carrier could hide a literal) — NEVER a partial `Some([\"present\"])`"
    );
}

#[test]
fn event_names_concrete_non_literal_arm_is_skipped_not_failed() {
    // CONTROL against over-failing: a mixed union `'a' | number` — the `number`
    // arm is a CONCRETE non-literal that is definitively NOT (and cannot hide) a
    // string literal, so it is SKIPPED (complete-no-name), NOT fail-closed.
    // `event_names` surfaces `["a"]`. This guards the residual-carrier fix from
    // OVER-rejecting: a concrete `Primitive` arm must stay `true` (the `build.rs`
    // `_ => {}` precedent), so only genuinely-unresolved carriers fail closed.
    //
    // DISCRIMINATING: if the fix wrongly classified `Primitive` as fail-closed,
    // this would return `None` and the assertion would fail.
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let a = string_literal(&graph, "a");
    let number = prim(&graph, PrimitiveKind::Number);
    let names_union = union(&graph, vec![a, number]);
    let f = function(
        &graph,
        vec![param(Some("e"), names_union, false, false)],
        void,
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, f).event_names(navigate()),
        Some(vec![Arc::<str>::from("a")]),
        "a concrete non-literal arm (`number`) is SKIPPED (complete-no-name), not fail-closed — `'a' | number` yields `[\"a\"]`"
    );
}

#[test]
fn event_names_constructor_type_arm_is_skipped_not_failed() {
    // [P2] CONTROL against over-failing on the `ConstructorType` sibling of
    // `Function`: a mixed first-param union `'a' | (new () => X)` — the
    // `ConstructorType` arm (`new () => X`) is a CONCRETE callable shape that,
    // exactly like `Function`, can neither BE nor HIDE a string-literal event
    // name, so it is SKIPPED (complete-no-name), NOT fail-closed. `event_names`
    // surfaces `["a"]`.
    //
    // FLIP (the cycle-4 partition-completeness gap both codex legs caught):
    // pre-fix `ConstructorType` fell into the `_ => false` fail-closed bucket
    // (only its sibling `Function` was in the `true`/skip set), so the
    // `new () => X` arm returned `false`, the whole union fail-closed, and
    // `event_names` wrongly returned `None` instead of `Some(["a"])` — an
    // over-fail dropping a legitimate event name. This test is RED pre-fix
    // (`None`) and GREEN post-fix (`Some(["a"])`).
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let void = prim(&graph, PrimitiveKind::Void);
    let a = string_literal(&graph, "a");
    // `new () => X` — a constructor-type whose construct signature is a
    // `Function` node. Architecture-equivalent to `Function` for the event-name
    // collector (it can neither BE nor HIDE a string-literal event name).
    let x = prim(&graph, PrimitiveKind::Object);
    let ctor_signature = function(&graph, vec![], x);
    let ctor = graph.intern_node(SemanticNodeData::ConstructorType {
        signature: ctor_signature,
    });
    let names_union = union(&graph, vec![a, ctor]);
    let f = function(
        &graph,
        vec![param(Some("e"), names_union, false, false)],
        void,
    );
    assert_eq!(
        CallableNodeView::new(&dispatch, f).event_names(navigate()),
        Some(vec![Arc::<str>::from("a")]),
        "a `ConstructorType` (`new () => X`) arm is SKIPPED (complete-no-name), exactly like `Function` — `'a' | (new () => X)` yields `[\"a\"]`, never fail-closed `None`"
    );
}
