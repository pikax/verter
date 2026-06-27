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
        RouteDemand::Pick(keys)
    } else {
        RouteDemand::Omit(keys)
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
                    route: RouteDemand::MemberPath(hops_reverse),
                });
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                // R8-2 — preserve generic root carriers like
                // `Foo<T>['a']`.
                hops_reverse.reverse();
                return Some(RouteExtraction {
                    root_identity: base.clone(),
                    root_args: Arc::clone(args),
                    route: RouteDemand::MemberPath(hops_reverse),
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

        let key = SemanticQueryKey::Instantiate {
            base: dispatch.type_slot_for(
                Arc::clone(&current.canonical_id),
                Arc::clone(&current.decl_name),
            ),
            args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            // Skeleton mode preserves open generics so body lowering
            // produces TypeParam graph nodes for T-refs (not
            // Opaque(Miss)). This BFS is a structural guard, not a
            // publication boundary, so keep the Skeleton shape while
            // using StructuralTransit demand to prevent nested mapped
            // operators from emitting member publication edges.
            context: dispatch.instantiate_context_for(
                &current.canonical_id,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Skeleton,
                ),
            ),
        };
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

/// Read a ROOT declaration NAME from a graph `node`, mirroring the `TypeExpr`
/// front's `root_name` extraction in
/// [`crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence`]:
///
/// - `Alias(inner)` — pass-through (graph-native; `Parenthesized` equivalent).
/// - `IndexedAccess { object, .. }` — recurse the indexed-access root.
/// - `Pick`/`Omit` builtin `InstantiationRef` (2 args) — the SOURCE root name
///   from `args[0]`, NOT the `__builtin__::Pick` wrapper (the source-root trap).
/// - `DeclRef` / `InstantiationRef` — the carried declaration name.
/// - `BareRef` — the unresolved head name.
/// - anything else — `None`.
///
/// `raise(node)` of a `DeclRef` is `Ref { name: decl_name }` and of a
/// `Pick`/`Omit` instantiation is `Ref { name: "Pick", type_arguments }`, so this
/// returns exactly what `root_name(raise(node))` would — parity by construction
/// with the shared resolution tail.
fn node_root_name(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> Option<String> {
    use crate::semantic_query::SemanticNodeData;
    if depth > 256 {
        return None;
    }
    let data = graph.node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => node_root_name(graph, *inner, depth + 1),
        SemanticNodeData::IndexedAccess { object, .. } => node_root_name(graph, *object, depth + 1),
        SemanticNodeData::InstantiationRef { base, args }
            if base.canonical_id.as_ref() == "__builtin__"
                && matches!(base.decl_name.as_ref(), "Pick" | "Omit")
                && args.len() == 2 =>
        {
            node_ref_name(graph, args[0])
        }
        SemanticNodeData::DeclRef { identity } => Some(identity.decl_name.to_string()),
        SemanticNodeData::InstantiationRef { base, .. } => Some(base.decl_name.to_string()),
        data if data.bare_ref_head().is_some() => {
            data.bare_ref_head().map(|head| head.0.to_string())
        }
        _ => None,
    }
}

/// The declaration NAME of a single reference node (`DeclRef` / `InstantiationRef`
/// / `BareRef`), used for the `Pick`/`Omit` source root (`args[0]`). Mirrors
/// `component_meta_registry_ref_name(&type_arguments[0])` on the `TypeExpr` front
/// (which returns the source `Ref`'s name); a non-reference source yields `None`.
fn node_ref_name(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
) -> Option<String> {
    use crate::semantic_query::SemanticNodeData;
    let data = graph.node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => Some(identity.decl_name.to_string()),
        SemanticNodeData::InstantiationRef { base, .. } => Some(base.decl_name.to_string()),
        data if data.bare_ref_head().is_some() => {
            data.bare_ref_head().map(|head| head.0.to_string())
        }
        _ => None,
    }
}

/// Node-domain front for the package-backed object-like-root gate. Extracts the
/// root declaration name from `node` ([`node_root_name`], which handles the
/// `Pick`/`Omit` source-root trap and indexed-access roots) and feeds it through
/// the SHARED `TypeExpr` resolution + object-like + fence tail
/// ([`crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence`])
/// via a synthetic `Ref { name }` — so the verdict + fence are computed by the
/// SAME body as the `TypeExpr` front (parity by construction: the only difference
/// is where the root name is read from). A node with no extractable root name is
/// not package-backed (empty fence — admittable, like the `TypeExpr` front's
/// `None` root arm).
pub(crate) fn node_package_backed_object_like_root_with_fence(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    node: crate::semantic_query::SemanticNodeId,
) -> (bool, Option<crate::semantic_query::DepSignature>) {
    let graph = Arc::clone(query_engine.ctx.project_type_store().semantic_graph());
    let Some(root_name) = node_root_name(&graph, node, 0) else {
        return (false, Some(Arc::from(Vec::new())));
    };
    drop(graph);
    crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence(
        &verter_type_expr::TypeExpr::named(root_name.as_str()),
        scope_canonical_id,
        query_engine,
    )
}

/// Collect the SURFACE root declaration identities of a graph `node`, mirroring
/// `collect_root_decl_identities` in the `TypeExpr` front
/// ([`crate::meta_resolve::lowered_root_reaches_transitive_cycle_with_fence`]):
/// the outer carrier's identity plus every type-argument's identity, descending
/// only `Alias` / `IndexedAccess.object` / `InstantiationRef.args`. The node
/// carrier already holds the RESOLVED `DeclIdentity` (`DeclRef.identity` /
/// `InstantiationRef.base`), so no name re-resolution is needed; an unresolved
/// `BareRef` head carries no identity and is not rooted (a missing body cannot
/// close a cycle). `MAX_*` caps mirror the `TypeExpr` front.
fn collect_node_root_identities(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
    out: &mut Vec<crate::semantic_query::DeclIdentity>,
) {
    use crate::semantic_query::SemanticNodeData;
    const MAX_CYCLE_ROOTS: usize = 16;
    const MAX_ROOT_COLLECT_DEPTH: u32 = 8;
    if out.len() >= MAX_CYCLE_ROOTS || depth >= MAX_ROOT_COLLECT_DEPTH {
        return;
    }
    let Some(data) = graph.node_data(node) else {
        return;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            collect_node_root_identities(graph, *inner, depth + 1, out)
        }
        SemanticNodeData::IndexedAccess { object, .. } => {
            collect_node_root_identities(graph, *object, depth + 1, out)
        }
        SemanticNodeData::DeclRef { identity } => {
            if !out.contains(identity) {
                out.push(identity.clone());
            }
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            if !out.contains(base) {
                out.push(base.clone());
            }
            for &arg in args.iter() {
                collect_node_root_identities(graph, arg, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Node-domain front for the transitive-cycle gate. Collects the surface root
/// identities of `node` ([`collect_node_root_identities`]) and ORs the shared
/// cached BFS ([`ref_root_reaches_transitive_cycle_node`]) over each, merging the
/// fence exactly as the `TypeExpr` front
/// ([`crate::meta_resolve::lowered_root_reaches_transitive_cycle_with_fence`])
/// does — `raise(node)` raises each carrier to its `Ref`, whose
/// `collect_root_decl_identities` resolves the SAME identity the node already
/// carries, so the per-root BFS verdicts (and the BFS-visited fence) match by
/// construction. Takes `ctx` (the node carries resolved identities, so no
/// name-resolution engine is needed).
pub(crate) fn node_root_reaches_transitive_cycle_with_fence(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    node: crate::semantic_query::SemanticNodeId,
) -> (bool, crate::semantic_query::DepSignature) {
    let graph = Arc::clone(ctx.project_type_store().semantic_graph());
    let mut roots: Vec<crate::semantic_query::DeclIdentity> = Vec::new();
    collect_node_root_identities(&graph, node, 0, &mut roots);
    drop(graph);
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
mod node_root_gate_differential_tests {
    //! DIFFERENTIAL EQUIVALENCE: the node-domain root gates equal the `TypeExpr`
    //! fronts, field-for-field (verdict AND fence), on inputs that genuinely reach
    //! each path — the `Pick`/`Omit` package SOURCE-root trap, an indexed-access
    //! root, a bare package ref, a workspace-local ref, a non-ref, and a transitive
    //! generic cycle.

    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::{
        node_package_backed_object_like_root_with_fence,
        node_root_reaches_transitive_cycle_with_fence,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::{DependencyResolution, VerterHost};

    fn lower(host: &VerterHost, scope: &str, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode(scope, expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    #[test]
    fn node_package_backed_root_matches_type_expr_front_field_for_field() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from("export interface VendorProps { a: string; b: number }\n"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script lang=\"ts\">\n\
                 import type { VendorProps } from 'pkg'\n\
                 export interface LocalProps { x: string }\n\
                 </script>\n<template><div /></template>",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        let scope = "/src/App.vue";

        let cases: Vec<TypeExpr> = vec![
            // Pick over a PACKAGE source — the source-root trap (must inspect the
            // VendorProps source, not the `__builtin__::Pick` wrapper).
            TypeExpr::named_with_args(
                "Pick",
                vec![
                    TypeExpr::named("VendorProps"),
                    TypeExpr::string_literal("a"),
                ],
            ),
            // indexed-access over the package source
            TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("VendorProps")),
                index: Arc::new(TypeExpr::string_literal("a")),
            },
            // bare package ref (interface ⇒ object-like)
            TypeExpr::named("VendorProps"),
            // workspace-local ref (NOT package-backed)
            TypeExpr::named("LocalProps"),
            // non-ref root (no extractable root name)
            TypeExpr::Primitive(PrimitiveName::String),
        ];

        let mut any_true = false;
        let mut any_false = false;
        for expr in &cases {
            let node = lower(&host, scope, expr);
            let mut qe_node = ComponentMetaQueryEngine::new(&host);
            let node_result =
                node_package_backed_object_like_root_with_fence(&mut qe_node, scope, node);
            let mut qe_expr = ComponentMetaQueryEngine::new(&host);
            let expr_result =
                crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence(
                    expr, scope, &mut qe_expr,
                );
            assert_eq!(
                node_result, expr_result,
                "node package-backed gate must equal the TypeExpr front (verdict + fence) for {expr:?}"
            );
            if node_result.0 {
                any_true = true;
            } else {
                any_false = true;
            }
        }
        // Genuine reach: the package source IS package-backed; the local ref is NOT
        // — the cases are not vacuously all-equal.
        assert!(
            any_true && any_false,
            "the differential must exercise BOTH a package-backed root and a non-package-backed \
             one (genuine reach), not a single verdict"
        );
    }

    #[test]
    fn node_transitive_cycle_matches_type_expr_front() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from(
                "export type A<T> = B<T>\n\
                 export type B<T> = A<T>\n\
                 export type C<T> = { v: T }\n",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        let scope = "/src/m.ts";

        // cyclic generic (A<string> → B<string> → A<string>) and a non-cyclic one.
        let cyclic =
            TypeExpr::named_with_args("A", vec![TypeExpr::Primitive(PrimitiveName::String)]);
        let acyclic =
            TypeExpr::named_with_args("C", vec![TypeExpr::Primitive(PrimitiveName::String)]);

        for (expr, expect_cycle) in [(&cyclic, true), (&acyclic, false)] {
            let node = lower(&host, scope, expr);
            let node_cycle = node_root_reaches_transitive_cycle_with_fence(&host, scope, node).0;
            let mut qe = ComponentMetaQueryEngine::new(&host);
            let expr_cycle = crate::meta_resolve::lowered_root_reaches_transitive_cycle_with_fence(
                &mut qe, scope, expr,
            )
            .0;
            assert_eq!(
                node_cycle, expr_cycle,
                "node cycle gate must equal the TypeExpr front for {expr:?}"
            );
            assert_eq!(
                node_cycle, expect_cycle,
                "case {expr:?} must GENUINELY reach the expected cycle verdict (not vacuous)"
            );
        }
    }
}

#[cfg(test)]
mod carrier_descent_tests {
    //! Carrier-arg descent for the cycle-BFS ref/recursive-ref walkers.
    //!
    //! `collect_ref_identities_node` and `body_contains_recursive_ref_to_name`
    //! walk a lowered body's structural children to discover declaration
    //! references and recursive-ref back-edges. A `BareRef` / `TypeOf` /
    //! `ImportType` carrier applies its `type_args` at the reference site; those
    //! args can themselves carry a `DeclRef` / `InstantiationRef` (a real
    //! cross-decl edge) or an `Opaque(RecursiveRef)` (a cycle back-edge). The
    //! walkers MUST descend `SemanticNodeData::carrier_type_args` so those
    //! identities / back-edges are not silently dropped — a missed edge would
    //! under-collect the cycle graph and let a genuine cycle escape the guard.
    //!
    //! Each test DIRECT-CONSTRUCTS a carrier (no head resolution — that is the
    //! producer's job) and asserts only the DESCENT into its args. Discrimination
    //! is the negative assertion: against the pre-descent `_ => {}` arm the
    //! identity / back-edge is missed.

    use std::sync::Arc;

    use crate::semantic_query::{
        DeclIdentity, NodeScopeId, QueryError, ScopeId, SemanticNodeData, SemanticNodeId,
        ValueRootKey,
    };
    use crate::semantic_query_memo::SemanticGraphStore;

    use super::{body_contains_recursive_ref_to_name, collect_ref_identities_node};

    fn decl_identity(canonical: &str, name: &str) -> DeclIdentity {
        DeclIdentity::from_scope(
            &NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                whole_hash: [7u8; 16],
                local_scope: None,
            },
            Arc::from(name),
        )
    }

    /// Build the three carriers, each wrapping `arg` as its single `type_args`
    /// entry, so a single descent assertion covers all three carrier kinds.
    fn carriers_wrapping(graph: &SemanticGraphStore, arg: SemanticNodeId) -> Vec<SemanticNodeId> {
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        vec![
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                NodeScopeId::Global,
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/v.ts"),
                        local_scope: None,
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::new().into_boxed_slice()),
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                Arc::clone(&args),
                false,
            )),
        ]
    }

    // ── D1 — collect_ref_identities_node descends carrier args ──────────────
    //
    // A `DeclRef` (and an `InstantiationRef`) inside a carrier's `type_args` IS
    // a declaration edge. `collect_ref_identities_node` must collect it.
    // NEGATIVE: with the unchanged `_ => {}` arm the carrier is a leaf and the
    // identity is missed (the collected set would be empty).
    #[test]
    fn collect_ref_identities_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let inner_id = decl_identity("/dep.ts", "Inner");
        let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: inner_id.clone(),
        });

        for carrier in carriers_wrapping(&graph, decl_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            collect_ref_identities_node(&graph, carrier, &mut out, 0);
            assert!(
                out.iter().any(|(id, _)| *id == inner_id),
                "a DeclRef inside a carrier's type_args must be collected; got {out:?} for \
                 carrier {:?}",
                graph.node_data(carrier).as_deref()
            );
        }

        // InstantiationRef arg variant — the base identity is collected with
        // `has_type_args = true`.
        let inst_base = decl_identity("/dep.ts", "Box");
        let inst_ref = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: inst_base.clone(),
            args: Arc::from(Vec::new().into_boxed_slice()),
        });
        for carrier in carriers_wrapping(&graph, inst_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            collect_ref_identities_node(&graph, carrier, &mut out, 0);
            assert!(
                out.iter().any(|(id, _)| *id == inst_base),
                "an InstantiationRef inside a carrier's type_args must be collected; got {out:?}"
            );
        }
    }

    // ── D2 — body_contains_recursive_ref_to_name descends carrier args ──────
    //
    // An `Opaque(RecursiveRef { name })` inside a carrier's `type_args` is a
    // cycle back-edge to `name`. NEGATIVE: with the unchanged `_ => {}` arm the
    // carrier is a leaf and the predicate returns `false`.
    #[test]
    fn body_contains_recursive_ref_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let target: Arc<str> = Arc::from("SelfRef");
        let rec = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::clone(&target),
        }));

        for carrier in carriers_wrapping(&graph, rec) {
            assert!(
                body_contains_recursive_ref_to_name(&graph, carrier, &target, 0),
                "a RecursiveRef back-edge inside a carrier's type_args must be found for `{}`; \
                 carrier {:?}",
                target,
                graph.node_data(carrier).as_deref()
            );
        }

        // NEGATIVE control: a carrier whose args contain a RecursiveRef to a
        // DIFFERENT name does NOT match the target (proving the descent reads
        // the actual name, not a blanket true).
        let other = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::from("OtherName"),
        }));
        for carrier in carriers_wrapping(&graph, other) {
            assert!(
                !body_contains_recursive_ref_to_name(&graph, carrier, &target, 0),
                "a carrier whose args reference a DIFFERENT name must NOT match the target"
            );
        }
    }
}
