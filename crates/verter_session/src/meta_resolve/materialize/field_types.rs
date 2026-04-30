//! Materialization core: TypeExpr stabilizer + field-type rescue.
//!
//! Phase 11a domain 7 (field-types portion). Owns:
//! - the eager whole-expression materializer
//!   (`materialize_component_meta_type_expr_until_stable` + `_full`),
//! - the `materialize_component_meta_field_types` driver,
//! - the field-rescue / shallow-symbolic / package-backed predicates that
//!   gate it,
//! - the test-only `MTL_CALL_COUNT` instrumentation that the eager-entry
//!   FAIL-FIRST tests count off (Step 6.2 / D22).
//!
//! Lines 99-1827 of the pre-split `meta_resolve.rs` (with the test-only
//! MTL counter section moved here from later in the shell so the static
//! it counts off lives in the same module). The body is verbatim apart
//! from `pub(crate)` visibility escalation on the formerly-private items
//! the parent shell still calls (matches the Phase 11a foundation
//! extractions: `dispatch_helpers.rs`, `resolved_state.rs`, etc.).

use crate::resolver_core::component_meta_registry::{
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};
use crate::types::FileAnalysisSnapshot;
use crate::VerterHost;
use std::sync::Arc;

use super::super::dep_signature::accumulate_dispatch_dep_signature;
use super::super::dispatch_helpers::{
    project_expr_class_a_via_dispatch, project_type_surface_expr_via_host_threaded,
};
use super::super::field_state::MacroFieldGraphState;
use super::super::request_host::ResolvedMacroMeta;
use super::super::resolved_state::{
    lowered_root_reaches_transitive_cycle, select_imported_materialization_scope,
};
use super::super::scoring::compare_type_expr_improvement;
use super::macro_shapes::expr_needs_projection_rescue;

// `materialize_component_meta_macro_shape_member_type_expr` lives in the
// `macro_member_walk` sibling (Phase 11a commit 10);
// `component_meta_registry_should_keep_raw_symbolic_non_object_alias`
// and `preserve_package_backed_symbolic_refs_node` live in the
// `registry_materialize` sibling (Phase 11a commit 11);
// `type_node_needs_member_route_materialization` lives in the
// `graph_predicates` sibling (Phase 11a commit 12).
use super::super::graph_predicates::type_node_needs_member_route_materialization;
use super::super::macro_member_walk::materialize_component_meta_macro_shape_member_type_expr;
use super::super::registry_materialize::{
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    preserve_package_backed_symbolic_refs_node,
};

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    materialize_component_meta_type_expr_until_stable_full(
        expr,
        scope_canonical_id,
        mode,
        query_engine,
    )
    .type_expr
}

/// Materialize a `TypeExpr` and return both the result and the
/// producing `SemanticNodeId` + accumulated dep_signature
/// ([`MaterializedTypeExpr`]; D31 / D32). Sidecar-capture call sites
/// (Step 9 surface-id propagation) read `.node_id`; the session merges
/// `.dep_signature` into `ResolvedComponentMetaState.fact_versions`
/// before publish (Step 6.6.A).
///
/// The main entry [`materialize_component_meta_type_expr_until_stable`]
/// remains for callers that need only the `TypeExpr` shell — it
/// delegates here and discards `node_id` / `dep_signature`.
///
/// **Body (Step 1.5 final cutover):** the legacy owner-vs-imported
/// scope reconciliation has been removed. Materialization now flows
/// entirely through dispatch:
/// `shallow_lower_type_expr` → `raise_and_reduce(mode)`. Step 1.5
/// closed the three substitution-parity gaps that previously required
/// the legacy walker fallback (Pick<X,K>['member'] indexed access,
/// mapped+conditional `infer P` per-key reduction, and method
/// signatures used as `IndexedAccess` bases).
///
/// Per-request memoisation is preserved so repeat queries of the same
/// `(scope, expr, mode)` triple within one component-meta request
/// reuse the prior result instead of re-running the dispatch
/// reduction. Dispatch's own family memo handles cross-request
/// deduplication.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_type_expr_until_stable_full(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    mode: crate::semantic_query::ProjectionMode,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> crate::project_semantic_dispatch::raise::MaterializedTypeExpr {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::NodeScopeId;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    // Step 6.2 / D22: count every entry into whole-expression
    // materialization. Memo hits + cold builds both increment so the
    // FAIL-FIRST test discriminates the call-ordering contract at the
    // *entry* boundary, not the build closure.
    #[cfg(test)]
    MTL_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // §4.5 items 2-5: per-request memo keyed on `(scope, candidate, mode)`.
    let memo_key = (
        scope_canonical_id.to_string(),
        expr.clone(),
        matches!(mode, crate::semantic_query::ProjectionMode::Navigate),
    );
    #[cfg(test)]
    crate::spike_instrumentation::record_cache_read("materialize_memo");
    if let Some(cached) = query_engine
        .materialize_memo
        .borrow()
        .get(&memo_key)
        .cloned()
    {
        return cached;
    }

    // Step 3 closure: peek host-owned MaterializeMemoDb.
    {
        let host = query_engine.host();
        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = host.project_type_store().materialize_memo_db();
        if let Some(cached) = host_db.peek(&arc_key, host) {
            query_engine
                .materialize_memo
                .borrow_mut()
                .insert(memo_key, cached.clone());
            return cached;
        }
    }

    // Step 1.5 thin dispatch wrapper. Build NodeScopeId for the file
    // scope, then lower → raise_and_reduce in the caller's mode.
    let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
    let host = query_engine.host();
    let dispatch = ProjectSemanticDispatch::new(host);
    let env: FxHashMap<String, crate::semantic_query::SemanticNodeId> = FxHashMap::default();
    let whole_hash = host
        .shallow_file_state(scope_canonical_id)
        .map(|state| state.whole_hash)
        .unwrap_or_default();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(scope_canonical_id),
        whole_hash,
        local_scope: None,
    };
    let name_resolution = rustc_hash::FxHashMap::default();
    let mut substitutions: Vec<(Arc<str>, crate::semantic_query::SemanticNodeId)> = Vec::new();
    // Phase 5h §5.10 r15/F11 — capture the scope-shadowing context
    // once for the materialize → lower pipeline so the dispatch
    // fast-path observes the same shadow set the route extraction
    // path uses.
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
        scope_payload.as_deref(),
    );
    let lowered = dispatch.shallow_lower_type_expr(
        expr,
        &env,
        &scope,
        &name_resolution,
        scope_payload.as_deref(),
        &shadowing,
        &mut substitutions,
        mode,
    );
    let dispatch_materialized = dispatch.raise_and_reduce(lowered, mode);

    // Step 6.6.A: accumulate dispatch's dep_signature into the
    // per-request thread-local so compute_component_meta_state_inner
    // can merge the facts into ResolvedComponentMetaState.fact_versions
    // before publish. Each materialize call contributes its own
    // dispatch-side fact set; the accumulator deduplicates.
    accumulate_dispatch_dep_signature(&dispatch_materialized.dep_signature);

    let materialized = MaterializedTypeExpr {
        node_id: dispatch_materialized.node_id,
        type_expr: dispatch_materialized.type_expr,
        dep_signature: dispatch_materialized.dep_signature,
    };

    // Step 3 closure: write-through to host-owned MaterializeMemoDb.
    {
        let arc_key = (
            std::sync::Arc::<str>::from(scope_canonical_id),
            std::sync::Arc::new(expr.clone()),
            mode,
        );
        let host_db = host.project_type_store().materialize_memo_db();
        let captured_value = materialized.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, host, move || {
            let dep_sig = crate::resolver_core::component_meta_query_engine::engine_dep_signature_for_canonical(
                host,
                captured_canonical.as_str(),
            );
            Some((captured_value, dep_sig))
        });
    }

    query_engine
        .materialize_memo
        .borrow_mut()
        .insert(memo_key, materialized.clone());
    materialized
}

pub(crate) fn type_expr_has_package_backed_object_like_root(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    fn root_name(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> Option<String> {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match expr {
            TypeExpr::Parenthesized(inner) => root_name(inner),
            TypeExpr::IndexedAccess { object, .. } => root_name(object),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if matches!(name.as_ref(), "Pick" | "Omit") && type_arguments.len() == 2 => {
                crate::resolver_core::component_meta_registry::component_meta_registry_ref_name(
                    &type_arguments[0],
                )
                .map(str::to_string)
            }
            TypeExpr::Ref { name, .. } => Some(name.to_string()),
            _ => None,
        }
    }

    let Some(root_name) = root_name(expr) else {
        return false;
    };

    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    if !declaration_scope.contains("/node_modules/") {
        return false;
    }

    if matches!(
        declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        return true;
    }

    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.clone()
    } else {
        declaration.resolved_name
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            crate::resolver_core::component_meta_registry::component_meta_registry_has_explicit_object_surface(&body)
        })
}

pub(crate) fn type_expr_is_slots_member_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { index, .. } => matches!(
            index.as_ref(),
            TypeExpr::Literal(LiteralValue::String(name)) if name.as_str() == "slots"
        ),
        TypeExpr::Parenthesized(inner) => type_expr_is_slots_member_route(inner),
        _ => false,
    }
}

// Plan §6.6 / E — the alias-body rescue chain was retired in commit
// E. B1's materialiser registry-route branch handles route shapes
// (`Pick<T, K>`, `Omit<T, K>`, `T['k']`) through dispatch's canonical
// projection, so the alias-body walk-through is no longer needed.
// The retired symbols are listed in the `RETIRED_SYMBOLS` array of
// the static-grep gate test (commit I).

pub(crate) fn parsed_field_raw_type(
    field: &verter_semantic::analysis::type_expand::ExpandedField,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
        .filter(|expr| !expr.is_unknown())
}

pub(crate) fn interface_body_has_members_needing_materialization(
    body: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
    fn member_type_needs_materialization(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::Conditional { .. } => true,
            TypeExpr::Parenthesized(inner) => member_type_needs_materialization(inner),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                types.iter().any(member_type_needs_materialization)
            }
            _ => false,
        }
    }
    match body {
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => member_type_needs_materialization(&prop.ty),
            _ => false,
        }),
        TypeExpr::Intersection(types) => types
            .iter()
            .any(interface_body_has_members_needing_materialization),
        _ => false,
    }
}

pub(crate) fn top_level_imported_ref_can_stay_symbolic(
    scope_canonical_id: &str,
    name: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (target_scope, target_name) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());
    let target_declaration = query_engine
        .resolve_direct_prepared_type_declaration_metadata(
            target_scope.as_str(),
            target_name.as_str(),
        )
        .unwrap_or(declaration);

    if matches!(
        target_declaration.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface
            | crate::resolver_core::ResolvedDeclarationKind::Class,
    ) {
        // Interfaces with members that need materialization (IndexedAccess,
        // Mapped types) should not stay symbolic — the consumer needs the
        // concrete member shapes.
        let body_needs_materialization = query_engine
            .named_decl_body(target_scope.as_str(), target_name.as_str())
            .is_some_and(|body| interface_body_has_members_needing_materialization(&body));
        if !body_needs_materialization {
            return true;
        }
    }

    query_engine
        .named_decl_body(target_scope.as_str(), target_name.as_str())
        .is_some_and(|body| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &body,
                target_scope.as_str(),
                query_engine,
            )
        })
}

pub(crate) fn field_should_preserve_shallow_symbolic_raw_type(
    scope_canonical_id: &str,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let Some(raw) = parsed_field_raw_type(field) else {
        return false;
    };

    match &raw {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => top_level_imported_ref_can_stay_symbolic(
            scope_canonical_id,
            name.as_ref(),
            query_engine,
        ),
        _ if component_meta_registry_public_utility_route(&raw).is_some() => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess { .. } => {
            type_expr_has_package_backed_object_like_root(&raw, scope_canonical_id, query_engine)
        }
        verter_semantic::analysis::type_expr::TypeExpr::TypeOf(_)
        | verter_semantic::analysis::type_expr::TypeExpr::TypeParameter(_) => false,
        _ => {
            query_engine.should_preserve_shallow_field_expr(scope_canonical_id, &raw)
                && !lowered_needs_member_route_materialization(
                    &raw,
                    scope_canonical_id,
                    query_engine,
                )
        }
    }
}

/// Plan §6.15 / N — migration helper. Lowers `expr` to a Navigate-mode
/// `SemanticNodeId` and dispatches to J1's graph-native
/// [`type_node_needs_member_route_materialization`] predicate. The
/// cycle-BFS dep-signature facts collected during the predicate's walk
/// are accumulated into the per-request thread-local dispatch
/// accumulator so the caller's completion fence remains complete
/// (matches legacy behaviour: the deleted TypeExpr predicate routed
/// through the deleted F-era TypeExpr cycle adapter which accumulated
/// the same way).
///
/// Returns `false` (conservative: not needed) when lowering fails —
/// matches the deleted TypeExpr predicate's behaviour for shapes the
/// dispatcher cannot lower.
pub(crate) fn lowered_needs_member_route_materialization(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = query_engine.host;
    let dispatch = ProjectSemanticDispatch::new(host);
    let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return false;
    };
    let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let result = type_node_needs_member_route_materialization(host, node, &mut local_fence, 0);
    if !local_fence.is_empty() {
        accumulate_dispatch_dep_signature(&Arc::from(local_fence.into_boxed_slice()));
    }
    result
}

/// Plan §6.15 / N — migration helper. Lowers `materialized` and `raw`
/// TypeExpr inputs to Navigate-mode `SemanticNodeId`s, dispatches to
/// J4's graph-native [`preserve_package_backed_symbolic_refs_node`],
/// and raises the result back to TypeExpr.
///
/// Returns `materialized.clone()` (matches the deleted TypeExpr
/// predicate's `_ => materialized.clone()` arm) when either lowering
/// fails or the raise back to TypeExpr fails — preserves existing
/// behaviour for shapes the dispatcher cannot lower deterministically.
pub(crate) fn lowered_preserve_package_backed_symbolic_refs(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    let host = engine.host;
    let dispatch = ProjectSemanticDispatch::new(host);
    let Some(materialized_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        materialized,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let Some(raw_node) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        raw,
        crate::semantic_query::ProjectionMode::Navigate,
    ) else {
        return materialized.clone();
    };
    let preserved_node =
        preserve_package_backed_symbolic_refs_node(host, materialized_node, raw_node, 0);
    if preserved_node == materialized_node {
        return materialized.clone();
    }
    dispatch
        .raise_node_to_type_expr(preserved_node)
        .unwrap_or_else(|| materialized.clone())
}

pub(crate) fn define_props_member_can_stay_symbolic_without_rescue(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => define_props_member_can_stay_symbolic_without_rescue(
            inner,
            scope_canonical_id,
            query_engine,
        ),
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            define_props_member_can_stay_symbolic_without_rescue(
                &element.ty,
                scope_canonical_id,
                query_engine,
            )
        }),
        TypeExpr::Union(types) => types.iter().all(|ty| {
            define_props_member_can_stay_symbolic_without_rescue(
                ty,
                scope_canonical_id,
                query_engine,
            )
        }),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
            let declaration_scope = if declaration.canonical_source.is_empty() {
                scope_canonical_id
            } else {
                declaration.canonical_source.as_str()
            };
            let resolved_name = if declaration.resolved_name.is_empty() {
                name.as_ref()
            } else {
                declaration.resolved_name.as_str()
            };
            query_engine
                .named_decl_body(declaration_scope, resolved_name)
                .is_none()
                || (declaration_scope != scope_canonical_id
                    && top_level_imported_ref_can_stay_symbolic(
                        scope_canonical_id,
                        name.as_ref(),
                        query_engine,
                    ))
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Function(_) => true,
        _ => false,
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn materialize_component_meta_field_types(
    scope_canonical_id: &str,
    snapshot: &FileAnalysisSnapshot,
    eval_source: &str,
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    /// Plan §4.10 / K1 — `rescue_field` mutates field type via
    /// `MacroFieldGraphState::set_current_type` rather than direct
    /// `field.r#type = X` assignment. The `field` reference is read-only
    /// here (used only for raw_type access via `parsed_field_raw_type`);
    /// type mutations route through `field_state`.
    fn rescue_field(
        scope_canonical_id: &str,
        field: &verter_semantic::analysis::type_expand::ExpandedField,
        field_state: &mut MacroFieldGraphState<'_>,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) {
        if !expr_needs_projection_rescue(
            query_engine,
            scope_canonical_id,
            field_state.published_type(),
        ) {
            return;
        }

        let materialize_scope_canonical_id = select_imported_materialization_scope(
            field_state.published_type(),
            scope_canonical_id,
            query_engine,
        )
        .or_else(|| {
            parsed_field_raw_type(field).as_ref().and_then(|raw| {
                select_imported_materialization_scope(raw, scope_canonical_id, query_engine)
            })
        })
        .unwrap_or_else(|| scope_canonical_id.to_string());
        let rescued = materialize_component_meta_type_expr_until_stable(
            field_state.published_type(),
            materialize_scope_canonical_id.as_str(),
            crate::semantic_query::ProjectionMode::Expanded,
            query_engine,
        );
        if rescued != *field_state.published_type() {
            field_state.set_current_type(rescued);
        }
    }

    /// Plan §6.14 / K2 — call the J1 `_node` predicate via the
    /// field-state's lazy-lowered current_node. Returns `false` when
    /// lowering fails (matches the legacy TypeExpr predicate's
    /// "conservative not-needed" fallback when no canonical node id
    /// exists for the input).
    fn current_needs_member_route_materialization(
        host: &VerterHost,
        field_state: &mut MacroFieldGraphState<'_>,
    ) -> bool {
        let Some(node) = field_state.current_node() else {
            return false;
        };
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        type_node_needs_member_route_materialization(host, node, &mut local_fence, 0)
    }

    /// Plan §6.14 / K2 — call the J1 `_node` predicate via the
    /// field-state's lazy-lowered raw_node. Returns `false` when
    /// lowering fails.
    fn raw_needs_member_route_materialization(
        host: &VerterHost,
        field_state: &mut MacroFieldGraphState<'_>,
        raw: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        let Some(node) = field_state.raw_node(raw) else {
            return false;
        };
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        type_node_needs_member_route_materialization(host, node, &mut local_fence, 0)
    }

    fn route_leaf_beats_wrapper_object(
        candidate: &verter_semantic::analysis::type_expr::TypeExpr,
        current: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        let TypeExpr::Object(object) = current else {
            return false;
        };
        let [ObjectMember::Property(property)] = object.properties.as_slice() else {
            return false;
        };
        property.ty == *candidate
    }

    fn type_expr_contains_named_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> bool {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } => name.as_ref() == target_name,
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => type_expr_contains_named_recursive_ref(inner, target_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| type_expr_contains_named_recursive_ref(&element.ty, target_name)),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| type_expr_contains_named_recursive_ref(ty, target_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    type_expr_contains_named_recursive_ref(&property.ty, target_name)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            type_expr_contains_named_recursive_ref(return_type, target_name)
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    type_expr_contains_named_recursive_ref(&signature.key_type, target_name)
                        || type_expr_contains_named_recursive_ref(
                            &signature.value_type,
                            target_name,
                        )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|parameter| {
                        type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        type_expr_contains_named_recursive_ref(return_type, target_name)
                    })
                }
            }),
            TypeExpr::Function(function) => {
                function.parameters.iter().any(|parameter| {
                    type_expr_contains_named_recursive_ref(&parameter.ty, target_name)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    type_expr_contains_named_recursive_ref(return_type, target_name)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                type_expr_contains_named_recursive_ref(object, target_name)
                    || type_expr_contains_named_recursive_ref(index, target_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                type_expr_contains_named_recursive_ref(check, target_name)
                    || type_expr_contains_named_recursive_ref(extends, target_name)
                    || type_expr_contains_named_recursive_ref(true_type, target_name)
                    || type_expr_contains_named_recursive_ref(false_type, target_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                type_expr_contains_named_recursive_ref(source, target_name)
                    || type_expr_contains_named_recursive_ref(value, target_name)
                    || name_type.as_deref().is_some_and(|name_type| {
                        type_expr_contains_named_recursive_ref(name_type, target_name)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| type_expr_contains_named_recursive_ref(expr, target_name)),
            TypeExpr::Ref { type_arguments, .. } => type_arguments
                .iter()
                .any(|arg| type_expr_contains_named_recursive_ref(arg, target_name)),
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    fn expand_named_recursive_refs_one_layer(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
        replacement: &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::RecursiveRef { name, .. } if name.as_ref() == target_name => {
                replacement.clone()
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(expand_named_recursive_refs_one_layer(
                    element,
                    target_name,
                    replacement,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                expand_named_recursive_refs_one_layer(inner, target_name, replacement),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: expand_named_recursive_refs_one_layer(
                                &element.ty,
                                target_name,
                                replacement,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| expand_named_recursive_refs_one_layer(ty, target_name, replacement))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty = expand_named_recursive_refs_one_layer(
                                &property.ty,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = expand_named_recursive_refs_one_layer(
                                &signature.key_type,
                                target_name,
                                replacement,
                            );
                            signature.value_type = expand_named_recursive_refs_one_layer(
                                &signature.value_type,
                                target_name,
                                replacement,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = expand_named_recursive_refs_one_layer(
                                    &parameter.ty,
                                    target_name,
                                    replacement,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                                    return_type,
                                    target_name,
                                    replacement,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty = expand_named_recursive_refs_one_layer(
                        &parameter.ty,
                        target_name,
                        replacement,
                    );
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(expand_named_recursive_refs_one_layer(
                        return_type,
                        target_name,
                        replacement,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(expand_named_recursive_refs_one_layer(
                    object,
                    target_name,
                    replacement,
                )),
                index: Arc::new(expand_named_recursive_refs_one_layer(
                    index,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(expand_named_recursive_refs_one_layer(
                    check,
                    target_name,
                    replacement,
                )),
                extends: Arc::new(expand_named_recursive_refs_one_layer(
                    extends,
                    target_name,
                    replacement,
                )),
                true_type: Arc::new(expand_named_recursive_refs_one_layer(
                    true_type,
                    target_name,
                    replacement,
                )),
                false_type: Arc::new(expand_named_recursive_refs_one_layer(
                    false_type,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                optional,
                readonly,
                name_type,
                value,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(expand_named_recursive_refs_one_layer(
                    source,
                    target_name,
                    replacement,
                )),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(expand_named_recursive_refs_one_layer(
                        name_type,
                        target_name,
                        replacement,
                    ))
                }),
                value: Arc::new(expand_named_recursive_refs_one_layer(
                    value,
                    target_name,
                    replacement,
                )),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| {
                        expand_named_recursive_refs_one_layer(expr, target_name, replacement)
                    })
                    .collect(),
            },
            TypeExpr::RecursiveRef { .. }
            | TypeExpr::Ref { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn rewrite_named_self_refs_to_recursive_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        target_name: &str,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use std::sync::Arc;
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if name.as_ref() == target_name && type_arguments.is_empty() => {
                TypeExpr::recursive_ref(target_name, Vec::new())
            }
            TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Array { element, readonly } => TypeExpr::Array {
                element: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    element,
                    target_name,
                )),
                readonly: *readonly,
            },
            TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
                rewrite_named_self_refs_to_recursive_ref(inner, target_name),
            )),
            TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                elements: elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: rewrite_named_self_refs_to_recursive_ref(&element.ty, target_name),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect(),
                readonly: *readonly,
            },
            TypeExpr::Union(types) => TypeExpr::Union(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Intersection(types) => TypeExpr::Intersection(
                types
                    .iter()
                    .map(|ty| rewrite_named_self_refs_to_recursive_ref(ty, target_name))
                    .collect(),
            ),
            TypeExpr::Object(object) => {
                let mut next = object.as_ref().clone();
                for member in &mut next.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty =
                                rewrite_named_self_refs_to_recursive_ref(&property.ty, target_name);
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.key_type,
                                target_name,
                            );
                            signature.value_type = rewrite_named_self_refs_to_recursive_ref(
                                &signature.value_type,
                                target_name,
                            );
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = rewrite_named_self_refs_to_recursive_ref(
                                    &parameter.ty,
                                    target_name,
                                );
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                                    return_type,
                                    target_name,
                                ));
                            }
                        }
                    }
                }
                TypeExpr::Object(Arc::new(next))
            }
            TypeExpr::Function(function) => {
                let mut next = function.as_ref().clone();
                for parameter in &mut next.parameters {
                    parameter.ty =
                        rewrite_named_self_refs_to_recursive_ref(&parameter.ty, target_name);
                }
                if let Some(return_type) = next.return_type.as_mut() {
                    *return_type = Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        return_type,
                        target_name,
                    ));
                }
                TypeExpr::Function(Arc::new(next))
            }
            TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    object,
                    target_name,
                )),
                index: Arc::new(rewrite_named_self_refs_to_recursive_ref(index, target_name)),
            },
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => TypeExpr::Conditional {
                check: Arc::new(rewrite_named_self_refs_to_recursive_ref(check, target_name)),
                extends: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    extends,
                    target_name,
                )),
                true_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    true_type,
                    target_name,
                )),
                false_type: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    false_type,
                    target_name,
                )),
            },
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: Arc::new(rewrite_named_self_refs_to_recursive_ref(
                    source,
                    target_name,
                )),
                value: Arc::new(rewrite_named_self_refs_to_recursive_ref(value, target_name)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_ref().map(|name_type| {
                    Arc::new(rewrite_named_self_refs_to_recursive_ref(
                        name_type,
                        target_name,
                    ))
                }),
            },
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis.clone(),
                expressions: expressions
                    .iter()
                    .map(|expr| rewrite_named_self_refs_to_recursive_ref(expr, target_name))
                    .collect(),
            },
            TypeExpr::Ref { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => expr.clone(),
        }
    }

    fn indexed_access_alias_body_transport(
        scope_canonical_id: &str,
        raw: &verter_semantic::analysis::type_expr::TypeExpr,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        let (root_symbol, route) = component_meta_registry_public_indexed_access_route(raw)?;
        let crate::resolver_core::RouteDemand::MemberPath(path) = route else {
            return None;
        };
        let [member_name] = path.as_slice() else {
            return None;
        };

        let member_ty = query_engine.prepared_member_raw_type(
            scope_canonical_id,
            root_symbol.as_str(),
            member_name,
        )?;
        let verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } = member_ty
        else {
            return None;
        };
        if !type_arguments.is_empty() {
            return None;
        }

        let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
        let declaration_scope = if declaration.canonical_source.is_empty() {
            scope_canonical_id.to_string()
        } else {
            declaration.canonical_source
        };
        let declaration_name = if declaration.resolved_name.is_empty() {
            name.as_ref().to_string()
        } else {
            declaration.resolved_name
        };
        let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
            declaration_scope.as_str(),
            declaration_name.as_str(),
        );
        let body = query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())?;
        let replacement = rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
        matches!(
            replacement,
            verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
        )
        .then_some(replacement)
    }

    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut prop_member_routes = rustc_hash::FxHashMap::<
        String,
        Vec<verter_semantic::analysis::type_expr::TypeExpr>,
    >::default();
    let mut slot_binding_scope_hints = rustc_hash::FxHashMap::<String, Vec<String>>::default();
    let mut define_props_index = 0usize;
    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                if let Some(lowered) = params.define_props.get(define_props_index) {
                    let mut prop_names = rustc_hash::FxHashSet::<String>::default();
                    prop_names.extend(mac.prop_fields.iter().map(|field| field.name.clone()));
                    for resolved in resolved_macros.iter().filter(|resolved| {
                        resolved.macro_index == macro_index
                            && resolved.macro_kind
                                == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                    }) {
                        prop_names.extend(resolved.props.iter().map(|field| field.name.clone()));
                    }
                    for prop_name in prop_names {
                        prop_member_routes
                            .entry(prop_name)
                            .or_default()
                            .push(lowered.clone());
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                for resolved in resolved_macros.iter().filter(|resolved| {
                    resolved.macro_index == macro_index
                        && resolved.macro_kind
                            == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
                }) {
                    let declaration_scope = resolved.declaration.canonical_source.as_str();
                    if declaration_scope.is_empty() {
                        continue;
                    }
                    for slot in &resolved.slots {
                        for binding in &slot.bindings {
                            let entry = slot_binding_scope_hints
                                .entry(format!("{}.{}", slot.name, binding.name))
                                .or_default();
                            if !entry.iter().any(|scope| scope == declaration_scope) {
                                entry.push(declaration_scope.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for field in &mut evaluated_types.props {
        let preserve_raw = field_should_preserve_shallow_symbolic_raw_type(
            scope_canonical_id,
            field,
            query_engine,
        );
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE owner={} field={} raw={:?} current={:?} preserve_raw={}",
                scope_canonical_id, field.name, field.raw_type, field.r#type, preserve_raw,
            ));
        }
        if preserve_raw {
            continue;
        }
        // Plan §4.10 / K1 — wrap `field.r#type` in a `MacroFieldGraphState`
        // for the duration of this iteration. Direct `field.r#type = X`
        // mutations are routed through `field_state.set_current_type(X)`;
        // graph-native rewrites (K2) will route through
        // `set_current_node_rewrite`. Final write-back via `publish()`
        // at iteration exit.
        let host = query_engine.host;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        if let Some(candidate) = evaluated_types
            .define_props
            .iter()
            .flat_map(|define_props| define_props.result.value.properties.iter())
            .find(|property| property.name == field.name)
            .map(|property| property.ty.clone())
        {
            if compare_type_expr_improvement(&candidate, field_state.published_type())
                && !expr_needs_projection_rescue(query_engine, scope_canonical_id, &candidate)
            {
                field_state.set_current_type(candidate);
            }
        }
        rescue_field(scope_canonical_id, field, &mut field_state, query_engine);
        // Plan §6.14 / K2 — migrate predicate to graph-native J1 _node
        // version via field_state.raw_node().
        let raw_needs_member_route = parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
            raw_needs_member_route_materialization(host, &mut field_state, raw)
                || component_meta_registry_public_utility_route(raw).is_some()
        });
        let raw_is_unpreserved_top_level_ref =
            parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                matches!(
                    raw,
                    verter_semantic::analysis::type_expr::TypeExpr::Ref { type_arguments, .. }
                        if type_arguments.is_empty()
                )
            });
        if crate::host_manage::component_meta_debug_enabled() {
            // Plan §6.14 / K2 — debug log uses the J1 _node predicate
            // through field_state.current_node().
            let current_needs = current_needs_member_route_materialization(host, &mut field_state);
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE_POST_RESCUE owner={} field={} current={:?} raw_needs_member_route={} raw_is_unpreserved_top_level_ref={} current_needs_member_route={}",
                scope_canonical_id,
                field.name,
                field_state.published_type(),
                raw_needs_member_route,
                raw_is_unpreserved_top_level_ref,
                current_needs,
            ));
        }
        // Plan §6.14 / K2 — migrate predicate to graph-native J1 _node
        // version via field_state.current_node().
        if !(raw_needs_member_route
            || raw_is_unpreserved_top_level_ref
            || current_needs_member_route_materialization(host, &mut field_state))
        {
            field.r#type = field_state.publish();
            continue;
        }

        if let Some(routes) = prop_member_routes.get(&field.name).cloned() {
            for lowered in routes {
                let rescued = materialize_component_meta_macro_shape_member_type_expr(
                    &lowered,
                    field.name.as_str(),
                    field_state.published_type(),
                    scope_canonical_id,
                    query_engine,
                );
                if compare_type_expr_improvement(&rescued, field_state.published_type()) {
                    field_state.set_current_type(rescued);
                }
            }
        }
        let materialize_scope_canonical_id = select_imported_materialization_scope(
            field_state.published_type(),
            scope_canonical_id,
            query_engine,
        )
        .or_else(|| {
            parsed_field_raw_type(field).as_ref().and_then(|raw| {
                select_imported_materialization_scope(raw, scope_canonical_id, query_engine)
            })
        })
        .unwrap_or_else(|| scope_canonical_id.to_string());
        let raw_route_root_is_package_backed =
            parsed_field_raw_type(field).as_ref().is_some_and(|raw| {
                type_expr_has_package_backed_object_like_root(raw, scope_canonical_id, query_engine)
            });
        if raw_needs_member_route && !raw_route_root_is_package_backed {
            // Plan §6.6 / E — the alias-body rescue chain was retired
            // in commit E. B1's materialiser registry-route branch
            // already handles `Pick<Foo, ...>`, `Omit<Foo, ...>`, and
            // `Foo['a']['b']…` shapes through dispatch's canonical
            // projection. The direct
            // `query_engine.materialize_member_surface_expr` call now
            // applies the same projection in the materialiser's
            // policy-gated form.
            {
                let routed_surface = query_engine.materialize_member_surface_expr(
                    materialize_scope_canonical_id.as_str(),
                    field_state.published_type(),
                    true,
                );
                if compare_type_expr_improvement(&routed_surface, field_state.published_type()) {
                    field_state.set_current_type(routed_surface);
                }
                if let Some(raw_route_surface) =
                    parsed_field_raw_type(field).as_ref().and_then(|raw| {
                        project_expr_class_a_via_dispatch(
                            query_engine.host,
                            materialize_scope_canonical_id.as_str(),
                            raw,
                        )
                    })
                {
                    let raw_route_surface = query_engine.materialize_member_surface_expr(
                        materialize_scope_canonical_id.as_str(),
                        &raw_route_surface,
                        true,
                    );
                    if compare_type_expr_improvement(
                        &raw_route_surface,
                        field_state.published_type(),
                    ) || route_leaf_beats_wrapper_object(
                        &raw_route_surface,
                        field_state.published_type(),
                    ) {
                        field_state.set_current_type(raw_route_surface);
                    }
                }
                if let Some(projected_route_surface) = project_expr_class_a_via_dispatch(
                    query_engine.host,
                    materialize_scope_canonical_id.as_str(),
                    field_state.published_type(),
                ) {
                    let projected_route_surface = query_engine.materialize_member_surface_expr(
                        materialize_scope_canonical_id.as_str(),
                        &projected_route_surface,
                        false,
                    );
                    if compare_type_expr_improvement(
                        &projected_route_surface,
                        field_state.published_type(),
                    ) || route_leaf_beats_wrapper_object(
                        &projected_route_surface,
                        field_state.published_type(),
                    ) {
                        field_state.set_current_type(projected_route_surface);
                    }
                }
            }
        }
        let rescued = match field_state.published_type() {
            verter_semantic::analysis::type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let name = name.clone();
                let declaration = query_engine.resolve_type_declaration(
                    materialize_scope_canonical_id.as_str(),
                    name.as_ref(),
                );
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    materialize_scope_canonical_id.clone()
                } else {
                    declaration.canonical_source.clone()
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name.clone()
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                let rescued = query_engine
                    .named_decl_body(target_scope.as_str(), target_name.as_str())
                    .map(|body| {
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str())
                    })
                    .or_else(|| {
                        // TODO(phase-5g): the Class B migration target
                        // is `dispatch.execute(Instantiate { args: [],
                        // body_mode: Expanded })` per sub-plan §4.1.
                        // Phase 5m §5.13a.2 — route through the
                        // bridge helper so the §5.14.1 pre-flight
                        // gate sees zero external engine-method
                        // callers. The bridge body retains the
                        // engine call through the migration window
                        // per §5.13a.2.
                        project_type_surface_expr_via_host_threaded(
                            query_engine,
                            target_scope.as_str(),
                            target_name.as_str(),
                        )
                    })
                    .unwrap_or_else(|| {
                        materialize_component_meta_type_expr_until_stable(
                            field_state.published_type(),
                            materialize_scope_canonical_id.as_str(),
                            crate::semantic_query::ProjectionMode::Expanded,
                            query_engine,
                        )
                    });
                if crate::host_manage::component_meta_debug_enabled() {
                    crate::host_manage::component_meta_debug(format!(
                        "FIELD_MATERIALIZE_REF owner={} field={} current_ref={} materialize_scope={} target_scope={} target_name={} rescued={:?}",
                        scope_canonical_id,
                        field.name,
                        name,
                        materialize_scope_canonical_id,
                        target_scope,
                        target_name,
                        rescued,
                    ));
                }
                rescued
            }
            _ => materialize_component_meta_type_expr_until_stable(
                field_state.published_type(),
                materialize_scope_canonical_id.as_str(),
                crate::semantic_query::ProjectionMode::Expanded,
                query_engine,
            ),
        };
        if compare_type_expr_improvement(&rescued, field_state.published_type()) {
            field_state.set_current_type(rescued);
        }
        // Track whether the raw-ref branch handled the field (legacy
        // `continue` semantics). Set TRUE when the legacy code would
        // have `continue`d before the final indexed-access transport
        // path. We still must `publish()` after `continue`; using a
        // local bool lets us re-route through publish().
        let mut raw_ref_branch_handled = false;
        if let Some(verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        }) = parsed_field_raw_type(field)
        {
            if type_arguments.is_empty() {
                let declaration =
                    query_engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
                let declaration_scope = if declaration.canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    declaration.canonical_source
                };
                let declaration_name = if declaration.resolved_name.is_empty() {
                    name.as_ref().to_string()
                } else {
                    declaration.resolved_name
                };
                let (target_scope, target_name) = query_engine.resolve_final_prepared_type_target(
                    declaration_scope.as_str(),
                    declaration_name.as_str(),
                );
                if let Some(body) =
                    query_engine.named_decl_body(target_scope.as_str(), target_name.as_str())
                {
                    let replacement =
                        rewrite_named_self_refs_to_recursive_ref(&body, target_name.as_str());
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "FIELD_RAW_REF_BODY owner={} field={} target_scope={} target_name={} body={:?} replacement={:?}",
                            scope_canonical_id,
                            field.name,
                            target_scope,
                            target_name,
                            body,
                            replacement,
                        ));
                    }
                    if matches!(
                        replacement,
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) && !matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) {
                        field_state.set_current_type(replacement);
                        raw_ref_branch_handled = true;
                    } else if matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Ref {
                            name: ref_name,
                            type_arguments,
                        } if type_arguments.is_empty()
                            && ref_name.as_ref() == target_name.as_str()
                    ) {
                        // When the field type is a bare Ref to the target
                        // type, apply the body replacement directly
                        // (initial expansion).
                        field_state.set_current_type(replacement);
                        raw_ref_branch_handled = true;
                    } else if type_expr_contains_named_recursive_ref(
                        field_state.published_type(),
                        target_name.as_str(),
                    ) {
                        let expanded = expand_named_recursive_refs_one_layer(
                            field_state.published_type(),
                            target_name.as_str(),
                            &replacement,
                        );
                        if expanded != *field_state.published_type() {
                            field_state.set_current_type(expanded);
                        }
                    } else {
                        // The define_props macro shape projection may have
                        // expanded the top-level type one layer (producing an
                        // Object body) while leaving nested self-references
                        // as bare `Ref{target}` instead of `RecursiveRef`.
                        // Rewrite those bare self-refs to `RecursiveRef` and
                        // expand one more layer so the transport carries the
                        // same two-level shape the RecursiveRef path would
                        // produce.
                        let with_recursive_refs = rewrite_named_self_refs_to_recursive_ref(
                            field_state.published_type(),
                            target_name.as_str(),
                        );
                        if with_recursive_refs != *field_state.published_type() {
                            let expanded = expand_named_recursive_refs_one_layer(
                                &with_recursive_refs,
                                target_name.as_str(),
                                &replacement,
                            );
                            if expanded != *field_state.published_type() {
                                field_state.set_current_type(expanded);
                            }
                        }
                    }
                }
            }
        }
        if !raw_ref_branch_handled {
            if let Some(raw) = parsed_field_raw_type(field).as_ref() {
                if let Some(replacement) =
                    indexed_access_alias_body_transport(scope_canonical_id, raw, query_engine)
                {
                    if !matches!(
                        field_state.published_type(),
                        verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                            | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                    ) {
                        field_state.set_current_type(replacement);
                    }
                }
            }
        }
        // Plan §4.10 / K1 — final publish + write-back to the field.
        field.r#type = field_state.publish();
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_MATERIALIZE_FINAL owner={} field={} final={:?}",
                scope_canonical_id, field.name, field.r#type,
            ));
        }
    }
    let finalized_prop_types = evaluated_types
        .props
        .iter()
        .map(|field| (field.name.clone(), field.r#type.clone()))
        .collect::<rustc_hash::FxHashMap<_, _>>();
    for define_props in &mut evaluated_types.define_props {
        for property in &mut define_props.result.value.properties {
            if let Some(finalized) = finalized_prop_types.get(&property.name) {
                property.ty = finalized.clone();
            }
        }
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "FIELD_DEFINE_PROPS_SYNC owner={} props={:?}",
                scope_canonical_id,
                define_props
                    .result
                    .value
                    .properties
                    .iter()
                    .map(|property| (property.name.clone(), property.ty.clone()))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    for field in &mut evaluated_types.emits {
        // Plan §4.10 / K1 — wrap field.r#type in MacroFieldGraphState.
        let host = query_engine.host;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        rescue_field(scope_canonical_id, field, &mut field_state, query_engine);
        field.r#type = field_state.publish();
    }
    for field in &mut evaluated_types.slot_bindings {
        // Plan §4.10 / K1 — wrap field.r#type in MacroFieldGraphState.
        let host = query_engine.host;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        rescue_field(scope_canonical_id, field, &mut field_state, query_engine);
        let Some(scope_hints) = slot_binding_scope_hints.get(&field.name) else {
            field.r#type = field_state.publish();
            continue;
        };
        for scope_hint in scope_hints {
            let parsed_raw = field
                .raw_type
                .as_deref()
                .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
            for candidate in [
                Some(field_state.published_type().clone()),
                parsed_raw.clone(),
                parsed_raw.as_ref().and_then(|raw| {
                    project_expr_class_a_via_dispatch(query_engine.host, scope_hint, raw)
                }),
                project_expr_class_a_via_dispatch(
                    query_engine.host,
                    scope_hint,
                    field_state.published_type(),
                ),
            ]
            .into_iter()
            .flatten()
            {
                let rescued = materialize_component_meta_type_expr_until_stable(
                    &candidate,
                    scope_hint,
                    crate::semantic_query::ProjectionMode::Expanded,
                    query_engine,
                );
                if compare_type_expr_improvement(&rescued, field_state.published_type()) {
                    field_state.set_current_type(rescued);
                }
                let surface =
                    query_engine.materialize_member_surface_expr(scope_hint, &candidate, false);
                if compare_type_expr_improvement(&surface, field_state.published_type()) {
                    field_state.set_current_type(surface);
                }
            }
        }
        field.r#type = field_state.publish();
    }
    for field in &mut evaluated_types.bindings {
        // Plan §4.10 / K1 — wrap field.r#type in MacroFieldGraphState.
        let host = query_engine.host;
        let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
        let mut field_state =
            MacroFieldGraphState::new(field.r#type.clone(), scope_canonical_id, &dispatch);
        rescue_field(scope_canonical_id, field, &mut field_state, query_engine);
        field.r#type = field_state.publish();
    }
}
/// Test-only call counter for `materialize_component_meta_type_expr_until_stable`
/// (plan §3 Step 6.2 / D22). Incremented at function entry — memo hits and
/// cold builds both count, since the counter discriminates the *entry*
/// invariant: did the caller route through whole-expression materialization
/// at all? The Step 6.2 FAIL-FIRST test asserts that route/project
/// candidates evaluated by `materialize_component_meta_macro_shape_member_type_expr`
/// satisfy the request without falling through to this entry, so the
/// counter stays at 0 in the success case.
#[cfg(test)]
pub(crate) static MTL_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Test accessor for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn mtl_call_count_for_tests() -> usize {
    MTL_CALL_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Test reset for [`MTL_CALL_COUNT`].
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn reset_mtl_call_count_for_tests() {
    MTL_CALL_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}
