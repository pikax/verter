//! Graph-native registry-route + cycle-gate predicates.
//!
//! Domain 12 — owns the §1.12 graph-native variants of the
//! TypeExpr-based registry-route helpers, the package-ref check, and
//! the node-domain front of the materialization cycle gate:
//!
//! - `RouteExtraction` struct + `extract_route_root_identity_node`,
//!   `extract_pick_omit_route`, `build_keys_union_node`,
//!   `extract_indexed_access_route`, `collect_string_literal_union_keys_node`
//!   (route extraction).
//! - `component_meta_ref_resolves_to_package_node` (package-ref check).
//! - `type_node_has_package_backed_root`,
//!   `declaration_body_prefers_inline_materialization_node`
//!   (graph-native predicates the materializer / walker call).
//! - `node_root_reaches_transitive_cycle_with_fence` (the node-root
//!   aggregator over the sealed materialization cycle gate —
//!   [`ProjectSemanticDispatch::classify_materialization_cycle_gate`],
//!   the SOLE cycle-gate authority).

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use std::sync::Arc;

/// Return type for [`extract_route_root_identity_node`].
///
/// Pairs the bare-root declaration identity with the route shape that
/// the Pick/Omit/IndexedAccess wrapping carries. Distinct from the
/// TypeExpr-based `(String, RouteDemand)` tuple in the existing
/// `component_meta_registry_public_*_route` helpers because
/// `DeclIdentity` carries the full canonical-id + whole-hash pair the
/// graph layer needs for dispatch keys and package-ref checks.
///
/// P0 #3 — `root_args` preserves the generic root
/// carrier's type arguments so `Pick<Foo<T>, 'a'>` and `Foo<T>['a']`
/// shapes can project. Empty for bare-DeclRef roots; non-empty for
/// `InstantiationRef` roots (i.e., the original generic shell).
#[derive(Debug, Clone)]
pub(crate) struct RouteExtraction {
    pub root_identity: crate::semantic_query::DeclIdentity,
    pub root_args: Arc<[crate::semantic_query::SemanticNodeId]>,
    pub route: crate::resolver_core::RouteDemand,
}

/// Graph-native variant of the `TypeExpr`-based
/// registry route extraction (`component_meta_registry_public_utility_route` +
/// `component_meta_registry_public_indexed_access_route`).
///
/// Returns `Some(RouteExtraction)` ONLY when `node` matches one of:
///
/// - `Pick<X, 'a' | 'b' | …>` — `InstantiationRef` with
///   `base.canonical_id == "__builtin__"` AND
///   `base.decl_name == "Pick"`, `args.len() == 2`, arg[1] is a
///   string-literal or a union of string-literals (must yield ≥ 1
///   key). arg[0] may be a bare `DeclRef` OR an `InstantiationRef`
///   (generic root preserved via `root_args` per R8-2).
/// - `Omit<X, 'a' | 'b' | …>` — same shape with `decl_name == "Omit"`.
/// - `Foo['a']['b']…` — chained `IndexedAccess` whose innermost
///   `object` is a bare `DeclRef` OR `InstantiationRef`, with every
///   `IndexKey` a `String` literal (rejects `IndexKey::Number` /
///   `IndexKey::Computed`).
///
/// Plain `DeclRef` and userland (non-builtin) `InstantiationRef`
/// return `None` — they are NOT route shapes; they fall through to the
/// recursive-helper guard in B1 step 4.
///
/// Parity tightenings with TypeScript's built-in semantics:
///
/// - Userland Pick/Omit (a userland `Pick`/`Omit` decl that shadows
///   the builtin) is NOT a registry route — only `__builtin__` Pick/
///   Omit dispatch through this branch.
/// - 1-arg / 3-arg `Pick` rejected: `args.len() != 2` returns `None`.
/// - Empty union rejected: `Pick<Foo, never>` returns `None`.
/// - Numeric/type indices rejected: `Foo[0]` and `Foo[K]` return `None`.
///
/// `depth` fuses recursion at 256 to bound runtime on adversarial
/// inputs.
pub(crate) fn extract_route_root_identity_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> Option<RouteExtraction> {
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return None;
    }
    let data = graph.node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::InstantiationRef { base, args }
            if base.canonical_id.as_ref() == "__builtin__"
                && matches!(base.decl_name.as_ref(), "Pick" | "Omit") =>
        {
            extract_pick_omit_route(graph, base, args, depth + 1)
        }
        SemanticNodeData::IndexedAccess { .. } => {
            extract_indexed_access_route(graph, node, depth + 1)
        }
        // Plain DeclRef → step 4 (recursive-helper guard).
        // Userland InstantiationRef → step 4 (recursive-helper guard).
        // Builtin Extract/Exclude/NonNullable → existing flow (lower
        // already eager-resolves them; they don't reach this branch).
        _ => None,
    }
}

/// Helper: extract `Pick<X, keys>` / `Omit<X, keys>` route. Recurses
/// into `args[0]` to find the actual root identity (R8-2 fix —
/// previously returned `Pick`'s `__builtin__` identity, breaking the
/// cycle / package guards).
///
/// P0 #3 — preserves generic root carriers: when
/// `args[0]` is `InstantiationRef { base: G, args: [..gargs..] }`,
/// the extracted `root_identity` is `G` and `root_args` is `[..gargs..]`.
/// Bare `DeclRef` arms produce empty `root_args`.
pub(crate) fn extract_pick_omit_route(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    base: &crate::semantic_query::DeclIdentity,
    args: &Arc<[crate::semantic_query::SemanticNodeId]>,
    depth: u32,
) -> Option<RouteExtraction> {
    use crate::resolver_core::RouteDemand;
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return None;
    }
    if args.len() != 2 {
        return None;
    }
    // R8-2 fix — recurse into args[0] for the actual root identity
    // and preserve generic carriers via root_args.
    let inner_data = graph.node_data(args[0])?;
    let (root_identity, root_args) = match inner_data.as_ref() {
        SemanticNodeData::DeclRef { identity } => (
            identity.clone(),
            Arc::<[crate::semantic_query::SemanticNodeId]>::from(Vec::new().into_boxed_slice()),
        ),
        SemanticNodeData::InstantiationRef {
            base: gen_base,
            args: gen_args,
        } => (gen_base.clone(), Arc::clone(gen_args)),
        // Symbolic-keep behavior for non-ref roots
        // depends on `evaluate_deferred_semantic_node` not unwrapping
        // carriers (verified at evaluate.rs:39). If a future change
        // there adds carrier unwrapping, this branch must keep
        // explicitly returning `None` so we don't materialise a
        // non-projectable shape.
        _ => return None,
    };
    let keys = collect_string_literal_union_keys_node(graph, args[1])?;
    if keys.is_empty() {
        return None;
    }
    let route = if base.decl_name.as_ref() == "Pick" {
        RouteDemand::pick(keys)
    } else {
        RouteDemand::omit(keys)
    };
    Some(RouteExtraction {
        root_identity,
        root_args,
        route,
    })
}

/// Build a string-literal-union node from a list of keys
/// for the 2-step Pick/Omit dispatch orchestration. Used by the
/// materialiser registry-route branch to construct the keys argument
/// for the second-step `Instantiate { Pick/Omit, [body_id, keys_node] }`
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
    if let [only] = key_ids.as_slice() {
        Some(*only)
    } else {
        Some(graph.intern_node(SemanticNodeData::Union(Arc::from(
            key_ids.into_boxed_slice(),
        ))))
    }
}

/// Helper: walk an `IndexedAccess` chain and produce a
/// `RouteExtraction` whose route is `RouteDemand::MemberPath`.
/// Innermost root may be a bare `DeclRef` OR `InstantiationRef`;
/// generic carriers are preserved via `root_args` per R8-2.
pub(crate) fn extract_indexed_access_route(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> Option<RouteExtraction> {
    use crate::resolver_core::RouteDemand;
    use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};

    let mut hops_reverse: Vec<String> = Vec::new();
    let mut current: SemanticNodeId = node;
    let mut d = depth;
    loop {
        if d > 256 {
            return None;
        }
        d += 1;
        let data = graph.node_data(current)?;
        match data.as_ref() {
            SemanticNodeData::IndexedAccess { object, index } => {
                let hop = match index {
                    IndexKey::String(s) => s.to_string(),
                    // Numeric / type indices are not legal route
                    // hops (parity with TypeScript: `Foo[0]` and
                    // `Foo[K]` are not declared registry routes).
                    IndexKey::Number(_) | IndexKey::UniqueSymbol(_) | IndexKey::Computed(_) => {
                        return None;
                    }
                };
                hops_reverse.push(hop);
                current = *object;
            }
            SemanticNodeData::DeclRef { identity } => {
                hops_reverse.reverse();
                return Some(RouteExtraction {
                    root_identity: identity.clone(),
                    root_args: Arc::from(Vec::new().into_boxed_slice()),
                    route: RouteDemand::member_path(hops_reverse),
                });
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                // R8-2 — preserve generic root carriers like
                // `Foo<T>['a']`.
                hops_reverse.reverse();
                return Some(RouteExtraction {
                    root_identity: base.clone(),
                    root_args: Arc::clone(args),
                    route: RouteDemand::member_path(hops_reverse),
                });
            }
            _ => return None,
        }
    }
}

/// Helper: collect all string-literal members of a literal-or-union
/// node. Returns `None` when any member is non-literal-string (rejects
/// `Pick<Foo, 'a' | number>` and similar mixed unions).
pub(crate) fn collect_string_literal_union_keys_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
) -> Option<Vec<String>> {
    use crate::semantic_query::SemanticNodeData;
    use verter_type_expr::LiteralValue;

    let data = graph.node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Literal(LiteralValue::String(s)) => Some(vec![s.clone()]),
        SemanticNodeData::Union(members) => {
            let mut keys: Vec<String> = Vec::with_capacity(members.len());
            for &member_id in members.iter() {
                let member_data = graph.node_data(member_id)?;
                match member_data.as_ref() {
                    SemanticNodeData::Literal(LiteralValue::String(s)) => keys.push(s.clone()),
                    _ => return None,
                }
            }
            Some(keys)
        }
        _ => None,
    }
}

// `collect_indexed_access_path_node` and `bare_decl_ref_identity_node`
// were
// chain inline (preserving generic root carriers via `root_args`),
// and `extract_pick_omit_route` recurses into `args[0]` directly to
// find the actual root identity (R8-2 fix). Both functions had
// `_node` allow_dead_code annotations and no remaining production
// callers; deleted to keep the surface minimal.

/// Graph-native variant of the TypeExpr package-ref
/// check. Routes the canonical-id classification through the
/// workspace's `ResolverContext::workspace_is_package_backed` accessor
/// so symlinked / pnpm-hoisted layouts (canonical path contains
/// `/node_modules/` but the realpath sits under a workspace project)
/// are correctly classified as workspace-owned.
pub(crate) fn component_meta_ref_resolves_to_package_node(
    ctx: &dyn ResolverContext,
    identity: &crate::semantic_query::DeclIdentity,
) -> bool {
    ctx.workspace_is_package_backed(identity.canonical_id.as_ref())
}

/// Graph-native predicate. Returns `true` when `node`'s route root
/// resolves to a `/node_modules/`-rooted decl identity.
///
/// Mirrors the TypeExpr predicate's structural recursion:
///
/// - `DeclRef` / `InstantiationRef` — terminal; checks root identity
///   via [`component_meta_ref_resolves_to_package_node`].
/// - `IndexedAccess { object, .. }` — recurses into `object` (matches
///   `TypeExpr::IndexedAccess { object, .. }`).
/// - `Array { element, .. }` — recurses into `element` (matches
///   `TypeExpr::Array { element, .. }`).
/// - `KeyOf { base }` — recurses into `base` (matches
///   `TypeExpr::KeyOf(object)`).
/// - `Tuple { elements }` — short-circuits to `true` on any element
///   whose `value` flips the predicate (matches `TypeExpr::Tuple`).
/// - `Alias(inner)` — pass-through (graph-native shape; TypeExpr has
///   no equivalent because it is not interned).
/// - All other shapes — `false` (matches the TypeExpr `_` arm).
///
/// `depth` is fused at 256 to bound runtime on pathological chains
/// (matches the cycle-gate producer's scanner fuses). On fuse
/// the predicate returns `false`: a runaway recursion is treated as
/// "not package-backed" so the caller does NOT short-circuit through
/// the package-backed branch.
#[allow(dead_code)]
pub(crate) fn type_node_has_package_backed_root(
    ctx: &dyn ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> bool {
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return false;
    }
    let graph = ctx.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => {
            component_meta_ref_resolves_to_package_node(ctx, identity)
        }
        SemanticNodeData::InstantiationRef { base, .. } => {
            component_meta_ref_resolves_to_package_node(ctx, base)
        }
        SemanticNodeData::IndexedAccess { object, .. } => {
            type_node_has_package_backed_root(ctx, *object, depth + 1)
        }
        SemanticNodeData::Array { element, .. } => {
            type_node_has_package_backed_root(ctx, *element, depth + 1)
        }
        SemanticNodeData::KeyOf { base } => {
            type_node_has_package_backed_root(ctx, *base, depth + 1)
        }
        SemanticNodeData::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_node_has_package_backed_root(ctx, element.value, depth + 1)),
        SemanticNodeData::Alias(inner) => type_node_has_package_backed_root(ctx, *inner, depth + 1),
        _ => false,
    }
}

/// Graph-native variant of the body inline-materialisation
/// preference predicate. Returns `true` when the body shape is suitable
/// for inline materialisation through the registry-route entry.
///
/// Reserved for re-wiring once migrates the inline-route
/// composition site to graph-native (the predicate's only consumer
/// previously the registry-route inline
/// composition predicate, which was deleted). Tests in
/// `meta_resolve_tests.rs` exercise this predicate directly.
#[allow(
    dead_code,
    reason = "Re-wired by inline-materialization predicate; covered by unit tests in meta_resolve_tests.rs"
)]
pub(crate) fn declaration_body_prefers_inline_materialization_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    body_id: crate::semantic_query::SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    let Some(data) = graph.node_data(body_id) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Object(_) => true,
        SemanticNodeData::DeclRef { .. } => true,
        SemanticNodeData::Alias(inner) => {
            declaration_body_prefers_inline_materialization_node(graph, *inner)
        }
        _ => extract_route_root_identity_node(graph, body_id, 0).is_some(),
    }
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
