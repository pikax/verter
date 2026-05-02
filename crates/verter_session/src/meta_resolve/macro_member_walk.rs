//! Walker + macro-shape member traversal.
//!
//! Phase 11a domain 9 — owns:
//! - `walk_component_meta_macro_shape_member_types` (the per-field driver
//!   the ctx method calls into),
//! - `materialize_component_meta_macro_shape_member_type_expr` (the
//!   member-route fast path with route-vs-project reconciliation),
//! - `MEMBER_ROUTE_FAST_PATH_HITS` (the test-only counter incremented when
//!   the fast-path satisfies a request).
//!
//! Lines 136-907 of the post-commit-9 `meta_resolve.rs` shell. Visibility
//! escalation: the formerly-private `walk_*` and `materialize_*_member_type_expr`
//! free functions are escalated to `pub(crate)` so the impl block in
//! `host_methods.rs` (Phase 11a commit 9) can keep calling them without
//! callsite churn.

use crate::host_manage::component_meta_trace_custom;
use crate::types::FileAnalysisSnapshot;

use super::dispatch_helpers::{
    instantiate_local_generic_ref_via_dispatch, project_expr_class_a_shape_via_dispatch,
    project_expr_class_a_via_dispatch, project_expr_class_a_via_dispatch_threaded,
};
use super::materialize::{
    define_props_member_can_stay_symbolic_without_rescue, expr_needs_projection_rescue,
    field_should_preserve_shallow_symbolic_raw_type, has_prop_shape_surface,
    lowered_needs_member_route_materialization, materialize_component_meta_type_expr_until_stable,
    projection_result_beats_solver_shape, type_expr_is_slots_member_route,
};
use super::resolved_state::{
    lowered_root_reaches_transitive_cycle, select_imported_materialization_scope,
};
use super::scoring::compare_type_expr_improvement;

use super::graph_predicates::slot_binding_param_can_stay_symbolic_node;
use super::registry_materialize::type_expr_contains_public_member_route;

use crate::resolver_core::component_meta_registry::{
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};

/// Step 6.2 fast-path counter — instrumented in
/// `materialize_component_meta_macro_shape_member_type_expr` whenever a
/// route / project candidate satisfies the request directly without
/// falling through to the eager whole-expression materialize path.
/// The static-text test
/// `step6_2_member_route_fast_path_runs_before_eager_materialize`
/// asserts the structural ordering invariant by reading the source
/// file; the counter is kept available for future runtime probes.
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

/// Issue #10 / Phase 10 — capture-token counter incremented every
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

// Plan §6.8 — legacy walker shim deleted; all production call sites
// now use `ComponentMetaQueryEngine::materialize_member_surface_expr`
// directly.

pub(crate) fn walk_component_meta_macro_shape_member_types(
    scope_canonical_id: &str,
    snapshot: &FileAnalysisSnapshot,
    eval_source: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    fn slot_member_needs_binding_rescue(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        fn slot_binding_param_needs_materialization(
            ty: &verter_semantic::analysis::type_expr::TypeExpr,
        ) -> bool {
            use verter_semantic::analysis::type_expr::TypeExpr;

            match ty {
                TypeExpr::Parenthesized(inner) => slot_binding_param_needs_materialization(inner),
                TypeExpr::Object(_) => false,
                TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
                    types.iter().any(slot_binding_param_needs_materialization)
                }
                _ => true,
            }
        }

        match ty {
            TypeExpr::Parenthesized(inner) => slot_member_needs_binding_rescue(inner),
            TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
                types.iter().any(slot_member_needs_binding_rescue)
            }
            TypeExpr::Function(function) => {
                if function.parameters.is_empty() {
                    return false;
                }
                function
                    .parameters
                    .iter()
                    .any(|parameter| slot_binding_param_needs_materialization(&parameter.ty))
            }
            TypeExpr::Object(object) => {
                let mut saw_callable = false;
                for member in &object.properties {
                    match member {
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            saw_callable = true;
                            if function.parameters.iter().any(|parameter| {
                                slot_binding_param_needs_materialization(&parameter.ty)
                            }) {
                                return true;
                            }
                        }
                        ObjectMember::Method(method) => {
                            saw_callable = true;
                            if method.function.parameters.iter().any(|parameter| {
                                slot_binding_param_needs_materialization(&parameter.ty)
                            }) {
                                return true;
                            }
                        }
                        ObjectMember::Property(_) | ObjectMember::IndexSignature(_) => {}
                    }
                }
                !saw_callable
            }
            _ => true,
        }
    }

    fn shape_needs_member_rescue(
        scope_canonical_id: &str,
        shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        shape.properties.iter().any(|property| {
            expr_needs_projection_rescue(query_engine, scope_canonical_id, &property.ty)
        })
    }

    /// Plan §6.15 / N — migration helper. Lowers the TypeExpr input to
    /// a `Navigate`-mode `SemanticNodeId` and dispatches to J2's
    /// [`slot_binding_param_can_stay_symbolic_node`].
    ///
    /// Returns `false` (conservative — "must materialize, not symbolic")
    /// when lowering fails. Matches the deleted TypeExpr fallback's
    /// `_ => false` arm semantically: when the dispatcher cannot lower
    /// the input, prefer materialization over symbolic preservation.
    fn lowered_slot_binding_param_can_stay_symbolic(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        let ctx = query_engine.ctx;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
        let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            ty,
            crate::semantic_query::ProjectionMode::Navigate,
        ) else {
            return false;
        };
        slot_binding_param_can_stay_symbolic_node(ctx, node, 0)
    }

    fn slot_member_binding_rescue_can_stay_symbolic(
        ty: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match ty {
            TypeExpr::Parenthesized(inner) => slot_member_binding_rescue_can_stay_symbolic(
                inner,
                scope_canonical_id,
                query_engine,
            ),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types.iter().all(|ty| {
                slot_member_binding_rescue_can_stay_symbolic(ty, scope_canonical_id, query_engine)
            }),
            TypeExpr::Function(function) => {
                !function.parameters.is_empty()
                    && function.parameters.iter().all(|parameter| {
                        lowered_slot_binding_param_can_stay_symbolic(
                            &parameter.ty,
                            scope_canonical_id,
                            query_engine,
                        )
                    })
            }
            TypeExpr::Object(object) => {
                let mut saw_callable = false;
                object.properties.iter().all(|member| match member {
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        saw_callable = true;
                        !function.parameters.is_empty()
                            && function.parameters.iter().all(|parameter| {
                                lowered_slot_binding_param_can_stay_symbolic(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    query_engine,
                                )
                            })
                    }
                    ObjectMember::Method(method) => {
                        saw_callable = true;
                        !method.function.parameters.is_empty()
                            && method.function.parameters.iter().all(|parameter| {
                                lowered_slot_binding_param_can_stay_symbolic(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    query_engine,
                                )
                            })
                    }
                    ObjectMember::Property(_) | ObjectMember::IndexSignature(_) => true,
                }) && saw_callable
            }
            _ => false,
        }
    }

    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                if let Some(lowered) = params.define_props.get(define_props_index) {
                    if let Some(define_props) = evaluated_types
                        .define_props
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue =
                            expr_needs_projection_rescue(query_engine, scope_canonical_id, lowered);
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_props.result.value,
                                query_engine,
                            );
                        if needs_projection_rescue {
                            if lowered_needs_projection_rescue
                                && define_props.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) =
                                    project_expr_class_a_shape_via_dispatch(
                                        query_engine.ctx,
                                        scope_canonical_id,
                                        lowered,
                                    )
                                    .filter(has_prop_shape_surface)
                                {
                                    let projected_result =
                                        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                            projected_shape,
                                        );
                                    if projection_result_beats_solver_shape(
                                        &projected_result,
                                        &define_props.result,
                                    ) {
                                        define_props.result = projected_result;
                                    }
                                }
                            }
                            for property in &mut define_props.result.value.properties {
                                if type_expr_is_slots_member_route(&property.ty) {
                                    continue;
                                }
                                let preserve_symbolic_field_surface = evaluated_types
                                    .props
                                    .iter()
                                    .find(|field| field.name == property.name)
                                    .is_some_and(|field| {
                                        field_should_preserve_shallow_symbolic_raw_type(
                                            scope_canonical_id,
                                            field,
                                            query_engine,
                                        )
                                    });
                                if preserve_symbolic_field_surface {
                                    continue;
                                }
                                if define_props_member_can_stay_symbolic_without_rescue(
                                    &property.ty,
                                    scope_canonical_id,
                                    query_engine,
                                ) {
                                    continue;
                                }
                                let property_needs_projection_rescue = expr_needs_projection_rescue(
                                    query_engine,
                                    scope_canonical_id,
                                    &property.ty,
                                );
                                if !property_needs_projection_rescue
                                    && !lowered_needs_member_route_materialization(
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    )
                                {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_props_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                if let Some(lowered) = params.define_emits.get(define_emits_index) {
                    if let Some(define_emits) = evaluated_types
                        .define_emits
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue =
                            expr_needs_projection_rescue(query_engine, scope_canonical_id, lowered);
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_emits.result.value,
                                query_engine,
                            );
                        if needs_projection_rescue {
                            if lowered_needs_projection_rescue
                                && define_emits.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) = project_expr_class_a_shape_via_dispatch(
                                    query_engine.ctx,
                                    scope_canonical_id,
                                    lowered,
                                )
                                .filter(
                                    verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                                )
                                {
                                    let projected_result =
                                        verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                            projected_shape,
                                        );
                                    if projection_result_beats_solver_shape(
                                        &projected_result,
                                        &define_emits.result,
                                    ) {
                                        define_emits.result = projected_result;
                                    }
                                }
                            }
                            for property in &mut define_emits.result.value.properties {
                                if !expr_needs_projection_rescue(
                                    query_engine,
                                    scope_canonical_id,
                                    &property.ty,
                                ) {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_emits_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_emits_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                if let Some(lowered) = params.define_slots.get(define_slots_index) {
                    if let Some(define_slots) = evaluated_types
                        .define_slots
                        .iter_mut()
                        .find(|entry| entry.macro_index == macro_index)
                    {
                        let lowered_needs_projection_rescue =
                            expr_needs_projection_rescue(query_engine, scope_canonical_id, lowered);
                        let needs_projection_rescue = lowered_needs_projection_rescue
                            || shape_needs_member_rescue(
                                scope_canonical_id,
                                &define_slots.result.value,
                                query_engine,
                            );
                        let needs_slot_binding_rescue = define_slots
                            .result
                            .value
                            .properties
                            .iter()
                            .any(|property| slot_member_needs_binding_rescue(&property.ty));
                        if needs_projection_rescue || needs_slot_binding_rescue {
                            if lowered_needs_projection_rescue
                                && define_slots.result.value.properties.is_empty()
                            {
                                if let Some(projected_shape) = project_expr_class_a_shape_via_dispatch(
                                    query_engine.ctx,
                                    scope_canonical_id,
                                    lowered,
                                )
                                .filter(
                                    verter_semantic::analysis::type_eval_build::has_named_shape_surface,
                                )
                                {
                                    if needs_projection_rescue {
                                        let projected_result =
                                            verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                                                projected_shape,
                                            );
                                        if projection_result_beats_solver_shape(
                                            &projected_result,
                                            &define_slots.result,
                                        ) {
                                            define_slots.result = projected_result;
                                        }
                                    }
                                }
                            }
                            for property in &mut define_slots.result.value.properties {
                                let property_needs_projection_rescue = expr_needs_projection_rescue(
                                    query_engine,
                                    scope_canonical_id,
                                    &property.ty,
                                );
                                let binding_rescue_can_stay_symbolic =
                                    slot_member_binding_rescue_can_stay_symbolic(
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                                if !property_needs_projection_rescue
                                    && (binding_rescue_can_stay_symbolic
                                        || !slot_member_needs_binding_rescue(&property.ty))
                                {
                                    continue;
                                }
                                component_meta_trace_custom!(
                                    "materialize_define_slots_member",
                                    format!(
                                        "owner={} name={} ty={:?}",
                                        scope_canonical_id, property.name, property.ty,
                                    ),
                                );
                                property.ty =
                                    materialize_component_meta_macro_shape_member_type_expr(
                                        lowered,
                                        property.name.as_str(),
                                        &property.ty,
                                        scope_canonical_id,
                                        query_engine,
                                    );
                            }
                        }
                    }
                }
                define_slots_index += 1;
            }
            _ => {}
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_macro_shape_member_type_expr(
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
    member_name: &str,
    current: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
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
            // Phase 5e commit 6 — migrate to dispatch (sub-plan §C.3 D-T recipe).
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
    // Plan §6.6 / E — the inline-registry-route candidate chain was
    // retired in commit E. B1's materialiser registry-route branch
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

    // Step 6.2 reorder (plan §3): try route/project candidates BEFORE
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
                // Phase 5e commit 5 — migrate the route-loop call to the
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
            return candidate.clone();
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
        return best;
    }
    // Plan §6.6 / E — the alias-body candidate path was retired in
    // commit E. B1's materialiser registry-route branch handles the
    // equivalent
    // projection through dispatch. The slow-path materialize-and-
    // improve loop below remains as the catch-all for shapes that
    // don't match a registry-route shape.

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

    best
}
