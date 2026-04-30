//! Graph-native registry-route + cycle-BFS predicates.
//!
//! Phase 11a domain 12 — owns the §1.12 graph-native variants of the
//! TypeExpr-based registry-route helpers, the package-ref check, and
//! the cycle-BFS predicates that gate ref/recursive-ref termination:
//!
//! - `RouteExtraction` struct + `extract_route_root_identity_node`,
//!   `extract_pick_omit_route`, `build_keys_union_node`,
//!   `extract_indexed_access_route`, `collect_string_literal_union_keys_node`
//!   (route extraction).
//! - `canonical_resolves_to_package`,
//!   `component_meta_ref_resolves_to_package_node` (package-ref check).
//! - `type_node_needs_member_route_materialization`,
//!   `node_has_non_object_top_level_surface`,
//!   `slot_binding_param_can_stay_symbolic_node`,
//!   `type_node_has_package_backed_root`,
//!   `declaration_body_prefers_inline_materialization_node`
//!   (graph-native predicates the materializer / walker call).
//! - `ref_root_reaches_transitive_cycle_node`, `bfs_compute_inner`,
//!   `body_contains_recursive_ref_to_name`,
//!   `has_complex_cycle_guard_surface_node`, `collect_ref_identities_node`
//!   (cycle-BFS termination + recursive-ref reachability).
//!
//! Lines 152-1384 of the post-commit-11 `meta_resolve.rs` shell.
//! Visibility escalation: the formerly-private free fns are escalated
//! to `pub(crate)` so the host_methods.rs / macro_member_walk.rs /
//! materialize/* / registry_materialize.rs siblings keep calling them
//! via the shell's `pub(crate) use graph_predicates::*;` re-export.

use crate::VerterHost;
use std::sync::Arc;

// `dep_signature` module is accessed via path (`dep_signature::BFS_*_COUNTER`)
// inside `bfs_compute_inner` for thread-local counter access.
use super::dep_signature;
#[cfg(test)]
use super::dep_signature::record_bfs_child_refs_count_for_test;

/// Plan §6.14 / L — graph-native variant of
/// [`component_meta_registry_prefers_structural_materialization`].
///
/// Returns `true` when `node`'s top-level shape is one the materializer
/// should expand structurally rather than preserve as a reference.
///
/// Mirrors the TypeExpr predicate's classification:
///
/// - **Structural (returns `true`):** `Array`, `Tuple`, `Union`,
///   `Intersection`, `Conditional`, `Mapped`, `TemplateLiteral`,
///   `Function`, `KeyOf` — these shapes need structural expansion to
///   render meaningful component-meta surface.
/// - **Reference-shaped (returns `false`):** `DeclRef`,
///   `InstantiationRef`, `Object`, `IndexedAccess`, `Primitive`,
///   `Literal`, `Opaque`, `TypeOf`, `TypeParam` — these shapes are
///   either already concrete (Object, Primitive) or are
///   reference-carrying (DeclRef, IndexedAccess) and the materializer
///   handles them via dedicated paths.
/// - **Pass-through:** `Alias(inner)` — graph-native shape with no
///   TypeExpr counterpart; matches the TypeExpr predicate's
///   `Parenthesized(inner)` arm semantics (recurse through wrapper).
///
/// `depth` is fused at 256 per §4.11. Fuse returns `false`
/// (conservative — runaway recursion does NOT route through the
/// structural-materialisation fast path).
// Plan §1.12 — graph-native registry-route + cycle-BFS predicates.
//
// These `_node` variants operate on `SemanticNodeId` directly instead of
// round-tripping through `TypeExpr`. They share the round-7 parity
// tightenings with the TypeExpr-based originals: Pick/Omit `args.len() == 2`,
// bare DeclRef root only, literal-string keys only; IndexedAccess uses
// `IndexKey::String` only with a bare DeclRef root.
//
// The TypeExpr-based originals (extract_route_root_identity-equivalent,
// the TypeExpr package-ref check, ...) are retained — they still
// have non-walker call sites per plan §11.2. The materialiser entry will be
// repointed at the `_node` predicates after non-walker callers migrate.
// ===========================================================================

/// Plan §1.12 / §4.4 — return type for [`extract_route_root_identity_node`].
///
/// Pairs the bare-root declaration identity with the route shape that
/// the Pick/Omit/IndexedAccess wrapping carries. Distinct from the
/// TypeExpr-based `(String, RouteDemand)` tuple in the existing
/// `component_meta_registry_public_*_route` helpers because
/// `DeclIdentity` carries the full canonical-id + whole-hash pair the
/// graph layer needs for dispatch keys and package-ref checks.
///
/// Plan §4.4 / Codex2 P0 #3 — `root_args` preserves the generic root
/// carrier's type arguments so `Pick<Foo<T>, 'a'>` and `Foo<T>['a']`
/// shapes can project. Empty for bare-DeclRef roots; non-empty for
/// `InstantiationRef` roots (i.e., the original generic shell).
#[derive(Debug, Clone)]
pub(crate) struct RouteExtraction {
    pub root_identity: crate::semantic_query::DeclIdentity,
    pub root_args: Arc<[crate::semantic_query::SemanticNodeId]>,
    pub route: crate::resolver_core::RouteDemand,
}

/// Plan §1.12 / §4.4 — graph-native variant of the `TypeExpr`-based
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
///   (generic root preserved via `root_args` per Codex2 P0 #3 / R8-2).
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
/// Round-7 parity tightenings:
///
/// - Userland Pick/Omit (a userland `Pick`/`Omit` decl that shadows
///   the builtin) is NOT a registry route — only `__builtin__` Pick/
///   Omit dispatch through this branch.
/// - 1-arg / 3-arg `Pick` rejected: `args.len() != 2` returns `None`.
/// - Empty union rejected: `Pick<Foo, never>` returns `None`.
/// - Numeric/type indices rejected: `Foo[0]` and `Foo[K]` return `None`.
///
/// `depth` fuses recursion at 256 to bound runtime on adversarial
/// inputs (Plan §4.11).
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
/// Plan §4.4 / Codex2 P0 #3 — preserves generic root carriers: when
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
        // Plan §4.4 / R8-1 — symbolic-keep behavior for non-ref roots
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

/// Plan §4.4 — build a string-literal-union node from a list of keys
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
    use verter_semantic::analysis::type_expr::LiteralValue;

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
/// generic carriers are preserved via `root_args` per Codex2 P0 #3.
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
                    // Round-7 parity: numeric/type indices are not
                    // legal route hops.
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
                // Codex2 P0 #3 — preserve generic root carriers like
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
    use verter_semantic::analysis::type_expr::LiteralValue;

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
// were retired in B1: `extract_indexed_access_route` now walks the
// chain inline (preserving generic root carriers via `root_args`),
// and `extract_pick_omit_route` recurses into `args[0]` directly to
// find the actual root identity (R8-2 fix). Both functions had
// `_node` allow_dead_code annotations and no remaining production
// callers; deleted to keep the surface minimal.

/// Plan §6.4 / C — primitive package-detection check on a canonical
/// id. Returns `true` when the canonical resolves under
/// `/node_modules/`. Shared by the graph-native predicate
/// (`component_meta_ref_resolves_to_package_node`) and the
/// node-based shape check (`is_package_backed_ref` in the
/// materialiser).
pub(crate) fn canonical_resolves_to_package(canonical_id: &str) -> bool {
    canonical_id.contains("/node_modules/")
}

/// Plan §1.12 — graph-native variant of the TypeExpr package-ref
/// check. Delegates to the primitive
/// [`canonical_resolves_to_package`] (commit C).
pub(crate) fn component_meta_ref_resolves_to_package_node(
    identity: &crate::semantic_query::DeclIdentity,
) -> bool {
    canonical_resolves_to_package(identity.canonical_id.as_ref())
}

/// Plan §1.12 / J1 — graph-native predicate (former TypeExpr
/// counterpart deleted in Plan §6.15 / N). Returns `true` when the
/// input node's shape requires member-route materialisation (i.e., a
/// non-package-backed reference target that has not been determined
/// to participate in a transitive cycle).
///
/// Mirrors the TypeExpr predicate's branch structure:
///
/// - `DeclRef { identity }` (the no-args case — `Ref { name, [] }`):
///   returns `!component_meta_ref_resolves_to_package_node(identity)`.
/// - `InstantiationRef { .. }` (the with-args case): returns `false`,
///   matching `type_arguments.is_empty() == false`.
/// - `TypeOf { .. } | IndexedAccess { .. } | TypeParam { .. }`:
///   `!cycle && !package_backed`. The cycle check uses
///   [`extract_route_root_identity_node`] to find a root identity for
///   the BFS — when no identity can be extracted (e.g., bare `TypeOf`
///   or `TypeParam`), the cycle check is `false` (matching the legacy
///   adapter behaviour at non-Ref tops). The package check delegates to
///   [`type_node_has_package_backed_root`] (J0).
/// - `Array { element, .. } | KeyOf { base }`: recurse into the carrier
///   (matches `TypeExpr::Array { element }`, `TypeExpr::KeyOf(element)`).
/// - `Tuple { elements }`: any element flips the predicate (matches
///   `TypeExpr::Tuple { elements }`).
/// - `Alias(inner)`: pass-through (graph-native shape).
/// - All other shapes: `false`.
///
/// `local_fence` accumulates dep-signature facts produced by the cycle
/// BFS so the caller's completion fence remains complete.
///
/// `depth` is fused at 256 to bound runtime on pathological chains
/// (Plan §4.11). Fuse returns `false` to match the conservative legacy
/// behaviour.
pub(crate) fn type_node_needs_member_route_materialization(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    depth: u32,
) -> bool {
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return false;
    }
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        // Lowered `Ref { name, type_arguments: [] }` — needs
        // materialisation when not package-backed.
        SemanticNodeData::DeclRef { identity } => {
            !component_meta_ref_resolves_to_package_node(identity)
        }
        // Lowered `Ref { name, type_arguments: [non-empty] }` — never
        // needs materialisation (`type_arguments.is_empty() == false`).
        SemanticNodeData::InstantiationRef { .. } => false,
        SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::TypeParam { .. } => {
            // Cycle check — try to extract a route root identity from
            // the node (legitimate for IndexedAccess chains; absent for
            // bare TypeOf / TypeParam). When no identity is extractable,
            // the legacy adapter returns `false` for these shapes, so
            // the cycle predicate stays `false` here.
            let cycle_reaches = extract_route_root_identity_node(graph, node, depth + 1)
                .is_some_and(|extraction| {
                    let mut sub_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> =
                        Vec::new();
                    let result = ref_root_reaches_transitive_cycle_node(
                        &extraction.root_identity,
                        host,
                        &mut sub_fence,
                    );
                    local_fence.extend(sub_fence);
                    result
                });
            !cycle_reaches && !type_node_has_package_backed_root(graph, node, depth + 1)
        }
        SemanticNodeData::Array { element, .. } => {
            type_node_needs_member_route_materialization(host, *element, local_fence, depth + 1)
        }
        SemanticNodeData::KeyOf { base } => {
            type_node_needs_member_route_materialization(host, *base, local_fence, depth + 1)
        }
        SemanticNodeData::Tuple { elements, .. } => elements.iter().any(|element| {
            type_node_needs_member_route_materialization(
                host,
                element.value,
                local_fence,
                depth + 1,
            )
        }),
        SemanticNodeData::Alias(inner) => {
            type_node_needs_member_route_materialization(host, *inner, local_fence, depth + 1)
        }
        _ => false,
    }
}

/// Plan §6.11 / J2 — graph-native helper mirroring the TypeExpr
/// predicate `type_expr_has_non_object_top_level_surface`. Returns
/// `true` when `node`'s top-level shape is something OTHER than a
/// concrete Object/Function/Array/Tuple/Primitive/Literal — i.e., the
/// body has a "complex" top-level shape that cannot be projected as a
/// flat Object surface.
///
/// Recurses through:
/// - `Alias(inner)` — pass-through.
/// - `DeclRef { identity }` / `InstantiationRef { base, .. }` — issue
///   an `Instantiate { base, args: [], body_mode: Skeleton }` dispatch
///   to retrieve the declaration body, then recurse.
/// - `Union | Intersection` — TypeExpr semantics: any non-Object
///   contributor returns `true`; otherwise (all Object) `false`.
///
/// Depth fused at 256.
#[allow(
    dead_code,
    reason = "Plan §6.11 / J2 — wired via slot_binding_param_can_stay_symbolic_node in K2/K3"
)]
pub(crate) fn node_has_non_object_top_level_surface(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> bool {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, QueryResult, SemanticNodeData, SemanticQueryKey};

    if depth > 256 {
        return false;
    }
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::TemplateLiteral { .. } => true,
        SemanticNodeData::Alias(inner) => {
            node_has_non_object_top_level_surface(host, *inner, depth + 1)
        }
        SemanticNodeData::DeclRef { identity } => {
            // Resolve declaration body via dispatch. Skeleton mode
            // preserves any open generic carriers in the body so the
            // top-level shape is observable structurally.
            let dispatch = ProjectSemanticDispatch::new(host);
            let key = SemanticQueryKey::Instantiate {
                base: identity.clone(),
                args: Arc::from(
                    Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                ),
                body_mode: ProjectionMode::Skeleton,
            };
            let read = dispatch.execute_read(key);
            let body_id = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return false,
            };
            node_has_non_object_top_level_surface(host, body_id, depth + 1)
        }
        SemanticNodeData::InstantiationRef { base, .. } => {
            let dispatch = ProjectSemanticDispatch::new(host);
            let key = SemanticQueryKey::Instantiate {
                base: base.clone(),
                args: Arc::from(
                    Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                ),
                body_mode: ProjectionMode::Skeleton,
            };
            let read = dispatch.execute_read(key);
            let body_id = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return false,
            };
            node_has_non_object_top_level_surface(host, body_id, depth + 1)
        }
        SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
            // Mirror the TypeExpr predicate's union/intersection rule:
            // any non-Object contributor returns `true`; if all
            // members are Object, returns `false`.
            let mut saw_object = false;
            for &m in members.iter() {
                let Some(member_data) = graph.node_data(m) else {
                    return true;
                };
                match member_data.as_ref() {
                    SemanticNodeData::Object(_) => {
                        saw_object = true;
                    }
                    SemanticNodeData::Alias(inner) => {
                        if node_has_non_object_top_level_surface(host, *inner, depth + 1) {
                            return true;
                        }
                        if matches!(
                            graph.node_data(*inner).as_deref(),
                            Some(SemanticNodeData::Object(_))
                        ) {
                            saw_object = true;
                        }
                    }
                    _ => return true,
                }
            }
            !saw_object
        }
        SemanticNodeData::Object(_)
        | SemanticNodeData::Function { .. }
        | SemanticNodeData::Array { .. }
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::VueMacroElements(_) => false,
    }
}

/// Plan §1.12 / J2 — graph-native predicate (former TypeExpr
/// counterpart, defined inline inside
/// `walk_component_meta_macro_shape_member_types`, deleted in
/// Plan §6.15 / N). Returns `true` when `node`'s shape allows the
/// slot binding parameter to remain symbolic without eager
/// materialisation.
///
/// Mirrors the TypeExpr predicate's branch structure:
///
/// - `Conditional | Mapped | IndexedAccess | TypeOf | TypeParam |
///   TemplateLiteral` → `true` (deferred / structural shells; safe
///   to keep symbolic).
/// - `Union | Intersection` → all members must satisfy the predicate
///   (matches `types.iter().all(...)`).
/// - `InstantiationRef { base, args }` (the with-args case) — when
///   the base is NOT package-backed, retrieve the declaration body
///   via dispatch and check whether it has a non-object top-level
///   surface (matches `query_engine.named_decl_body(...).is_some_and(|body|
///   type_expr_has_non_object_top_level_surface(...))`).
/// - `Alias(inner)` → pass-through (graph-native shape; TypeExpr's
///   `Parenthesized(inner)` arm).
/// - All other shapes → `false`.
///
/// Depth-fused at 256 per §4.11. Fuse returns `false` (conservative —
/// runaway recursion does not allow staying symbolic).
pub(crate) fn slot_binding_param_can_stay_symbolic_node(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> bool {
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return false;
    }
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            slot_binding_param_can_stay_symbolic_node(host, *inner, depth + 1)
        }
        SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::TemplateLiteral { .. } => true,
        SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => members
            .iter()
            .all(|&m| slot_binding_param_can_stay_symbolic_node(host, m, depth + 1)),
        // Lowered `Ref { name, type_arguments: [non-empty] }` —
        // mirrors the legacy TypeExpr `Ref { name, type_arguments }` arm
        // with the `!type_arguments.is_empty() && !package_backed` guard.
        SemanticNodeData::InstantiationRef { base, .. } => {
            if component_meta_ref_resolves_to_package_node(base) {
                return false;
            }
            // Resolve declaration body via dispatch, then check
            // top-level surface shape.
            use crate::project_semantic_dispatch::ProjectSemanticDispatch;
            use crate::semantic_query::{ProjectionMode, QueryResult, SemanticQueryKey};
            let dispatch = ProjectSemanticDispatch::new(host);
            let key = SemanticQueryKey::Instantiate {
                base: base.clone(),
                args: Arc::from(
                    Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                ),
                body_mode: ProjectionMode::Skeleton,
            };
            let read = dispatch.execute_read(key);
            let body_id = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return false,
            };
            node_has_non_object_top_level_surface(host, body_id, depth + 1)
        }
        _ => false,
    }
}

/// Plan §1.12 / J0 — graph-native predicate (former TypeExpr
/// counterpart deleted in Plan §6.15 / N). Returns `true` when
/// `node`'s route root resolves to a `/node_modules/`-rooted decl
/// identity.
///
/// Mirrors the TypeExpr predicate's structural recursion:
///
/// - `DeclRef` / `InstantiationRef` — terminal; checks root identity
///   via [`component_meta_ref_resolves_to_package_node`] (commit C +
///   §1.12).
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
/// (Plan §4.11 convention; matches
/// [`has_complex_cycle_guard_surface_node`] etc.). On fuse the
/// predicate returns `false`, matching the conservative legacy
/// behaviour: a runaway recursion is treated as "not package-backed"
/// so the caller does NOT short-circuit through the package-backed
/// branch.
pub(crate) fn type_node_has_package_backed_root(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> bool {
    use crate::semantic_query::SemanticNodeData;

    if depth > 256 {
        return false;
    }
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => {
            component_meta_ref_resolves_to_package_node(identity)
        }
        SemanticNodeData::InstantiationRef { base, .. } => {
            component_meta_ref_resolves_to_package_node(base)
        }
        SemanticNodeData::IndexedAccess { object, .. } => {
            type_node_has_package_backed_root(graph, *object, depth + 1)
        }
        SemanticNodeData::Array { element, .. } => {
            type_node_has_package_backed_root(graph, *element, depth + 1)
        }
        SemanticNodeData::KeyOf { base } => {
            type_node_has_package_backed_root(graph, *base, depth + 1)
        }
        SemanticNodeData::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_node_has_package_backed_root(graph, element.value, depth + 1)),
        SemanticNodeData::Alias(inner) => {
            type_node_has_package_backed_root(graph, *inner, depth + 1)
        }
        _ => false,
    }
}

/// Plan §1.12 — graph-native variant of the body inline-materialisation
/// preference predicate. Returns `true` when the body shape is suitable
/// for inline materialisation through the registry-route entry.
///
/// Reserved for re-wiring once Phase 11 migrates the inline-route
/// composition site to graph-native (the predicate's only consumer
/// before commit I sub-task 4 was the registry-route inline
/// composition predicate, which was deleted in this commit). Tests in
/// `meta_resolve_tests.rs` exercise this predicate directly.
#[allow(
    dead_code,
    reason = "Re-wired in Phase 11; covered by unit tests in meta_resolve_tests.rs"
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

/// Plan §1.12 / §4.8 / Commit R — graph-native BFS for transitive cycle
/// detection, with host-owned cache.
///
/// Architecture:
///   1. **Fast path (§4.9)** — `RefCycleResultDb::peek` consults the
///      generation-local cache. On `validated_at_generation == current`,
///      returns the cached `bool` without re-walking.
///   2. **Slow path** — cooperative-admission via
///      `ref_cycle_db_get_or_compute`; the BFS body
///      ([`bfs_compute_inner`]) runs synchronously in the
///      `compute` closure (per cooperative_admission's synchronous-
///      compute contract), capturing `&VerterHost` directly. On
///      cooperative-admission failure (revalidation rejected the entry),
///      falls back to an uncached recompute so the caller never sees
///      a publishing miss.
///
/// The cache key is `DeclIdentity`; entries store `(result, dep_signature,
/// validated_at_generation)`. `dep_signature` is built from every
/// `Instantiate` dispatch's recorded fence accumulated during the BFS,
/// so cache invalidation is precise per-canonical (via `RefCycleResultDb::
/// invalidate_for_canonical`) and project-generation-wide (via
/// `invalidate_all`).
///
/// Plan §4.1 / R7-13 / R7-14 — legacy parity rules carried into the
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
/// recursive-helper guards (plan §4.13).
pub(crate) fn ref_root_reaches_transitive_cycle_node(
    root_identity: &crate::semantic_query::DeclIdentity,
    host: &VerterHost,
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
) -> bool {
    let db = host.project_type_store().ref_cycle_db();

    // Fast path: peek with generation-local validity. On hit, extend
    // the caller's local_fence and return without dispatching any
    // Instantiate query.
    if let Some(read) = crate::component_meta_caches::ref_cycle_db_peek(db, root_identity, host) {
        local_fence.extend(read.dep_signature.iter().cloned());
        return read.value;
    }

    // Slow path: cooperative-admission with synchronous compute. The
    // closure captures `&VerterHost` by reference — Rust borrow safe
    // because `cooperative_get_or_insert_with_post_publish` runs the
    // compute closure on the calling thread (per its
    // synchronous-compute contract documented at
    // `cooperative_admission.rs:278`).
    let read_opt = crate::component_meta_caches::ref_cycle_db_get_or_compute(
        db,
        root_identity,
        host,
        |compute_fence| bfs_compute_inner(root_identity, host, compute_fence),
    );

    match read_opt {
        Some(read) => {
            local_fence.extend(read.dep_signature.iter().cloned());
            read.value
        }
        None => {
            // Cooperative admission returned None (revalidation
            // rejected the freshly-built entry). Recompute uncached as
            // a fallback so the caller still sees a result. Do NOT
            // cache: the same revalidation race that just rejected
            // the entry would reject the next attempt too.
            let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
            let result = bfs_compute_inner(root_identity, host, &mut fence);
            local_fence.extend(fence);
            result
        }
    }
}

/// Plan §6.13 / Commit R — extracted BFS body. Identical legacy-parity
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
    host: &VerterHost,
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
) -> bool {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryKey};
    use rustc_hash::FxHashSet;
    use std::collections::VecDeque;

    const MAX_HOPS: usize = 64;

    #[cfg(test)]
    dep_signature::BFS_COMPUTE_COUNTER.with(|c| c.set(c.get() + 1));

    let dispatch = ProjectSemanticDispatch::new(host);
    let graph = host.project_type_store().semantic_graph();
    let mut visited: FxHashSet<crate::semantic_query::DeclIdentity> = FxHashSet::default();
    let mut queue: VecDeque<(crate::semantic_query::DeclIdentity, bool)> = VecDeque::new();
    visited.insert(root_identity.clone());
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
            base: current,
            args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            // Plan §4.21 / R10-2 — Skeleton mode preserves open generics so
            // body lowering produces TypeParam graph nodes for T-refs (not
            // Opaque(Miss)). Without this, nested-Conditional fixtures like
            // canonical nuxt-ui DotPathKeys collapse the conditional and
            // recursive refs are invisible to collect_ref_identities_node.
            body_mode: ProjectionMode::Skeleton,
        };
        let read = dispatch.execute_read(key);
        local_fence.extend(read.dep_signature.iter().cloned());
        let body_id = match read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) | QueryResult::Error(_) => continue,
        };

        let body_has_complex_signal =
            path_has_complex_signal || has_complex_cycle_guard_surface_node(host, body_id, 0);

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

        // Plan §6.2 / §6.6.5 — F-prep test instrumentation. Records
        // child_refs.len() per visited identity name into the per-thread
        // observer (no-op when no observer installed).
        #[cfg(test)]
        record_bfs_child_refs_count_for_test(current_decl_name_for_test.as_ref(), child_refs.len());

        for (child_identity, ref_has_type_args) in child_refs {
            let cycle_has_complex_signal = body_has_complex_signal || ref_has_type_args;
            // Cycle is reported when:
            //  (a) child == root (transitive cycle back to BFS root), OR
            //  (b) child == current (intermediate self-reference at this
            //      decl — legacy parity: the legacy walker checked
            //      `ref_name == name` against the CURRENT decl, not the
            //      root). This catches fixtures where DotPathKeys's body
            //      recursively references DotPathKeys via a complex
            //      helper surface (canonical nuxt-ui DotPathKeys).
            if cycle_has_complex_signal
                && (&child_identity == root_identity || child_identity == current_identity)
            {
                return true;
            }
            if visited.insert(child_identity.clone()) {
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
/// graphs (Plan §4.11). The fuse intentionally returns `false` on
/// hit — a runaway recursion is treated as "not complex" so the
/// caller continues the BFS rather than terminating prematurely.
pub(crate) fn has_complex_cycle_guard_surface_node(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    if depth > 256 {
        return false;
    }
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return false;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            has_complex_cycle_guard_surface_node(host, *inner, depth + 1)
        }
        SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
            members
                .iter()
                .any(|&m| has_complex_cycle_guard_surface_node(host, m, depth + 1))
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
        | SemanticNodeData::TypeOf { .. }
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
/// `depth` fuses recursion at 256 (Plan §4.11). The fuse returns
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
            _ => {}
        }
    }
}
