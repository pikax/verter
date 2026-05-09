//! Helpers used by the per-macro projector decomposition (§7.1):
//! root-name collection on `defineProps<T>()` macros, the
//! slot-binding registry-collection skip predicate, and the
//! per-member rescue helper called by the dispatch-path refinement
//! (`materialize_component_meta_field_types`) when the projector
//! path leaves an unresolved cross-file Pick<>['key'] /
//! recursive-alias terminal.
//!
//! The legacy OUTER walker driver was retired by the projector cutover — production
//! routes through `meta_resolve::projectors::project_evaluated_types`.
//! The per-member rescue below remains load-bearing for cross-file
//! recursive-alias preservation (e.g. `Tree = { children?: Tree[] }`).

use crate::host_manage::component_meta_trace_custom;
use crate::types::FileAnalysisSnapshot;

use super::dispatch_helpers::{
    instantiate_local_generic_ref_via_dispatch, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded,
};
use super::materialize::materialize_component_meta_type_expr_until_stable;
use super::resolved_state::{
    lowered_root_reaches_transitive_cycle, select_imported_materialization_scope,
};
use super::scoring::compare_type_expr_improvement;

use super::registry_materialize::type_expr_contains_public_member_route;

use crate::resolver_core::component_meta_registry::{
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};

/// Step 6.2 fast-path counter — instrumented in
/// `materialize_component_meta_macro_shape_member_type_expr` whenever a route / project
/// candidate satisfies the request directly without falling through
/// to the eager whole-expression materialize path.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) static MEMBER_ROUTE_FAST_PATH_HITS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Capture-token counter name recorded every time the slot-binding
/// registry-collection skip predicate fires for a slot binding rooted
/// in the owner's own `defineProps<T>()` interface. Used by
/// `component_meta_slot_binding_skip_tests` to discriminate the
/// positive case (counter > 0) from the counterfixtures (counter == 0)
/// via `CaptureToken::start_for_query` / `CaptureToken::end()`.
pub(crate) const SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER: &str =
    "slot_binding_registry_collection_skips";

/// Issue #10 / capture-token counter incremented every
/// time the Pick member-route materialiser actually descends into a
/// callable parameter type. The package-backed suppression predicate
/// (`pick_member_route_should_skip_callable_descent`) bypasses the
/// indexed-access route entirely; when bypassed, the counter does NOT
/// increment for that member. Used by
/// `component_meta_pick_omit_tests::declared_session_meta_preserves_imported_pick_callback_package_param`
/// (asserts `== 0` for package-backed param) and
/// `pick_callback_workspace_local_param_still_descends` (asserts
/// `>= 1` for workspace-local param).
pub(crate) const PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER: &str =
    "pick_member_route_callable_descent_count";

/// Collect the source-level root type names referenced by every
/// type-based `defineProps<T>()` macro in `snapshot.macros`. The
/// result is the set of names that — when found at the root of a
/// slot binding's raw type — make the binding's contribution to the
/// component-meta registry redundant (the defineProps interface is
/// already authoritative for that surface).
///
/// Only top-level `Ref { name }` roots are collected. Inline
/// object-literal type arguments, intersections, unions, and other
/// composite shapes do not produce a root name (the predicate
/// downstream falls back to `false` for those cases).
pub(crate) fn collect_define_props_root_names(
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    use verter_semantic::analysis::AnalyzedMacroKind;

    fn root_ref_name(ty: &TypeExpr) -> Option<&str> {
        match ty {
            TypeExpr::Ref { name, .. } => Some(name.as_ref()),
            TypeExpr::Parenthesized(inner) => root_ref_name(inner),
            _ => None,
        }
    }

    let mut names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for mac in snapshot.macros.iter() {
        if mac.kind != AnalyzedMacroKind::DefineProps || !mac.is_type_based {
            continue;
        }
        if let Some(arg) = mac.parsed_type_argument.as_deref() {
            if let Some(name) = root_ref_name(arg) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// Decide whether the slot-binding's raw-type root is the owner's
/// own `defineProps<T>()` interface — in which case the binding's
/// registry-collection contribution is redundant and can be
/// skipped.
///
/// The predicate fires only when the binding's raw type root
/// resolves to a name in `define_props_roots` for the same owner
/// AND the binding does NOT introduce a new prop surface beyond
/// what the defineProps root already exposes:
///
/// - `Props['avatar']` (indexed-access route) → fires when
///   `Props ∈ define_props_roots`.
/// - `Pick<Props, 'avatar' | 'label'>` / `Omit<Props, 'count'>`
///   (utility route) → fires when `Props ∈ define_props_roots`.
/// - `Props & Extra` (intersection broadens surface) → does NOT
///   fire. The intersection's `Extra` arm is reachable only
///   through the registry-collection call.
/// - `Props | Other` (union) → does NOT fire. Same reasoning as
///   intersection.
/// - `ButtonProps['avatar']` where `ButtonProps` is imported (not
///   the owner's defineProps root) → does NOT fire.
/// - `Primitive(_)` / `Object(_)` / fully-expanded fields whose
///   raw type was None → does NOT fire (no work to skip).
pub(crate) fn slot_binding_targets_define_props_root(
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    define_props_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    if define_props_roots.is_empty() {
        return false;
    }

    // Mirror the `expr` selection in
    // `collect_component_meta_registry_public_field_refs`: prefer the
    // parsed `raw_type` when present, otherwise fall back to the
    // (already-expanded) `r#type`.
    let parsed_raw = field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
    let expr: &TypeExpr = parsed_raw.as_ref().unwrap_or(&field.r#type);

    fn unwrap_paren(ty: &TypeExpr) -> &TypeExpr {
        match ty {
            TypeExpr::Parenthesized(inner) => unwrap_paren(inner),
            _ => ty,
        }
    }
    let expr = unwrap_paren(expr);

    // Broadening shapes (intersection / union) must NOT skip — extra
    // arms beyond the defineProps surface would be lost.
    if matches!(expr, TypeExpr::Intersection(_) | TypeExpr::Union(_)) {
        return false;
    }

    // Try the indexed-access / utility route extractors. These return
    // the source-level root name (e.g. `Props` for `Props['avatar']`
    // or `Pick<Props, ...>`) when the expression is structurally a
    // path projection rooted at a single Ref.
    let root_name = crate::resolver_core::component_meta_registry::component_meta_registry_public_utility_route(expr)
        .or_else(|| {
            crate::resolver_core::component_meta_registry::component_meta_registry_public_indexed_access_route(expr)
        })
        .map(|(name, _route)| name);

    if let Some(root) = root_name {
        return define_props_roots.contains(&root);
    }

    false
}

fn publish_member_route_result(
    ctx: &dyn crate::resolver_core::ResolverContext,
    cache_key: &crate::component_meta_caches::MemberRouteResultCacheKey,
    result: &verter_semantic::analysis::type_expr::TypeExpr,
) {
    let db = ctx.project_type_store().member_route_result_db();
    let captured_canonical = cache_key.scope_canonical_id.clone();
    let captured_result = result.clone();
    let _ = crate::component_meta_caches::member_route_result_db_get_or_compute(
        db,
        cache_key.clone(),
        ctx,
        move |compute_fence| {
            // Seed the dep_signature with the scope's whole_hash so a
            // file-content edit on the owner invalidates this entry.
            // The walker's downstream calls into
            // `materialize_component_meta_type_expr_until_stable`
            // accumulate dispatch dep facts via
            // `accumulate_dispatch_dep_signature` into the per-request
            // thread-local, but those don't propagate into THIS entry's
            // signature directly — the scope whole_hash is the
            // load-bearing fact for invalidation correctness.
            if let Some(state) = ctx.shallow_file_state(captured_canonical.as_ref()) {
                compute_fence.push((
                    std::sync::Arc::clone(&captured_canonical),
                    crate::semantic_query::DepVersion::WholeHash(state.whole_hash),
                ));
            }
            captured_result
        },
    );
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_macro_shape_member_type_expr(
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    member_name: &str,
    current: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
        &crate::loop5_instrumentation::MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS,
        &crate::loop5_instrumentation::MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_NS,
    );
    fn wrapped_member_leaf(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        member_name: &str,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let verter_semantic::analysis::type_expr::TypeExpr::Object(object) = expr else {
            return None;
        };
        let [verter_semantic::analysis::type_expr::ObjectMember::Property(property)] =
            object.properties.as_slice()
        else {
            return None;
        };
        (property.name == member_name).then(|| property.ty.clone())
    }

    fn top_level_needs_owner_route_fallback(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        fn inner(expr: &TypeExpr) -> bool {
            match expr {
                TypeExpr::Parenthesized(inner_expr)
                | TypeExpr::KeyOf(inner_expr)
                | TypeExpr::Rest(inner_expr) => inner(inner_expr),
                TypeExpr::Ref { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::Conditional { .. }
                | TypeExpr::Mapped { .. }
                | TypeExpr::TypeOf(_)
                | TypeExpr::TypeParameter(_)
                | TypeExpr::Infer { .. } => true,
                TypeExpr::Array { element, .. } => inner(element),
                TypeExpr::Tuple { elements, .. } => {
                    elements.iter().any(|element| inner(&element.ty))
                }
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => types.iter().any(inner),
                TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                    ObjectMember::Property(property) => inner(&property.ty),
                    ObjectMember::Method(method) => {
                        method
                            .function
                            .parameters
                            .iter()
                            .any(|param| inner(&param.ty))
                            || method.function.return_type.as_deref().is_some_and(inner)
                    }
                    ObjectMember::IndexSignature(signature) => {
                        inner(&signature.key_type) || inner(&signature.value_type)
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        function.parameters.iter().any(|param| inner(&param.ty))
                            || function.return_type.as_deref().is_some_and(inner)
                    }
                }),
                TypeExpr::Function(function) => {
                    function.parameters.iter().any(|param| inner(&param.ty))
                        || function.return_type.as_deref().is_some_and(inner)
                }
                TypeExpr::TemplateLiteral { expressions, .. } => expressions.iter().any(inner),
                TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::Unknown { .. }
                | TypeExpr::RecursiveRef { .. } => false,
            }
        }

        inner(expr)
    }

    // Final-result cache peek BEFORE the route_candidates Vec
    // builder. The cache key is `(scope, member_name, lowered, mode)`;
    // `current` is the per-property surface that varies between
    // sibling properties of the same `lowered` and is NOT part of
    // the key — `wrapped_member_leaf` reproduces the per-property
    // adjustment when needed below. Sits below the cycle / package
    // guards because those are encoded in the dep_signature path
    // through `materialize_component_meta_type_expr_until_stable`'s
    // dispatch fence accumulation. The stored result is the `best`
    // value the slow path would have selected for this `(scope,
    // member, lowered, mode)` tuple.
    let cache_key = crate::component_meta_caches::MemberRouteResultCacheKey {
        scope_canonical_id: std::sync::Arc::<str>::from(scope_canonical_id),
        member_name: std::sync::Arc::<str>::from(member_name),
        lowered: std::sync::Arc::new(lowered.clone()),
        mode: crate::semantic_query::ProjectionMode::Expanded,
    };
    {
        let ctx = query_engine.ctx;
        let db = ctx.project_type_store().member_route_result_db();
        if let Some(cached) = db.peek(&cache_key, ctx) {
            return cached.value;
        }
    }

    // Loop-5 instrumentation — bumped for every outer macro-member walk
    // that misses MemberRouteResultDb. The TypeExpr operator-node count
    // is sampled here as the lowered surface coming in.
    crate::loop5_instrumentation::MACRO_MEMBER_WALK_OUTER_CALLS
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::loop5_instrumentation::record_outer_call_type_expr(lowered);

    let current_is_route_expr = matches!(
        current,
        verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess { .. }
    ) || component_meta_registry_public_utility_route(current)
        .or_else(|| component_meta_registry_public_indexed_access_route(current))
        .is_some();
    if !current_is_route_expr
        && matches!(
            current,
            verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
                if type_arguments.is_empty()
        )
    {
        return current.clone();
    }
    let materialize_scope_canonical_id = if current_is_route_expr {
        select_imported_materialization_scope(current, scope_canonical_id, query_engine).or_else(
            || select_imported_materialization_scope(lowered, scope_canonical_id, query_engine),
        )
    } else {
        select_imported_materialization_scope(lowered, scope_canonical_id, query_engine)
    }
    .unwrap_or_else(|| scope_canonical_id.to_string());
    let route_object_expr = match lowered {
        verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
            if !type_arguments.is_empty()
                && !lowered_root_reaches_transitive_cycle(
                    query_engine,
                    materialize_scope_canonical_id.as_str(),
                    lowered,
                ) =>
        {
            // Migrate to dispatch (sub- D-T recipe).
            instantiate_local_generic_ref_via_dispatch(
                query_engine.ctx,
                materialize_scope_canonical_id.as_str(),
                lowered,
            )
            .unwrap_or_else(|| lowered.clone())
        }
        _ => lowered.clone(),
    };
    let route_expr = verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess {
        object: std::sync::Arc::new(route_object_expr),
        index: std::sync::Arc::new(
            verter_semantic::analysis::type_expr::TypeExpr::string_literal(member_name.to_string()),
        ),
    };
    // The inline-registry-route candidate chain was
    // B1's materialiser registry-route branch
    // dispatches Pick/Omit + IndexedAccess shapes canonically
    // through dispatch; the empty `inline_route_candidate` lets the
    // surrounding materialize-and-improve loop drive the member
    // route through the materialiser entry.
    let inline_route_candidate: Option<verter_semantic::analysis::type_expr::TypeExpr> = None;
    let _ = current_is_route_expr;
    if let Some(candidate) = &inline_route_candidate {
        component_meta_trace_custom!(
            "materialize_member_route_inline_candidate_result",
            format!(
                "owner={} member={} candidate={:?}",
                scope_canonical_id, member_name, candidate,
            ),
        );
    }

    // Step 6.2 reorder: try route/project candidates BEFORE
    // the eager whole-expression `materialize_component_meta_type_expr_until_stable(current, …)`
    // call. The pre-Step-6.2 ordering ran `current` materialization
    // first and only consulted route candidates as fallbacks; this
    // unconditionally invoked the heaviest path even for fixtures
    // where a single project/solve hop satisfied the request. The
    // FAIL-FIRST test `materialize_member_route_caller_ordering`
    // exercises the symbolic-intersection case where a route
    // candidate succeeds without falling through to the eager
    // materialize — `MTL_CALL_COUNT` stays at 0 post-fix.
    //
    // Returns Some(early-good-enough-candidate) when a route /
    // project candidate is concrete enough to satisfy the public
    // contract directly; None means we must fall through to the
    // eager materialization path.
    let route_scope_candidates: Vec<String> = if current_is_route_expr {
        Vec::new()
    } else if materialize_scope_canonical_id == scope_canonical_id {
        vec![scope_canonical_id.to_string()]
    } else {
        vec![
            scope_canonical_id.to_string(),
            materialize_scope_canonical_id.clone(),
        ]
    };

    let route_candidates: Vec<verter_semantic::analysis::type_expr::TypeExpr> = {
        let mut acc = Vec::new();
        for candidate_scope in &route_scope_candidates {
            let projected = {
                component_meta_trace_custom!(
                    "materialize_member_route_projected_candidate",
                    format!(
                        "owner={} member={} candidate_scope={} route={:?}",
                        scope_canonical_id, member_name, candidate_scope, route_expr,
                    ),
                );
                project_expr_class_a_via_dispatch(
                    query_engine.ctx,
                    candidate_scope.as_str(),
                    &route_expr,
                )
            };
            let solved = {
                component_meta_trace_custom!(
                    "materialize_member_route_solved_candidate",
                    format!(
                        "owner={} member={} candidate_scope={} route={:?}",
                        scope_canonical_id, member_name, candidate_scope, route_expr,
                    ),
                );
                // Migrate the route-loop call to the
                // Class A dispatch helper. Preserve the engine's
                // `lower_and_project_to_expanded` `reduced != *expr` filter
                // (the helper omits that constraint by design — see
                // `project_expr_class_a_via_dispatch_threaded` filter at
                // `meta_resolve.rs` lines 156-157).
                project_expr_class_a_via_dispatch_threaded(
                    query_engine.ctx,
                    Some(query_engine),
                    candidate_scope.as_str(),
                    &route_expr,
                )
                .filter(|reduced| reduced != &route_expr)
            };
            for candidate in [projected, solved].into_iter().flatten() {
                acc.push(candidate);
            }
        }
        acc
    };

    let candidate_is_good_enough = |candidate: &verter_semantic::analysis::type_expr::TypeExpr| {
        !matches!(
            candidate,
            verter_semantic::analysis::type_expr::TypeExpr::Unknown { .. }
        ) && !type_expr_contains_public_member_route(candidate)
            && !top_level_needs_owner_route_fallback(candidate)
    };

    // Fast-path: any route candidate that is already structurally
    // sufficient short-circuits the eager materialize. This is the
    // observable contract the FAIL-FIRST test asserts — when a route
    // candidate succeeds, MTL_CALL_COUNT stays at 0.
    for candidate in &route_candidates {
        if lowered_root_reaches_transitive_cycle(query_engine, scope_canonical_id, candidate) {
            continue;
        }
        if candidate_is_good_enough(candidate) {
            #[cfg(test)]
            MEMBER_ROUTE_FAST_PATH_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let result = candidate.clone();
            publish_member_route_result(query_engine.ctx, &cache_key, &result);
            return result;
        }
    }

    let current_materialized = if inline_route_candidate.is_some() {
        inline_route_candidate.clone().unwrap()
    } else {
        component_meta_trace_custom!(
            "materialize_member_route_current",
            format!(
                "owner={} member={} current={:?}",
                scope_canonical_id, member_name, current,
            ),
        );
        materialize_component_meta_type_expr_until_stable(
            current,
            materialize_scope_canonical_id.as_str(),
            crate::semantic_query::ProjectionMode::Expanded,
            query_engine,
        )
    };
    component_meta_trace_custom!(
        "materialize_member_route_current_result",
        format!(
            "owner={} member={} current_materialized={:?}",
            scope_canonical_id, member_name, current_materialized,
        ),
    );
    let mut best = if current_is_route_expr {
        wrapped_member_leaf(&current_materialized, member_name).unwrap_or(current_materialized)
    } else {
        current_materialized
    };
    if let Some(candidate) = inline_route_candidate {
        if compare_type_expr_improvement(&candidate, &best) {
            best = candidate;
        }
    }
    if candidate_is_good_enough(&best) {
        publish_member_route_result(query_engine.ctx, &cache_key, &best);
        return best;
    }
    // The materialiser's registry-route branch
    // handles the alias-body projection through dispatch. The
    // slow-path materialize-and-improve loop below remains as the
    // catch-all for shapes that don't match a registry-route shape.

    // Slow path: previously-cached project/solve candidates that
    // weren't structurally sufficient now feed into the
    // materialize-and-improve loop, same as before. This preserves
    // behavioral coverage for fixtures that need the heavier
    // `materialize_component_meta_type_expr_until_stable(&candidate, …)`
    // recursion.
    for candidate in route_candidates {
        if lowered_root_reaches_transitive_cycle(query_engine, scope_canonical_id, &candidate)
            && !compare_type_expr_improvement(&candidate, &best)
        {
            continue;
        }
        let candidate_materialized = {
            component_meta_trace_custom!(
                "materialize_member_route_candidate_materialized",
                format!(
                    "owner={} member={} candidate={:?}",
                    scope_canonical_id, member_name, candidate,
                ),
            );
            materialize_component_meta_type_expr_until_stable(
                &candidate,
                materialize_scope_canonical_id.as_str(),
                crate::semantic_query::ProjectionMode::Expanded,
                query_engine,
            )
        };
        if compare_type_expr_improvement(&candidate_materialized, &best) {
            best = candidate_materialized;
        }
    }

    publish_member_route_result(query_engine.ctx, &cache_key, &best);
    best
}
