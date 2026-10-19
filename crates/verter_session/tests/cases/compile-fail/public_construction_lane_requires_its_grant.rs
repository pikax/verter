//! Compile-fail: holding a graph grants no construction lane. A bare
//! ordinal is rooted or read only inside a construction scope, and a scope
//! opens only through the grant the engine minted with its graph; the grant
//! cannot be minted from the graph (and, see
//! `public_node_capability_cannot_be_forged`, not forged either). No graph
//! read keyed by a bare ordinal — alias chains, origin scopes, structure
//! verdicts, substitution memos, member indexes, origin-edge exports and
//! records, stable keys and their orderings, union and intersection
//! builders, union-surface merges over caller-built surfaces, warm memo
//! reads and producer claims keyed by a query identity, printed
//! names, mapper and `keyof` classifications, index signature
//! applicability, merged-declaration reduction, flow-return verdicts,
//! display — is callable outside the engine, and no bare ordinal
//! releases kernel roots. The lane-gated readers (`display_in_scope`,
//! `intern_ordered_union_in_scope`) answer only inside a held lane, and a
//! macro spread-formula merge runs only through a dispatcher.

use verter_session_query::flow::completion::NormalCompletion;
use verter_session_query::flow::policy::NullabilityPolicy;
use verter_type_engine::project_semantic_dispatch::{canonical_algebra, relation_predicates, walk};
use verter_type_engine::resolver_core::{RequestFlags, ResolverCapabilities, ResolverContext};
use verter_type_engine::semantic_query::semantic_context::SemanticContext;
use verter_type_engine::semantic_query::{
    display, keyof_operand_is_type_variable, printed_union_arms, stable_key,
    ClassExpressionIdentity, DepSignature, DisplayNeeds, FlowReturnKey, FlowReturnResult,
    MapperKind, ObjectProjectionAlternative, OriginEdgeKind, OriginMeta, PrimitiveKind,
    PropertyKey, RelateMemoKey, ResolveCallKey, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, SurfaceView,
};
use verter_type_engine::semantic_query_memo::node_roots::NodeConstructionGrant;
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

fn open_a_lane(graph: &SemanticGraphStore) {
    let _scope = graph.enter_node_root_scope();
}

fn root_a_bare_ordinal(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _roots = graph.root_nodes([id]);
}

fn mint_a_grant(graph: &SemanticGraphStore) -> NodeConstructionGrant {
    NodeConstructionGrant::mint(graph)
}

fn follow_an_alias_chain(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _end = graph.alias_chain_end(id, |_| {});
}

fn read_an_origin_scope(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _scope = graph.node_scope(id);
}

fn copy_an_origin_scope(graph: &SemanticGraphStore, id: SemanticNodeId) -> SemanticNodeId {
    graph.intern_preserving_scope(id, SemanticNodeData::Primitive(PrimitiveKind::String))
}

fn classify_a_structure(graph: &SemanticGraphStore, id: SemanticNodeId) -> bool {
    graph.node_reaches_unresolved(id) || graph.node_is_inert_structure(id)
}

fn read_a_substitution(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _hit = graph.substitute_memo_get(id, id, id);
    graph.substitute_memo_publish(id, id, id, id);
}

fn index_a_surface(graph: &SemanticGraphStore, id: SemanticNodeId, view: &SurfaceView) {
    let _index = graph.member_ordinal_index(id, view);
}

fn export_every_origin_edge(graph: &SemanticGraphStore) {
    let _edges = graph.export_all_origin_edges();
}

fn release_kernel_roots(graph: &SemanticGraphStore, id: SemanticNodeId) {
    let _stranded = graph.signature_store().note_released_nodes([id]);
    let _epoch = graph.release_signature_records_naming([id]);
}

fn record_an_origin_edge(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    kind: OriginEdgeKind,
    meta: OriginMeta,
    fence: DepSignature,
) {
    graph.record_origin_edge(id, kind, std::sync::Arc::from([id]), meta, fence);
}

fn key_an_ordinal(graph: &SemanticGraphStore, id: SemanticNodeId, ids: &mut Vec<SemanticNodeId>) {
    let _key = stable_key::stable_key_for_node(graph, id);
    let _equal = stable_key::provably_equal(graph, id, id);
    stable_key::sort_by_stable_key(graph, ids);
    stable_key::sort_union_members_by_stable_key(graph, ids);
    stable_key::sort_and_collapse_union_members(graph, ids);
    let _canonical = stable_key::canonicalize_union_members(graph, ids);
    let _ranks = stable_key::union_rank_order(graph, ids);
}

fn view_a_union(graph: &SemanticGraphStore, id: SemanticNodeId, ctx: &SemanticContext) {
    let _members = stable_key::semantic_union_members(graph, id, ctx);
}

fn build_over_ordinals(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    nullability: NullabilityPolicy,
) {
    let _union = canonical_algebra::intern_ordered_union(graph, &[id], nullability);
    let _intersection = canonical_algebra::intern_ordered_intersection(graph, &[id]);
    let _reduced = canonical_algebra::reduced_authored_intersection(graph, id, nullability);
    let _merged = walk::reduce_merged_decl_with_graph(graph, &[id]);
}

fn print_and_classify(
    graph: &SemanticGraphStore,
    id: SemanticNodeId,
    class: &ClassExpressionIdentity,
    key: &PropertyKey,
) {
    let _arms = printed_union_arms(graph, &[id]);
    let _name = class.printed_name_in(graph, &[id]);
    let _mapper = MapperKind::classify_value_expr(graph, id, id, id);
    let _variable = keyof_operand_is_type_variable(graph, id);
    let _applies = relation_predicates::index_signature_applies_to_property(graph, id, key);
}

fn render_and_judge(graph: &SemanticGraphStore, id: SemanticNodeId, completion: NormalCompletion) {
    let _shown = display::display(
        graph,
        &SemanticQueryValue::TypeNode(id),
        DisplayNeeds::empty(),
    );
    let _verdict = FlowReturnResult::new(graph, id, completion, None);
}

fn merge_caller_surfaces(
    graph: &SemanticGraphStore,
    arms: &[Option<walk::ShallowSurface>],
    alternatives: &[ObjectProjectionAlternative],
    evidence: &mut canonical_algebra::CanonicalEvidence,
) {
    let _common = walk::merge_union_surfaces(graph, arms, evidence);
    let _macro = walk::merge_union_surfaces_for_macro(graph, arms, evidence);
    let _members = walk::spread_formula_positive_members_for_macro(graph, alternatives, evidence);
}

fn read_memos_by_key<C: ResolverCapabilities>(
    graph: &SemanticGraphStore,
    ctx: &dyn ResolverContext<C>,
    flags: &RequestFlags,
    key: SemanticQueryKey,
    flow: &FlowReturnKey,
    relation: &RelateMemoKey,
    call: &ResolveCallKey,
) {
    let _warm = graph.get_validated(&key, ctx);
    let _flow = graph.get_flow_return_result(ctx, flow);
    let _relation = graph.get_relation_payload(ctx, relation);
    let _call = graph.get_resolve_call_result(ctx, call);
    let _same_path = graph.is_same_path_claim(&key);
    let _recompute = graph.begin_query_recompute(flags, key);
    let _flow_flight = graph.begin_inline_flow_return_flight(flow);
    let _relation_flight = graph.begin_inline_relation_flight(relation);
}

fn main() {}
