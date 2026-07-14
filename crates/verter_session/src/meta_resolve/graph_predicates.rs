//! Graph-native registry-route + cycle-BFS predicates.
//!
//! Domain 12 — owns the §1.12 graph-native variants of the
//! TypeExpr-based registry-route helpers, the package-ref check, and
//! the cycle-BFS predicates that gate ref/recursive-ref termination:
//!
//! - `RouteExtraction` struct + `extract_route_root_identity_node`,
//!   `extract_pick_omit_route`, `build_keys_union_node`,
//!   `extract_indexed_access_route`, `collect_string_literal_union_keys_node`
//!   (route extraction).
//! - `component_meta_ref_resolves_to_package_node` (package-ref check).
//! - `type_node_has_package_backed_root`,
//!   `declaration_body_prefers_inline_materialization_node`
//!   (graph-native predicates the materializer / walker call).
//! - `ref_root_reaches_transitive_cycle_node`, `bfs_compute_inner`,
//!   `body_contains_recursive_ref_to_name`,
//!   `has_complex_cycle_guard_surface_node`, `collect_ref_identities_node`
//!   (cycle-BFS termination + recursive-ref reachability).

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use std::sync::Arc;

// `dep_signature` module is accessed via path (`dep_signature::BFS_*_COUNTER`)
// inside `bfs_compute_inner` for thread-local counter access. The bare
// module-name use is gated `#[cfg(test)]` because the BFS instrumentation
// counters are themselves test-only (`#[cfg(test)]` thread-locals).
#[cfg(test)]
use super::dep_signature;
#[cfg(test)]
use super::dep_signature::record_bfs_child_refs_count_for_test;

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
///   `IndexKey::TypeNode`).
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
    keys: &[String],
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::SemanticNodeData;
    use verter_type_expr::LiteralValue;

    if keys.len() == 1 {
        graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            keys[0].clone(),
        )))
    } else {
        let key_ids: Vec<crate::semantic_query::SemanticNodeId> = keys
            .iter()
            .map(|k| graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(k.clone()))))
            .collect();
        graph.intern_node(SemanticNodeData::Union(Arc::from(
            key_ids.into_boxed_slice(),
        )))
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
                    IndexKey::Number(_) | IndexKey::TypeNode(_) => return None,
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
/// (matches [`has_complex_cycle_guard_surface_node`] etc.). On fuse
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

/// R — graph-native BFS for transitive cycle
/// detection, with ctx-owned cache.
///
/// Architecture:
///   1. **Fast path (§4.9)** — `RefCycleResultDb::peek` consults the
///      generation-local cache. On `validated_at_generation == current`,
///      returns the cached `bool` without re-walking.
///   2. **Slow path** — cooperative-admission via
///      `ref_cycle_db_get_or_compute`; the BFS body
///      ([`bfs_compute_inner`]) runs synchronously in the
///      `compute` closure (per singleflight's synchronous-
///      compute contract), capturing `&dyn ResolverContext` directly. On
///      cooperative-admission failure (revalidation rejected the entry),
///      falls back to an uncached recompute so the caller never sees
///      a publishing miss.
///
/// The cache key is the content-free `RefCycleResultKey`
/// (`ResolvedDeclSlotIdentity` root slot + `resolve_env_hash` + version
/// — R6: no `DeclIdentity`/`whole_hash` in the key); entries store
/// `(result, read_set_signature.facts + self_root_canonicals,
/// validated_at_generation)`. The fact rail is built from the BFS root's
/// self-root plus every visited decl's `FileWholeHash` and the
/// `Instantiate` dispatch fences accumulated during the BFS, so version
/// rooting is value-side and cache invalidation is precise per-canonical
/// (via `RefCycleResultDb::invalidate_for_canonical`) and
/// project-generation-wide (via `invalidate_all`).
///
/// Legacy parity rules carried into the
/// inner BFS body unchanged:
///
/// - Queue carries `(DeclIdentity, path_has_complex_signal: bool)`.
/// - Visited set keyed on `DeclIdentity` (first-visit-wins).
/// - Walks THROUGH bodies with complex surfaces (does NOT stop at
///   them); the complex-signal flag composes through child hops.
/// - Decision rule on self-rediscovery:
///   `cycle_has_complex_signal = body_has_complex_signal || ref_has_type_args`
///   — a self-cycle through a plain object self-member route like
///   `Props['to']` does NOT trigger; only complex helpers do.
/// - `MAX_HOPS = 64`; when the budget is exhausted, returns the
///   path's complex-signal flag (matches legacy fallback).
///
/// Wired in production by B1's materialiser registry-route +
/// recursive-helper guards.
pub(crate) fn ref_root_reaches_transitive_cycle_node(
    root_identity: &crate::semantic_query::DeclIdentity,
    ctx: &dyn ResolverContext,
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
) -> bool {
    let db = ctx.project_type_store().ref_cycle_db();

    // Fast path: peek with generation-local validity. On hit, extend
    // the caller's local_fence and return without dispatching any
    // Instantiate query.
    if let Some(read) = crate::component_meta_caches::ref_cycle_db_peek(db, root_identity, ctx) {
        crate::component_meta_audit::merge_dep_signature_into_local_fence(
            local_fence,
            &read.dep_signature,
        );
        // Type-resolution audit: surface the ref-cycle cache hit on
        // the active request context so the per-request snapshot
        // attributes the hit. Cheap when no context is installed.
        if let Some(req_ctx) = crate::request_context::current_request_context() {
            req_ctx.bump_type_resolution_ref_root_cycle_hit();
        }
        return read.value;
    }

    // Slow path: cooperative-admission with synchronous compute. The
    // closure captures `&dyn ResolverContext` by reference — Rust borrow safe
    // because the query-identity `query::lookup` split-publish path (which
    // `ref_cycle_db_get_or_compute` drives) runs the compute closure on
    // the calling thread (per the synchronous-compute contract documented
    // in `cache_runtime/singleflight.rs`).
    let read_opt = crate::component_meta_caches::ref_cycle_db_get_or_compute(
        db,
        root_identity,
        ctx,
        |compute_fence, observed_self_roots| {
            bfs_compute_inner(root_identity, ctx, compute_fence, observed_self_roots)
        },
    );

    match read_opt {
        Some(read) => {
            crate::component_meta_audit::merge_dep_signature_into_local_fence(
                local_fence,
                &read.dep_signature,
            );
            read.value
        }
        None => {
            // Cooperative admission returned None (revalidation
            // rejected the freshly-built entry). Recompute uncached as
            // a fallback so the caller still sees a result. Do NOT
            // cache: the same revalidation race that just rejected
            // the entry would reject the next attempt too.
            let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
            let mut observed_self_roots: Vec<(Arc<str>, crate::types::Hash16)> = Vec::new();
            let result =
                bfs_compute_inner(root_identity, ctx, &mut fence, &mut observed_self_roots);
            local_fence.extend(fence);
            result
        }
    }
}

/// R — extracted BFS body. Identical legacy-parity
/// logic to `ref_root_reaches_transitive_cycle_node`'s pre-cache body
/// (preserves recursive-ref back-edge detection, intermediate-self
/// check, and `ProjectionMode::Skeleton` for open-generic preservation
/// per §4.21 / R10-2).
///
/// The wrapper [`ref_root_reaches_transitive_cycle_node`] calls this
/// from inside the cooperative-admission `compute` closure on the cold
/// path. The wrapper additionally calls it directly on the
/// uncached-fallback branch when the cooperative admission's
/// revalidation rejects the freshly-built entry.
pub(crate) fn bfs_compute_inner(
    root_identity: &crate::semantic_query::DeclIdentity,
    ctx: &dyn ResolverContext,
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    observed_self_roots: &mut Vec<(Arc<str>, crate::types::Hash16)>,
) -> bool {
    use crate::semantic_query::{ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryKey};
    use rustc_hash::FxHashSet;
    use std::collections::VecDeque;

    const MAX_HOPS: usize = 64;

    #[cfg(test)]
    dep_signature::BFS_COMPUTE_COUNTER.with(|c| c.set(c.get() + 1));

    // Record one observed self-root per visited declaration identity.
    // Each `DeclIdentity` carries an embedded observed `whole_hash`
    // captured when the identity was constructed — an observed
    // identity, NOT a current-content re-read. A real declaring file
    // (non-builtin canonical) becomes a strict self-root: a content
    // edit to it rejects the cached cycle result. The synthetic
    // `__builtin__` carrier identity (and any other empty/synthetic
    // canonical) is skipped — it has no file to root against.
    let record_self_root =
        |identity: &crate::semantic_query::DeclIdentity,
         roots: &mut Vec<(Arc<str>, crate::types::Hash16)>| {
            let canonical = identity.canonical_id.as_ref();
            if canonical.is_empty() || canonical == "__builtin__" {
                return;
            }
            roots.push((Arc::clone(&identity.canonical_id), identity.whole_hash));
        };

    let dispatch = ctx.dispatch();
    let graph = ctx.project_type_store().semantic_graph();
    let mut visited: FxHashSet<crate::semantic_query::DeclIdentity> = FxHashSet::default();
    let mut queue: VecDeque<(crate::semantic_query::DeclIdentity, bool)> = VecDeque::new();
    visited.insert(root_identity.clone());
    record_self_root(root_identity, observed_self_roots);
    queue.push_back((root_identity.clone(), false));

    let mut remaining_hops: usize = MAX_HOPS;
    while let Some((current, path_has_complex_signal)) = queue.pop_front() {
        if remaining_hops == 0 {
            // Legacy parity: fall back to the carried flag rather
            // than blanket-false. Conservative on bounded cyclic
            // chains.
            return path_has_complex_signal;
        }
        remaining_hops -= 1;

        #[cfg(test)]
        dep_signature::BFS_VISITED_COUNTER.with(|c| c.set(c.get() + 1));

        // Clone current's identity for instrumentation AND for the
        // self-cycle / intermediate-self check below. `current` is
        // moved into the SemanticQueryKey on the next line; we keep
        // a clone here for the rest of this iteration.
        let current_identity = current.clone();
        #[cfg(test)]
        let current_decl_name_for_test = Arc::clone(&current.decl_name);

        let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            dispatch.type_slot_for(
                Arc::clone(&current.canonical_id),
                Arc::clone(&current.decl_name),
            ),
            Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            // Skeleton mode preserves open generics so body lowering
            // produces TypeParam graph nodes for T-refs (not
            // Opaque(Miss)). This BFS is a structural guard, not a
            // publication boundary, so keep the Skeleton shape while
            // using StructuralTransit demand to prevent nested mapped
            // operators from emitting member publication edges.
            dispatch.instantiate_context_for(
                &current.canonical_id,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Skeleton,
                ),
            ),
        ));
        let read = dispatch.execute_read(key);
        crate::request_context::observe_component_meta_read_suppress(&read);
        crate::component_meta_audit::merge_dep_signature_into_local_fence(
            local_fence,
            &read.dep_signature,
        );
        let body_id = match read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) | QueryResult::Error(_) => continue,
        };

        let body_has_complex_signal =
            path_has_complex_signal || has_complex_cycle_guard_surface_node(ctx, body_id, 0);

        // Dispatch's recursive-ref back-edge is published as
        // `Opaque(RecursiveRef { name })` — not a DeclRef — so a
        // pure-graph walk would miss the self-cycle. Detect it
        // explicitly: any `Opaque(RecursiveRef { name })` whose name
        // matches the BFS root's decl_name is a back-edge to root,
        // and the body already carries complex_signal (the body
        // contained a recursive carrier, which is exactly the
        // canonical complex-cycle-guard pattern via DeclRef /
        // InstantiationRef arms).
        if body_contains_recursive_ref_to_name(graph, body_id, &root_identity.decl_name, 0) {
            // The recursive-ref back-edge IS the cycle. Compose the
            // signal: body_has_complex_signal already carries the
            // complex shape; if the body wraps the back-edge in any
            // complex shape (Union/IndexedAccess/Conditional/etc),
            // body_has_complex_signal is true and we report the cycle.
            if body_has_complex_signal {
                return true;
            }
        }

        let mut child_refs: Vec<(crate::semantic_query::DeclIdentity, bool)> = Vec::new();
        collect_ref_identities_node(graph, body_id, &mut child_refs, 0);

        // F-prep test instrumentation. Records
        // child_refs.len() per visited identity name into the per-thread
        // observer (no-op when no observer installed).
        #[cfg(test)]
        record_bfs_child_refs_count_for_test(current_decl_name_for_test.as_ref(), child_refs.len());

        for (child_identity, ref_has_type_args) in child_refs {
            let cycle_has_complex_signal = body_has_complex_signal || ref_has_type_args;
            // Cycle is reported when:
            //  (a) child == root (transitive cycle back to BFS root), OR
            //  (b) child == current (intermediate self-reference at this
            //      decl — match `ref_name == name` against the
            //      CURRENT decl, not the root). This catches fixtures
            //      where DotPathKeys's body recursively references
            //      DotPathKeys via a complex helper surface (canonical
            //      nuxt-ui DotPathKeys).
            if cycle_has_complex_signal
                && (&child_identity == root_identity || child_identity == current_identity)
            {
                return true;
            }
            if visited.insert(child_identity.clone()) {
                record_self_root(&child_identity, observed_self_roots);
                queue.push_back((child_identity, cycle_has_complex_signal));
            }
        }
    }
    false
}

/// Helper: returns `true` when `node`'s shallow surface contains a
/// `SemanticNodeData::Opaque(QueryError::RecursiveRef { name })`
/// matching `target_name`. Used by
/// [`ref_root_reaches_transitive_cycle_node`] to detect dispatch's
/// recursive-ref back-edges (the dispatch engine collapses self-
/// references into an `Opaque(RecursiveRef)` sentinel rather than
/// a regular DeclRef, so a pure-graph walk would miss them).
///
/// Walks the same shallow shapes as
/// [`collect_ref_identities_node`]; depth-fused at 256.
pub(crate) fn body_contains_recursive_ref_to_name(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    target_name: &Arc<str>,
    depth: u32,
) -> bool {
    use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId};
    use rustc_hash::FxHashSet;

    if depth > 256 {
        return false;
    }

    let mut stack: Vec<SemanticNodeId> = vec![node];
    let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(data) = graph.node_data(current) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::Opaque(QueryError::RecursiveRef { name }) => {
                if name == target_name {
                    return true;
                }
            }
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                for &m in members.iter() {
                    stack.push(m);
                }
            }
            SemanticNodeData::Object(surface) => {
                for member in surface.members.iter() {
                    stack.push(member.value);
                }
                for sig in surface.index_signatures.iter() {
                    stack.push(sig.key_type);
                    stack.push(sig.value_type);
                }
                for &call in surface.call_signatures.iter() {
                    stack.push(call);
                }
                for &cons in surface.construct_signatures.iter() {
                    stack.push(cons);
                }
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter() {
                    stack.push(element.value);
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                stack.push(*object);
                if let crate::semantic_query::IndexKey::TypeNode(idx_node) = index {
                    stack.push(*idx_node);
                }
            }
            SemanticNodeData::KeyOf { base } => stack.push(*base),
            SemanticNodeData::Function {
                params,
                return_type,
                ..
            } => {
                for param in params.iter() {
                    stack.push(param.ty);
                }
                stack.push(*return_type);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                stack.push(*check);
                stack.push(*extends);
                stack.push(*true_branch_ref);
                stack.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                stack.push(*source);
                stack.push(mapper.key_space);
                stack.push(mapper.value_expr);
                if let Some(remap) = mapper.name_remap {
                    stack.push(remap);
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for &expr in expressions.iter() {
                    stack.push(expr);
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => {
                for &arg in args.iter() {
                    stack.push(arg);
                }
            }
            // Carrier `type_args` are descended (args-only): a carrier's
            // applied arguments can carry an `Opaque(RecursiveRef)` back-edge.
            // The carrier head is not inspected (head resolution is separate).
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::ImportType(_) => {
                for &arg in data.carrier_type_args().iter() {
                    stack.push(arg);
                }
            }
            _ => {}
        }
    }
    false
}

/// Helper: walker-parity check for "complex" cycle-guard surfaces.
/// R7-13 legacy parity: a body whose top shape is something other
/// than a plain Object / Function / Array / Tuple / Primitive /
/// Literal / TypeParameter / Infer counts as "complex".
///
/// `depth` fuses recursion at 256 to bound runtime on pathological
/// graphs. The fuse intentionally returns `false` on
/// hit — a runaway recursion is treated as "not complex" so the
/// caller continues the BFS rather than terminating prematurely.
pub(crate) fn has_complex_cycle_guard_surface_node(
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
        SemanticNodeData::Alias(inner) => {
            has_complex_cycle_guard_surface_node(ctx, *inner, depth + 1)
        }
        SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
            members
                .iter()
                .any(|&m| has_complex_cycle_guard_surface_node(ctx, m, depth + 1))
                || members.iter().any(|&m| {
                    let d = graph.node_data(m);
                    !matches!(d.as_deref(), Some(SemanticNodeData::Object(_)))
                })
        }
        SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::TemplateLiteral { .. } => true,
        _ => false,
    }
}

/// Helper: collect every reachable `DeclRef` / `InstantiationRef`
/// identity from `node`'s declaration body, paired with whether the
/// reference carries type arguments. Walker-parity (R7-14): walks
/// THROUGH every TypeExpr-like shape that could carry a Ref —
/// Conditional / Mapped / TemplateLiteral / Object members + index
/// signatures + call/construct/method signatures / Function
/// parameters + return / Tuple elements / IndexedAccess(index +
/// object) / KeyOf / Array / Alias. Aggressive collection — never
/// stops at "complex" body shapes (those are the cycle indicator,
/// not the termination signal).
///
/// `depth` fuses recursion at 256. The fuse returns
/// without recording new identities to bound runtime on
/// pathological graphs.
pub(crate) fn collect_ref_identities_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    out: &mut Vec<(crate::semantic_query::DeclIdentity, bool)>,
    depth: u32,
) {
    use crate::semantic_query::{SemanticNodeData, SemanticNodeId};
    use rustc_hash::FxHashSet;

    if depth > 256 {
        return;
    }

    let mut stack: Vec<SemanticNodeId> = vec![node];
    let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(data) = graph.node_data(current) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                // Bare DeclRef has no type arguments — false.
                out.push((identity.clone(), false));
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let ref_has_type_args = !args.is_empty();
                out.push((base.clone(), ref_has_type_args));
                for &arg in args.iter() {
                    stack.push(arg);
                }
            }
            SemanticNodeData::Alias(inner) => stack.push(*inner),
            SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                for &m in members.iter() {
                    stack.push(m);
                }
            }
            SemanticNodeData::Object(surface) => {
                // Members hold property/method bodies.
                for member in surface.members.iter() {
                    stack.push(member.value);
                }
                // Index signatures expose key + value types.
                for sig in surface.index_signatures.iter() {
                    stack.push(sig.key_type);
                    stack.push(sig.value_type);
                }
                // Call / construct signatures publish as Function nodes.
                for &call in surface.call_signatures.iter() {
                    stack.push(call);
                }
                for &cons in surface.construct_signatures.iter() {
                    stack.push(cons);
                }
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                for element in elements.iter() {
                    stack.push(element.value);
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                stack.push(*object);
                if let crate::semantic_query::IndexKey::TypeNode(idx_node) = index {
                    stack.push(*idx_node);
                }
            }
            SemanticNodeData::KeyOf { base } => stack.push(*base),
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                for param in params.iter() {
                    stack.push(param.ty);
                }
                stack.push(*return_type);
                for tp in type_parameters.iter() {
                    if let Some(c) = tp.constraint {
                        stack.push(c);
                    }
                    if let Some(d) = tp.default {
                        stack.push(d);
                    }
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                stack.push(*check);
                stack.push(*extends);
                stack.push(*true_branch_ref);
                stack.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, mapper } => {
                stack.push(*source);
                stack.push(mapper.key_space);
                stack.push(mapper.value_expr);
                if let Some(remap) = mapper.name_remap {
                    stack.push(remap);
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for &expr in expressions.iter() {
                    stack.push(expr);
                }
            }
            // Carrier `type_args` are descended (args-only): a `BareRef` /
            // `TypeOf` / `ImportType` carrier applies its arguments at the
            // reference site, and an arg can carry a `DeclRef` /
            // `InstantiationRef` (a declaration edge). The carrier HEAD is NOT
            // collected here — a `BareRef` / `ImportType` head is unresolved
            // (no decl identity) and a `TypeOf` head is a value root (no decl
            // identity); head resolution is a separate concern.
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::ImportType(_) => {
                for &arg in data.carrier_type_args().iter() {
                    stack.push(arg);
                }
            }
            _ => {}
        }
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
/// A real miss collects no root. `MAX_*` caps bound the collection.
fn collect_node_root_identities(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
    out: &mut Vec<crate::semantic_query::DeclIdentity>,
) {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticNodeData};
    const MAX_CYCLE_ROOTS: usize = 16;
    const MAX_ROOT_COLLECT_DEPTH: u32 = 8;
    if out.len() >= MAX_CYCLE_ROOTS || depth >= MAX_ROOT_COLLECT_DEPTH {
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
        Step::Recurse(next) => collect_node_root_identities(dispatch, next, depth + 1, out),
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
                collect_node_root_identities(dispatch, arg, depth + 1, out);
            }
        }
        Step::ResolveBare => {
            let resolved = dispatch.resolve_carrier_subject_node(
                node,
                ProjectionReductionContext::published(ProjectionMode::Navigate),
            );
            if resolved != node {
                collect_node_root_identities(dispatch, resolved, depth + 1, out);
            }
        }
        Step::Stop => {}
    }
}

/// Node-domain front for the transitive-cycle gate. Collects the surface root
/// identities of `node` ([`collect_node_root_identities`]) and ORs the shared
/// cached BFS ([`ref_root_reaches_transitive_cycle_node`]) over each, merging
/// each root's cross-file fence entry (the scope self-entry and `__builtin__`
/// roots are skipped) into the BFS-visited fence, which is dual-emitted via
/// `emit_dispatch_dep_signature_facts`. Takes `ctx` (the node carries resolved
/// identities, so no name-resolution engine is needed).
pub(crate) fn node_root_reaches_transitive_cycle_with_fence(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    node: crate::semantic_query::SemanticNodeId,
) -> (bool, crate::semantic_query::DepSignature) {
    let mut roots: Vec<crate::semantic_query::DeclIdentity> = Vec::new();
    {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        collect_node_root_identities(&dispatch, node, 0, &mut roots);
    }
    if roots.is_empty() {
        return (false, Arc::from(Vec::new()));
    }
    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let mut result = false;
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
        result |= ref_root_reaches_transitive_cycle_node(identity, ctx, &mut fence);
    }
    let fence_signature: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
    crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts(ctx, &fence_signature);
    (result, fence_signature)
}

#[cfg(test)]
#[path = "graph_predicates_tests.rs"]
mod graph_predicates_tests;
