//! Graph-native key-domain + package-root + cycle-gate predicates.
//!
//! Owns the graph-native predicates the projector output sink and the
//! registry-decl route consult over interned semantic nodes:
//!
//! - `build_keys_union_node` — interns the canonical key-domain union a
//!   builtin `Pick`/`Omit` route carries.
//! - `node_package_backed_object_like_root_with_fence` — the
//!   package-backed object-like root check, fact-fenced.
//! - `node_root_reaches_transitive_cycle_with_fence` (the node-root
//!   aggregator over the sealed materialization cycle gate —
//!   [`ProjectSemanticDispatch::classify_materialization_cycle_gate`],
//!   the SOLE cycle-gate authority).
//!
//! No predicate here evaluates a selective operator: route recognition
//! and surface materialisation belong to the one shared query route
//! (`SemanticQueryKey` → `ProjectSemanticDispatch::execute`).

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use std::sync::Arc;

/// Build a string-literal-union node from a list of keys
/// for the 2-step Pick/Omit dispatch orchestration. Used by the
/// registry-decl Pick/Omit route to construct the keys argument for
/// the second-step `Instantiate { Pick/Omit, [body_id, keys_node] }`
/// dispatch.
///
/// Single-key fast path produces a bare `Literal` node; multi-key
/// produces a `Union` of literals. Both are interned at global scope
/// (no file scope) since the keys are workspace-shared sentinels.
pub(crate) fn build_keys_union_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    keys: &[verter_type_expr::facts::FactPropertyKey],
) -> Option<crate::semantic_query::SemanticNodeId> {
    use crate::semantic_query::SemanticNodeData;
    use verter_type_expr::LiteralValue;
    use verter_type_expr::PropertyKey;

    let key_ids: Option<Vec<crate::semantic_query::SemanticNodeId>> = keys
        .iter()
        .map(|key| match key {
            PropertyKey::String(value) => Some(graph.intern_node(SemanticNodeData::Literal(
                LiteralValue::String(value.to_string()),
            ))),
            PropertyKey::Number(value) => Some(graph.intern_node(SemanticNodeData::Literal(
                LiteralValue::Number(value.get() as f64),
            ))),
            // The semantic node vocabulary has no nominal unique-symbol leaf.
            // Reject the conversion instead of fabricating a string literal.
            PropertyKey::UniqueSymbol(_) => None,
        })
        .collect();
    let key_ids = key_ids?;
    // Canonical construction: the literal key-domain union routes through
    // the one authority. This is the PROVABLY-EMPTY-evidence site: every
    // arm is a freshly interned `Global`-scoped childless literal, so the
    // walk records no file roots and can never be incomplete — asserted
    // below rather than threaded to a disposition boundary.
    let composite = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
        graph,
        &key_ids,
        crate::semantic_query::NullabilityPolicy::Strict,
    );
    verter_debug_assert::verter_debug_assert!(
        composite.evidence.inspected_file_roots.is_empty() && !composite.evidence.incomplete,
        "key-domain union over freshly interned Global literals must carry no evidence"
    );
    Some(composite.node)
}

/// Extract the package-backed gate's ROOT declaration IDENTITY from a graph
/// `node` — the node front of the SHARED root-identity tail
/// ([`crate::meta_resolve::materialize::package_backed_object_like_root_identity_with_fence`]).
/// The node carrier already holds the RESOLVED [`crate::semantic_query::DeclIdentity`]
/// (`DeclRef.identity` / `InstantiationRef.base`), so NO name re-resolution from
/// `scope` is needed — this is the identity-preserving fix for the former
/// synthetic `TypeExpr::named(name)` bridge, which could re-resolve a DIFFERENT
/// symbol than the carrier names.
///
/// - `Alias(inner)` — pass-through (graph-native; `Parenthesized` equivalent).
/// - `IndexedAccess { object, .. }` — descend to the indexed-access root.
/// - `Pick`/`Omit` BUILTIN `InstantiationRef` (2 args) — descend to the SOURCE
///   root (`args[0]`), NOT the `__builtin__::Pick` wrapper. A userland
///   `InstantiationRef` whose base is NOT `__builtin__` is its OWN root.
/// - `DeclRef` / `InstantiationRef` — the carried declaration identity.
/// - `BareRef` — resolve the head through the carrier resolver
///   ([`ProjectSemanticDispatch::resolve_carrier_subject_node`] under
///   `Published(Navigate)`) and extract the resolved identity; a real miss yields
///   `None`.
/// - anything else — `None`.
fn node_root_identity(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> Option<crate::semantic_query::DeclIdentity> {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticNodeData};

    if depth > 256 {
        return None;
    }
    enum Action {
        Recurse(crate::semantic_query::SemanticNodeId),
        Identity(crate::semantic_query::DeclIdentity),
        ResolveBare,
        None,
    }
    let action = {
        let graph = dispatch.graph();
        let data = graph.node_data(node)?;
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => Action::Recurse(*inner),
            SemanticNodeData::IndexedAccess { object, .. } => Action::Recurse(*object),
            SemanticNodeData::InstantiationRef { base, args }
                if base.canonical_id.as_ref() == "__builtin__"
                    && matches!(base.decl_name.as_ref(), "Pick" | "Omit")
                    && args.len() == 2 =>
            {
                Action::Recurse(args[0])
            }
            SemanticNodeData::DeclRef { identity } => Action::Identity(identity.clone()),
            SemanticNodeData::InstantiationRef { base, .. } => Action::Identity(base.clone()),
            data if data.bare_ref_head().is_some() => Action::ResolveBare,
            _ => Action::None,
        }
    };
    match action {
        Action::Recurse(next) => node_root_identity(dispatch, next, depth + 1),
        Action::Identity(identity) => Some(identity),
        Action::ResolveBare => {
            let resolved = dispatch.resolve_carrier_subject_node(
                node,
                ProjectionReductionContext::published(ProjectionMode::Navigate),
            );
            if resolved != node {
                node_root_identity(dispatch, resolved, depth + 1)
            } else {
                None
            }
        }
        Action::None => None,
    }
}

/// Node-domain front for the package-backed object-like-root gate. Extracts the
/// root declaration IDENTITY from `node` ([`node_root_identity`], which handles
/// the `Pick`/`Omit` builtin source-root trap, indexed-access roots, and BareRef
/// head resolution) and feeds it through the SHARED identity + object-like + fence
/// tail
/// ([`crate::meta_resolve::materialize::package_backed_object_like_root_identity_with_fence`])
/// — so the verdict + fence are computed by the one shared identity tail over the
/// resolved root identity. A node with no extractable root identity is not
/// package-backed (empty fence — admittable).
pub(crate) fn node_package_backed_object_like_root_with_fence(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    node: crate::semantic_query::SemanticNodeId,
) -> (bool, Option<crate::semantic_query::DepSignature>) {
    let root_identity = {
        let dispatch = ProjectSemanticDispatch::new(query_engine.ctx);
        node_root_identity(&dispatch, node, 0)
    };
    let Some(root_identity) = root_identity else {
        return (false, Some(Arc::from(Vec::new())));
    };
    crate::meta_resolve::materialize::package_backed_object_like_root_identity_with_fence(
        query_engine,
        scope_canonical_id,
        &root_identity,
    )
}

/// Collect the SURFACE root declaration identities of a graph `node`:
/// the outer carrier's identity plus every type-argument's identity, descending
/// only `Alias` / `IndexedAccess.object` / `InstantiationRef.args`. The node
/// carrier already holds the RESOLVED `DeclIdentity` (`DeclRef.identity` /
/// `InstantiationRef.base`); a `BareRef` head is resolved through the carrier
/// resolver ([`ProjectSemanticDispatch::resolve_carrier_subject_node`] under
/// `Published(Navigate)`) and the resolved `DeclRef`/`InstantiationRef` identity
/// is collected (a generic carrier `A<string>` must NOT bypass the cycle gate).
/// A real miss collects no root. `MAX_*` caps bound the collection; when a cap
/// stops a push or a descent that could have contributed a root, `truncated`
/// is set so the aggregate demotes to a fallback instead of silently ORing a
/// partial root set.
fn collect_node_root_identities(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
    out: &mut Vec<crate::semantic_query::DeclIdentity>,
    truncated: &mut bool,
) {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticNodeData};
    const MAX_CYCLE_ROOTS: usize = 16;
    const MAX_ROOT_COLLECT_DEPTH: u32 = 8;
    if out.len() >= MAX_CYCLE_ROOTS || depth >= MAX_ROOT_COLLECT_DEPTH {
        // A cap fired with the node unexamined: any shape that could
        // have contributed (or descended to) a root marks the
        // collection truncated.
        let could_contribute = match dispatch.graph().node_data(node).as_deref() {
            Some(
                SemanticNodeData::Alias(_)
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. },
            ) => true,
            Some(data) => data.bare_ref_head().is_some(),
            None => false,
        };
        if could_contribute {
            *truncated = true;
        }
        return;
    }
    enum Step {
        Recurse(crate::semantic_query::SemanticNodeId),
        Push(crate::semantic_query::DeclIdentity),
        PushAndRecurseArgs(
            crate::semantic_query::DeclIdentity,
            Vec<crate::semantic_query::SemanticNodeId>,
        ),
        ResolveBare,
        Stop,
    }
    let step = {
        let graph = dispatch.graph();
        let Some(data) = graph.node_data(node) else {
            return;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => Step::Recurse(*inner),
            SemanticNodeData::IndexedAccess { object, .. } => Step::Recurse(*object),
            SemanticNodeData::DeclRef { identity } => Step::Push(identity.clone()),
            SemanticNodeData::InstantiationRef { base, args } => {
                Step::PushAndRecurseArgs(base.clone(), args.to_vec())
            }
            data if data.bare_ref_head().is_some() => Step::ResolveBare,
            _ => Step::Stop,
        }
    };
    match step {
        Step::Recurse(next) => {
            collect_node_root_identities(dispatch, next, depth + 1, out, truncated)
        }
        Step::Push(identity) => {
            if !out.contains(&identity) {
                out.push(identity);
            }
        }
        Step::PushAndRecurseArgs(base, args) => {
            if !out.contains(&base) {
                out.push(base);
            }
            for arg in args {
                collect_node_root_identities(dispatch, arg, depth + 1, out, truncated);
            }
        }
        Step::ResolveBare => {
            let resolved = dispatch.resolve_carrier_subject_node(
                node,
                ProjectionReductionContext::published(ProjectionMode::Navigate),
            );
            if resolved != node {
                collect_node_root_identities(dispatch, resolved, depth + 1, out, truncated);
            }
        }
        Step::Stop => {}
    }
}

/// Node-domain front for the transitive-cycle gate. Collects the surface root
/// identities of `node` ([`collect_node_root_identities`]) and aggregates the
/// sealed materialization cycle gate
/// ([`ProjectSemanticDispatch::classify_materialization_cycle_gate`]) over
/// each through the OR lattice
/// ([`crate::semantic_query::MaterializationCycleGateOutcome::aggregate`]):
/// Stop dominates Continue, any `LegacyFallback` infects the aggregate
/// (its partial rail is observed onto the request), and a truncated root
/// collection adds `RootCollectorLimit` (never a silent false). Each root
/// read's cross-file dep signature is merged into the returned fence, which
/// is observed via `emit_dispatch_dep_signature_facts`. Takes `ctx` (the
/// node carries resolved identities, so no name-resolution engine is
/// needed).
pub(crate) fn node_root_reaches_transitive_cycle_with_fence(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    node: crate::semantic_query::SemanticNodeId,
) -> (bool, crate::semantic_query::DepSignature) {
    use crate::semantic_query::{
        MaterializationCycleGateFallbackReason, MaterializationCycleGateFallbackReasons,
        MaterializationCycleGateOutcome, MaterializationCycleGateVerdict,
    };

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let mut roots: Vec<crate::semantic_query::DeclIdentity> = Vec::new();
    let mut truncated = false;
    collect_node_root_identities(&dispatch, node, 0, &mut roots, &mut truncated);
    if roots.is_empty() {
        if truncated {
            // A truncated collection with no collected roots cannot
            // prove "no cycle": the walk is incomplete, the verdict is
            // fail-open Continue, and the request goes partial.
            crate::request_context::mark_request_result_partial();
        }
        return (false, Arc::from(Vec::new()));
    }
    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut outcomes: Vec<MaterializationCycleGateOutcome> = Vec::with_capacity(roots.len() + 1);
    if truncated {
        crate::request_context::mark_request_result_partial();
        outcomes.push(MaterializationCycleGateOutcome::LegacyFallback {
            verdict: MaterializationCycleGateVerdict::Continue,
            reasons: MaterializationCycleGateFallbackReasons::new([
                MaterializationCycleGateFallbackReason::RootCollectorLimit,
            ])
            .expect("single reason is non-empty"),
        });
    }
    for identity in &roots {
        if !identity.canonical_id.as_ref().is_empty()
            && identity.canonical_id.as_ref() != "__builtin__"
            && identity.canonical_id.as_ref() != scope_canonical_id
        {
            fence.push((
                Arc::clone(&identity.canonical_id),
                crate::semantic_query::DepVersion::WholeHash(identity.whole_hash),
            ));
        }
        let read = dispatch.classify_materialization_cycle_gate(identity);
        crate::request_context::observe_component_meta_read_suppress(&read);
        crate::component_meta_audit::merge_dep_signature_into_local_fence(
            &mut fence,
            &read.dep_signature,
        );
        outcomes.push(read.value);
    }
    let aggregate = MaterializationCycleGateOutcome::aggregate(outcomes);
    let fence_signature: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
    crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts(ctx, &fence_signature);
    (
        matches!(aggregate.verdict(), MaterializationCycleGateVerdict::Stop),
        fence_signature,
    )
}

#[cfg(test)]
#[path = "graph_predicates_tests.rs"]
mod graph_predicates_tests;
