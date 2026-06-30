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
    FunctionParam, LiteralValue, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    SemanticNodeData, SemanticNodeId, SurfaceMember, SurfaceView, TupleElement,
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

    let void = prim(&graph, PrimitiveKind::Void);
    let p1 = prim(&graph, PrimitiveKind::Number);
    let p2 = prim(&graph, PrimitiveKind::String);
    let f1 = function(&graph, vec![param(Some("a"), p1, false, false)], void);
    let f2 = function(&graph, vec![param(Some("b"), p2, false, false)], void);
    let slot = union(&graph, vec![f1, f2]);

    let view = CallableNodeView::new(&dispatch, slot);
    let parts = view
        .slot_param_and_return_by_arm(ArmCombineNode::Union, shallow())
        .expect("a 2-arm slot yields parts");
    let first_param = parts
        .first_param
        .expect("every arm supplies a first param -> intersected binding");
    let mat = dispatch
        .materialize_output_type_expr_for_test(first_param)
        .expect("the combined first param materializes");
    let TypeExpr::Intersection(arms) = &mat else {
        panic!("the combined first param is an Intersection of both arms, got {mat:?}");
    };
    assert_eq!(
        arms.len(),
        2,
        "the intersection carries both arms' first params"
    );
    assert!(
        arms.iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::Number))),
        "one arm's first param is `number`"
    );
    assert!(
        arms.iter()
            .any(|a| matches!(a, TypeExpr::Primitive(PrimitiveName::String))),
        "the other arm's first param is `string`"
    );
    assert!(
        parts.return_type.is_some(),
        "the return type combines across arms"
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
        ret,
        "return_type reads the function's return node"
    );
    assert_eq!(
        sig.return_type_span(),
        Some(span),
        "return_type_span reads the stored span (not a constant None)"
    );
    let positions = sig.positional_params_expanded();
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
