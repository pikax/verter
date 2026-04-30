//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ProjectionMode::Identity` vs `ProjectionMode::Expanded`)
//! - materialized resolved outputs (`ResolvedComponentMetaState`)
//! - mode-aware caching
//! - JSDoc attachment and typed-tag resolution
//!
//! It calls into `host_resolve.rs` for declaration traversal â€” it does NOT
//! replace or duplicate the shared traversal substrate.
//!
//! # Architecture
//!
//! ```text
//! caller â†’ resolve_component_meta(canonical, mode)
//!            â†“
//!        meta_resolve.rs  (orchestration, materialization, caching)
//!            â†“
//!        host_resolve.rs  (declaration graph traversal, shared cache)
//! ```

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
};
use crate::resolver_core::{
    run_component_meta_request, ComponentMetaEvalOutputs, ComponentMetaRequestHost, RequestSource,
    SingleflightRole,
};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use verter_semantic::analysis::types::AnalyzedMacro;

pub(crate) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

// Phase 11a sub-module split — siblings live in `crates/verter_session/src/meta_resolve/`.
// The shell re-exports the moved `pub(crate)` surface so existing
// `crate::meta_resolve::*` paths keep working without callsite churn.
mod dep_signature;
mod dispatch_helpers;
mod field_state;
mod host_methods;
mod macro_member_walk;
mod materialize;
mod request_host;
mod resolved_state;
mod scoring;
pub(crate) use dep_signature::{
    accumulate_dispatch_dep_signature, drain_dispatch_dep_signature_accumulator,
    reset_dispatch_dep_signature_accumulator,
};
#[cfg(test)]
pub(crate) use dep_signature::{
    bfs_compute_counter_for_test, record_bfs_child_refs_count_for_test,
    reset_bfs_compute_counter_for_test, with_bfs_child_refs_observer_for_test, with_visited_counter,
};
pub(crate) use dispatch_helpers::{
    instantiate_local_generic_ref_via_dispatch, lower_and_project_to_expanded_via_host_threaded,
    pick_via_dispatch_pick_helper, project_expr_class_a_shape_via_dispatch,
    project_expr_class_a_shape_via_dispatch_threaded, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded,
    project_expr_surface_expr_via_host_threaded,
    project_expr_surface_expr_with_compound_objects_via_host_threaded,
    project_expr_surface_shape_via_host_threaded,
    project_prepared_type_surface_expr_via_host_threaded,
    project_prepared_type_surface_shape_via_host,
    project_prepared_type_surface_shape_via_host_threaded,
    project_route_surface_expr_via_host_threaded, project_type_surface_expr_via_host,
    project_type_surface_expr_via_host_threaded, project_type_surface_shape_via_host,
    project_type_surface_shape_via_host_threaded,
};
pub(crate) use field_state::MacroFieldGraphState;
#[cfg(test)]
pub(crate) use field_state::{dispatch_lower_counter_get, dispatch_lower_counter_reset};
pub use request_host::{
    CapturedComponentMetaInputs, ResolvedComponentMetaComputeAudit, ResolvedDeclarationKind,
    ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta, ResolvedNativeProp,
    ResolvedTypeDeclaration, ResolvedTypeRegistryMeta, SessionRequestHost,
};
pub(crate) use request_host::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    resolved_meta_cache_key, should_skip_imported_registry_seed_refresh, trace_request_source,
};
pub use resolved_state::{ResolvedComponentMetaState, SurfaceNodeIdentities};
pub(crate) use resolved_state::{
    collect_expanded_slot_binding_param_types, collect_expanded_slot_bindings_from_object_type,
    component_meta_owner_local_shallow_substituted_alias_body, component_meta_substitute_typeexpr,
    decide_typeexpr_conditional_with_function_extends, enrich_missing_slot_bindings,
    lowered_root_reaches_transitive_cycle, select_imported_materialization_scope,
    substitute_infer_in_typeexpr, walk_substitute_typeexpr, RegistryMaterialization,
};
pub(crate) use macro_member_walk::{
    materialize_component_meta_macro_shape_member_type_expr,
    walk_component_meta_macro_shape_member_types,
};
pub(crate) use scoring::compare_type_expr_improvement;
use scoring::component_meta_registry_prefers_structural_materialization;
pub(crate) use materialize::{
    collect_type_expr_ref_names, define_props_fields_fast_path_allowed,
    define_props_member_can_stay_symbolic_without_rescue, expr_needs_projection_rescue,
    field_should_preserve_shallow_symbolic_raw_type, has_prop_shape_surface,
    lowered_needs_member_route_materialization,
    lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_field_types,
    materialize_component_meta_type_expr_until_stable,
    materialize_component_meta_type_expr_until_stable_full,
    parsed_field_raw_type, produce_macro_object_shapes,
    produce_macro_object_shapes_for_purpose, produce_one_macro_object_shape,
    projection_result_beats_solver_shape, registry_entry_to_expanded_shape,
    synthesize_define_props_shape_from_known_surface_with_authority,
    top_level_imported_ref_can_stay_symbolic,
    type_expr_has_non_object_top_level_surface,
    type_expr_has_package_backed_object_like_root,
    type_expr_is_slots_member_route, MacroShapeSource,
};
#[cfg(test)]
pub(crate) use materialize::{mtl_call_count_for_tests, reset_mtl_call_count_for_tests};



use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route, component_meta_registry_raw_member_path_surface,
    enqueue_component_meta_registry_ref, merge_component_meta_registry_candidates,
    owner_component_meta_registry_import_root, upsert_component_meta_registry_entry,
    PendingComponentMetaRegistryRef,
};



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
pub(crate) fn component_meta_registry_prefers_structural_materialization_node(
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
        SemanticNodeData::Array { .. }
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::Union(_)
        | SemanticNodeData::Intersection(_)
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::Function { .. }
        | SemanticNodeData::KeyOf { .. } => true,
        SemanticNodeData::Alias(inner) => {
            component_meta_registry_prefers_structural_materialization_node(
                graph,
                *inner,
                depth + 1,
            )
        }
        SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::Object(_)
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::VueMacroElements(_) => false,
    }
}

fn materialize_component_meta_registry_structural_expr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};

    /// Plan §6.11 / J3 — graph-native package check on a lowered
    /// `Ref { name, [] }`. Lowers via Navigate to a DeclRef /
    /// InstantiationRef, extracts the canonical identity, and
    /// delegates to the J0 / commit-C primitive
    /// `component_meta_ref_resolves_to_package_node`. Falls back to
    /// `false` (not package-backed) when lowering fails or produces a
    /// non-Ref node — the closure's structural recursion path then
    /// projects through `project_type_surface_expr` like any other
    /// local Ref.
    fn ref_is_package_backed_node(host: &VerterHost, scope_canonical_id: &str, name: &str) -> bool {
        let dispatch = ProjectSemanticDispatch::new(host);
        let probe = verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        };
        let Some(node_id) = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            &probe,
            ProjectionMode::Navigate,
        ) else {
            return false;
        };
        let graph = host.project_type_store().semantic_graph();
        let Some(data) = graph.node_data(node_id) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                component_meta_ref_resolves_to_package_node(identity)
            }
            SemanticNodeData::InstantiationRef { base, .. } => {
                component_meta_ref_resolves_to_package_node(base)
            }
            _ => false,
        }
    }

    fn inner(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        scope_canonical_id: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        active: &mut rustc_hash::FxHashSet<SemanticNodeId>,
    ) -> verter_semantic::analysis::type_expr::TypeExpr {
        use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

        // Plan §6.11 / J3 — graph-native cycle guard. Lower the
        // current expr to a Navigate-mode SemanticNodeId and use
        // structural identity (interned node id) for cycle tracking
        // instead of TypeExpr-equality hashing. When lowering fails
        // (None), we cannot intern a key — proceed without cycle
        // tracking for this visit (TypeExpr-equality cycle tracking
        // would not have terminated either; the structural recursion
        // remains safe under the existing structural bounds).
        let dispatch_for_cycle = ProjectSemanticDispatch::new(engine.host);
        let cycle_key = dispatch_for_cycle.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Navigate,
        );
        if let Some(key) = cycle_key {
            if !active.insert(key) {
                return expr.clone();
            }
        }

        let result = if let Some((root_symbol, route)) =
            component_meta_registry_public_utility_route(expr)
                .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        {
            // Phase 5e commit 6 — migrate route-target callers to dispatch
            // (sub-plan §C.3 D-T recipe). Route the utility/indexed
            // expression through the Class A dispatch helper, which handles
            // Whole/MemberPath via its registry-route fast-path AND falls
            // back to the generic ProjectPath{[],Expanded} dispatch. Pick/Omit
            // route-targets reach the dispatch's `Instantiate` builtin
            // utility path internally via lower_type_expr_in_scope.
            //
            // The two-step engine resolution (try direct scope, then
            // declaration scope) is preserved so re-exported / barrel-routed
            // declarations resolve correctly.
            let _ = &route; // route demand carrier preserved for parity reads
            project_expr_class_a_via_dispatch_threaded(
                engine.host,
                Some(engine),
                scope_canonical_id,
                expr,
            )
            .or_else(|| {
                let declaration = engine.resolve_type_declaration(scope_canonical_id, &root_symbol);
                (!declaration.canonical_source.is_empty())
                    .then(|| {
                        project_expr_class_a_via_dispatch_threaded(
                            engine.host,
                            Some(engine),
                            declaration.canonical_source.as_str(),
                            expr,
                        )
                    })
                    .flatten()
            })
            .unwrap_or_else(|| expr.clone())
        } else {
            match expr {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } if type_arguments.is_empty() => {
                    // Plan §6.11 / J3 — graph-native package check.
                    if ref_is_package_backed_node(engine.host, scope_canonical_id, name) {
                        expr.clone()
                    } else {
                        // Phase 5m §5.13a.2 — bridge via per-engine
                        // helper.
                        project_type_surface_expr_via_host_threaded(
                            engine,
                            scope_canonical_id,
                            name,
                        )
                        .or_else(|| {
                            let declaration =
                                engine.resolve_type_declaration(scope_canonical_id, name);
                            (!declaration.canonical_source.is_empty())
                                .then(|| {
                                    project_type_surface_expr_via_host_threaded(
                                        engine,
                                        declaration.canonical_source.as_str(),
                                        if declaration.resolved_name.is_empty() {
                                            name
                                        } else {
                                            declaration.resolved_name.as_str()
                                        },
                                    )
                                })
                                .flatten()
                        })
                        .unwrap_or_else(|| expr.clone())
                    }
                }
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => TypeExpr::Ref {
                    name: name.clone(),
                    type_arguments: Arc::from(
                        type_arguments
                            .iter()
                            .map(|arg| inner(arg, scope_canonical_id, engine, active))
                            .collect::<Vec<_>>(),
                    ),
                },
                TypeExpr::Parenthesized(inner_expr) => TypeExpr::Parenthesized(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Array { element, readonly } => TypeExpr::Array {
                    element: Arc::new(inner(element, scope_canonical_id, engine, active)),
                    readonly: *readonly,
                },
                TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                    elements: Arc::from(
                        elements
                            .iter()
                            .map(
                                |element| verter_semantic::analysis::type_expr::TupleElement {
                                    label: element.label.clone(),
                                    ty: inner(&element.ty, scope_canonical_id, engine, active),
                                    optional: element.optional,
                                    rest: element.rest,
                                },
                            )
                            .collect::<Vec<_>>(),
                    ),
                    readonly: *readonly,
                },
                TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
                    types
                        .iter()
                        .map(|ty| inner(ty, scope_canonical_id, engine, active))
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
                    types
                        .iter()
                        .map(|ty| inner(ty, scope_canonical_id, engine, active))
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => TypeExpr::Conditional {
                    check: Arc::new(inner(check, scope_canonical_id, engine, active)),
                    extends: Arc::new(inner(extends, scope_canonical_id, engine, active)),
                    true_type: Arc::new(inner(true_type, scope_canonical_id, engine, active)),
                    false_type: Arc::new(inner(false_type, scope_canonical_id, engine, active)),
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
                    source: Arc::new(inner(source, scope_canonical_id, engine, active)),
                    optional: *optional,
                    readonly: *readonly,
                    name_type: name_type.as_deref().map(|name_type| {
                        Arc::new(inner(name_type, scope_canonical_id, engine, active))
                    }),
                    value: Arc::new(inner(value, scope_canonical_id, engine, active)),
                },
                TypeExpr::TemplateLiteral {
                    quasis,
                    expressions,
                } => TypeExpr::TemplateLiteral {
                    quasis: quasis.clone(),
                    expressions: Arc::from(
                        expressions
                            .iter()
                            .map(|expr| inner(expr, scope_canonical_id, engine, active))
                            .collect::<Vec<_>>(),
                    ),
                },
                TypeExpr::Function(function) => {
                    let mut function = function.as_ref().clone();
                    for parameter in &mut function.parameters {
                        parameter.ty = inner(&parameter.ty, scope_canonical_id, engine, active);
                    }
                    if let Some(return_type) = function.return_type.as_mut() {
                        *return_type =
                            Arc::new(inner(return_type, scope_canonical_id, engine, active));
                    }
                    for type_parameter in &mut function.type_parameters {
                        if let Some(constraint) = type_parameter.constraint.as_mut() {
                            *constraint =
                                Arc::new(inner(constraint, scope_canonical_id, engine, active));
                        }
                        if let Some(default) = type_parameter.default.as_mut() {
                            *default = Arc::new(inner(default, scope_canonical_id, engine, active));
                        }
                    }
                    TypeExpr::Function(Arc::new(function))
                }
                TypeExpr::KeyOf(inner_expr) => TypeExpr::KeyOf(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Rest(inner_expr) => TypeExpr::Rest(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                ))),
                TypeExpr::Object(object) => {
                    let mut object = object.as_ref().clone();
                    for member in &mut object.properties {
                        match member {
                            ObjectMember::Property(property) => {
                                property.ty =
                                    inner(&property.ty, scope_canonical_id, engine, active);
                            }
                            ObjectMember::IndexSignature(signature) => {
                                signature.key_type =
                                    inner(&signature.key_type, scope_canonical_id, engine, active);
                                signature.value_type = inner(
                                    &signature.value_type,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                );
                            }
                            ObjectMember::CallSignature(function)
                            | ObjectMember::ConstructSignature(function) => {
                                for parameter in &mut function.parameters {
                                    parameter.ty =
                                        inner(&parameter.ty, scope_canonical_id, engine, active);
                                }
                                if let Some(return_type) = function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                    ));
                                }
                            }
                            ObjectMember::Method(method) => {
                                for parameter in &mut method.function.parameters {
                                    parameter.ty =
                                        inner(&parameter.ty, scope_canonical_id, engine, active);
                                }
                                if let Some(return_type) = method.function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                    ));
                                }
                            }
                        }
                    }
                    TypeExpr::Object(Arc::new(object))
                }
                TypeExpr::IndexedAccess { .. }
                | TypeExpr::Primitive(_)
                | TypeExpr::Literal(_)
                | TypeExpr::Unknown { .. }
                | TypeExpr::RecursiveRef { .. }
                | TypeExpr::TypeOf(_)
                | TypeExpr::TypeParameter(_)
                | TypeExpr::Infer { .. } => expr.clone(),
            }
        };

        if let Some(key) = cycle_key {
            active.remove(&key);
        }
        result
    }

    let mut active: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    inner(expr, scope_canonical_id, engine, &mut active)
}

/// Plan §1.12 / J4 — graph-native predicate (former TypeExpr
/// counterpart deleted in Plan §6.15 / N). Walks two parallel
/// `SemanticNodeId` trees (materialised + raw) and, when the raw
/// surface exposes a package-backed `DeclRef` / `InstantiationRef` at
/// a given member, overrides the materialised member's value with
/// the raw graph node so the symbolic Ref is preserved through
/// materialisation.
///
/// Mirrors the TypeExpr predicate's branch structure:
///
/// - `(Object(materialized_surface), Object(raw_surface))` —
///   walk parallel members keyed by name. For each materialised
///   member with a matching raw member: when the raw member's
///   value is a `DeclRef` / `InstantiationRef` whose root identity
///   is package-backed (via
///   [`component_meta_ref_resolves_to_package_node`]), replace
///   the materialised member's value with the raw node; otherwise
///   recurse into the parallel pair. Returns a freshly interned
///   Object with the rebuilt member list.
/// - All other shape pairs — return `materialized` unchanged
///   (matches the TypeExpr `_ => materialized.clone()` arm).
///
/// Members of the materialised surface that do NOT have a matching
/// raw member by name are preserved unchanged.
///
/// Depth fused at 256 per §4.11. Fuse returns `materialized`
/// unchanged.
pub(crate) fn preserve_package_backed_symbolic_refs_node(
    host: &VerterHost,
    materialized: crate::semantic_query::SemanticNodeId,
    raw: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{SemanticNodeData, SurfaceMember, SurfaceView};
    use rustc_hash::FxHashMap;

    if depth > 256 {
        return materialized;
    }
    let graph = host.project_type_store().semantic_graph();
    let materialized_data = graph.node_data(materialized);
    let raw_data = graph.node_data(raw);
    let (Some(m_data), Some(r_data)) = (materialized_data, raw_data) else {
        return materialized;
    };
    match (m_data.as_ref(), r_data.as_ref()) {
        (SemanticNodeData::Object(materialized_surface), SemanticNodeData::Object(raw_surface)) => {
            // Build a name -> raw member map for O(1) parallel lookup.
            let mut raw_members: FxHashMap<&str, &SurfaceMember> =
                FxHashMap::with_capacity_and_hasher(raw_surface.members.len(), Default::default());
            for raw_member in raw_surface.members.iter() {
                raw_members.insert(raw_member.name.as_ref(), raw_member);
            }

            let mut new_members: Vec<SurfaceMember> =
                Vec::with_capacity(materialized_surface.members.len());
            let mut any_changed = false;
            for materialised_member in materialized_surface.members.iter() {
                let Some(&raw_member) = raw_members.get(materialised_member.name.as_ref()) else {
                    new_members.push(materialised_member.clone());
                    continue;
                };
                // Check whether the raw member's value is a
                // package-backed Ref. If so, override.
                let raw_value_data = graph.node_data(raw_member.value);
                let raw_is_package_backed = raw_value_data
                    .as_deref()
                    .map(|d| match d {
                        SemanticNodeData::DeclRef { identity } => {
                            component_meta_ref_resolves_to_package_node(identity)
                        }
                        SemanticNodeData::InstantiationRef { base, .. } => {
                            component_meta_ref_resolves_to_package_node(base)
                        }
                        _ => false,
                    })
                    .unwrap_or(false);
                if raw_is_package_backed {
                    if materialised_member.value != raw_member.value {
                        any_changed = true;
                    }
                    new_members.push(SurfaceMember {
                        name: Arc::clone(&materialised_member.name),
                        value: raw_member.value,
                        optional: materialised_member.optional,
                        readonly: materialised_member.readonly,
                        is_method: materialised_member.is_method,
                    });
                    continue;
                }
                // Recurse into the parallel pair.
                let recursed = preserve_package_backed_symbolic_refs_node(
                    host,
                    materialised_member.value,
                    raw_member.value,
                    depth + 1,
                );
                if recursed != materialised_member.value {
                    any_changed = true;
                }
                new_members.push(SurfaceMember {
                    name: Arc::clone(&materialised_member.name),
                    value: recursed,
                    optional: materialised_member.optional,
                    readonly: materialised_member.readonly,
                    is_method: materialised_member.is_method,
                });
            }
            if !any_changed {
                return materialized;
            }
            let new_surface = SurfaceView {
                members: Arc::from(new_members.into_boxed_slice()),
                call_signatures: Arc::clone(&materialized_surface.call_signatures),
                construct_signatures: Arc::clone(&materialized_surface.construct_signatures),
                index_signatures: Arc::clone(&materialized_surface.index_signatures),
                keyspace: materialized_surface.keyspace,
                has_index_signature: materialized_surface.has_index_signature,
            };
            graph.intern_node(SemanticNodeData::Object(new_surface))
        }
        _ => materialized,
    }
}

fn preserve_registry_callable_param_member_routes(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use rustc_hash::FxHashMap;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    fn inner(materialized: &TypeExpr, raw: &TypeExpr, preserve_routes: bool) -> TypeExpr {
        if preserve_routes
            && (component_meta_registry_public_utility_route(raw).is_some()
                || component_meta_registry_public_indexed_access_route(raw).is_some())
        {
            return raw.clone();
        }

        match (materialized, raw) {
            (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
                let mut object = materialized_object.as_ref().clone();
                let mut raw_properties = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                let mut raw_methods = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                let mut raw_callables = FxHashMap::with_capacity_and_hasher(
                    raw_object.properties.len(),
                    Default::default(),
                );
                for candidate in &raw_object.properties {
                    match candidate {
                        ObjectMember::Property(property) => {
                            raw_properties.insert(property.name.as_str(), property);
                            if let TypeExpr::Function(function) = &property.ty {
                                raw_callables
                                    .insert(property.name.as_str(), function.as_ref().clone());
                            }
                        }
                        ObjectMember::Method(method) => {
                            raw_methods.insert(method.name.as_str(), method);
                            raw_callables.insert(method.name.as_str(), method.function.clone());
                        }
                        _ => {}
                    }
                }
                for member in &mut object.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            if let Some(raw_property) = raw_properties.get(property.name.as_str()) {
                                property.ty =
                                    inner(&property.ty, &raw_property.ty, preserve_routes);
                            } else if let TypeExpr::Function(function) = &property.ty {
                                if let Some(raw_callable) =
                                    raw_callables.get(property.name.as_str())
                                {
                                    property.ty = inner(
                                        &TypeExpr::Function(function.clone()),
                                        &TypeExpr::Function(std::sync::Arc::new(
                                            raw_callable.clone(),
                                        )),
                                        preserve_routes,
                                    );
                                }
                            }
                        }
                        ObjectMember::Method(method) => {
                            if let Some(raw_method) = raw_methods.get(method.name.as_str()) {
                                method.function = match inner(
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        method.function.clone(),
                                    )),
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        raw_method.function.clone(),
                                    )),
                                    preserve_routes,
                                ) {
                                    TypeExpr::Function(function) => function.as_ref().clone(),
                                    _ => method.function.clone(),
                                };
                            } else if let Some(raw_callable) =
                                raw_callables.get(method.name.as_str())
                            {
                                method.function = match inner(
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        method.function.clone(),
                                    )),
                                    &TypeExpr::Function(std::sync::Arc::new(raw_callable.clone())),
                                    preserve_routes,
                                ) {
                                    TypeExpr::Function(function) => function.as_ref().clone(),
                                    _ => method.function.clone(),
                                };
                            }
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            let _ = function;
                        }
                        ObjectMember::IndexSignature(_) => {}
                    }
                }
                TypeExpr::Object(std::sync::Arc::new(object))
            }
            (TypeExpr::Function(materialized_function), TypeExpr::Function(raw_function)) => {
                let mut function = materialized_function.as_ref().clone();
                for (parameter, raw_parameter) in function
                    .parameters
                    .iter_mut()
                    .zip(raw_function.parameters.iter())
                {
                    parameter.ty = inner(&parameter.ty, &raw_parameter.ty, true);
                }
                if let (Some(return_type), Some(raw_return_type)) = (
                    function.return_type.as_mut(),
                    raw_function.return_type.as_deref(),
                ) {
                    *return_type = std::sync::Arc::new(inner(return_type, raw_return_type, false));
                }
                TypeExpr::Function(std::sync::Arc::new(function))
            }
            _ => materialized.clone(),
        }
    }

    inner(materialized, raw, false)
}

fn nested_symbolic_member_route_should_stay_symbolic(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn declaration_body_keeps_nested_member_route_symbolic(body: &TypeExpr) -> bool {
        match body {
            TypeExpr::Parenthesized(inner) => {
                declaration_body_keeps_nested_member_route_symbolic(inner)
            }
            TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty() => true,
            _ => component_meta_registry_public_utility_route(body)
                .or_else(|| component_meta_registry_public_indexed_access_route(body))
                .is_some(),
        }
    }

    let Some((root_name, _)) = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
    else {
        return false;
    };
    let declaration = engine.resolve_type_declaration(scope_canonical_id, root_name.as_str());
    let declaration_scope = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name
    } else {
        declaration.resolved_name
    };
    let Some(body) = engine.named_decl_body(declaration_scope.as_str(), declaration_name.as_str())
    else {
        return false;
    };
    declaration_body_keeps_nested_member_route_symbolic(&body)
}

fn type_expr_contains_public_member_route(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    if component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .is_some()
    {
        return true;
    }

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => type_expr_contains_public_member_route(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_contains_public_member_route(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(type_expr_contains_public_member_route)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => {
                type_expr_contains_public_member_route(&property.ty)
            }
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
                    || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_public_member_route)
            }
            ObjectMember::IndexSignature(signature) => {
                type_expr_contains_public_member_route(&signature.key_type)
                    || type_expr_contains_public_member_route(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(type_expr_contains_public_member_route)
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .any(|param| type_expr_contains_public_member_route(&param.ty))
                || function
                    .return_type
                    .as_deref()
                    .is_some_and(type_expr_contains_public_member_route)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_contains_public_member_route(check)
                || type_expr_contains_public_member_route(extends)
                || type_expr_contains_public_member_route(true_type)
                || type_expr_contains_public_member_route(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_contains_public_member_route(source)
                || type_expr_contains_public_member_route(value)
                || name_type
                    .as_deref()
                    .is_some_and(type_expr_contains_public_member_route)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_contains_public_member_route),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_contains_public_member_route),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_contains_public_member_route(object)
                || type_expr_contains_public_member_route(index)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn type_expr_needs_nested_symbolic_route_preservation(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => type_expr_needs_nested_symbolic_route_preservation(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| type_expr_needs_nested_symbolic_route_preservation(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            ObjectMember::Property(property) => match &property.ty {
                TypeExpr::Function(function) => function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty)),
                other => type_expr_needs_nested_symbolic_route_preservation(other),
            },
            ObjectMember::Method(method) => method
                .function
                .parameters
                .iter()
                .any(|param| type_expr_contains_public_member_route(&param.ty)),
            ObjectMember::IndexSignature(signature) => {
                type_expr_needs_nested_symbolic_route_preservation(&signature.key_type)
                    || type_expr_needs_nested_symbolic_route_preservation(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function
                    .parameters
                    .iter()
                    .any(|param| type_expr_contains_public_member_route(&param.ty))
            }
        }),
        TypeExpr::Function(function) => function
            .parameters
            .iter()
            .any(|param| type_expr_contains_public_member_route(&param.ty)),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            type_expr_needs_nested_symbolic_route_preservation(check)
                || type_expr_needs_nested_symbolic_route_preservation(extends)
                || type_expr_needs_nested_symbolic_route_preservation(true_type)
                || type_expr_needs_nested_symbolic_route_preservation(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            type_expr_needs_nested_symbolic_route_preservation(source)
                || type_expr_needs_nested_symbolic_route_preservation(value)
                || name_type
                    .as_deref()
                    .is_some_and(type_expr_needs_nested_symbolic_route_preservation)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .any(type_expr_needs_nested_symbolic_route_preservation),
        TypeExpr::IndexedAccess { object, index } => {
            type_expr_needs_nested_symbolic_route_preservation(object)
                || type_expr_needs_nested_symbolic_route_preservation(index)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn preserve_nested_symbolic_member_routes(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested: bool,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    let should_keep_symbolic = nested
        && nested_symbolic_member_route_should_stay_symbolic(raw, scope_canonical_id, engine);
    if should_keep_symbolic {
        return raw.clone();
    }

    match (materialized, raw) {
        (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
            let mut object = materialized_object.as_ref().clone();
            let raw_properties = raw_object
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(property) => Some((property.name.as_str(), member)),
                    ObjectMember::Method(method) => Some((method.name.as_str(), member)),
                    _ => None,
                })
                .collect::<rustc_hash::FxHashMap<_, _>>();
            for member in &mut object.properties {
                match member {
                    ObjectMember::Property(property) => {
                        let Some(raw_member) = raw_properties.get(property.name.as_str()) else {
                            continue;
                        };
                        let ObjectMember::Property(raw_property) = raw_member else {
                            continue;
                        };
                        property.ty = preserve_nested_symbolic_member_routes(
                            &property.ty,
                            &raw_property.ty,
                            scope_canonical_id,
                            engine,
                            true,
                        );
                    }
                    ObjectMember::Method(method) => {
                        let Some(raw_member) = raw_properties.get(method.name.as_str()) else {
                            continue;
                        };
                        let ObjectMember::Method(raw_method) = raw_member else {
                            continue;
                        };
                        for (param, raw_param) in method
                            .function
                            .parameters
                            .iter_mut()
                            .zip(raw_method.function.parameters.iter())
                        {
                            param.ty = preserve_nested_symbolic_member_routes(
                                &param.ty,
                                &raw_param.ty,
                                scope_canonical_id,
                                engine,
                                true,
                            );
                        }
                        if let (Some(return_type), Some(raw_return_type)) = (
                            method.function.return_type.as_mut(),
                            raw_method.function.return_type.as_deref(),
                        ) {
                            *return_type = Arc::new(preserve_nested_symbolic_member_routes(
                                return_type,
                                raw_return_type,
                                scope_canonical_id,
                                engine,
                                true,
                            ));
                        }
                    }
                    _ => {}
                }
            }
            TypeExpr::Object(Arc::new(object))
        }
        (TypeExpr::Function(materialized_function), TypeExpr::Function(raw_function)) => {
            let mut function = materialized_function.as_ref().clone();
            for (param, raw_param) in function
                .parameters
                .iter_mut()
                .zip(raw_function.parameters.iter())
            {
                param.ty = preserve_nested_symbolic_member_routes(
                    &param.ty,
                    &raw_param.ty,
                    scope_canonical_id,
                    engine,
                    true,
                );
            }
            if let (Some(return_type), Some(raw_return_type)) = (
                function.return_type.as_mut(),
                raw_function.return_type.as_deref(),
            ) {
                *return_type = Arc::new(preserve_nested_symbolic_member_routes(
                    return_type,
                    raw_return_type,
                    scope_canonical_id,
                    engine,
                    true,
                ));
            }
            TypeExpr::Function(Arc::new(function))
        }
        (TypeExpr::Parenthesized(materialized_inner), TypeExpr::Parenthesized(raw_inner)) => {
            TypeExpr::Parenthesized(Arc::new(preserve_nested_symbolic_member_routes(
                materialized_inner,
                raw_inner,
                scope_canonical_id,
                engine,
                nested,
            )))
        }
        _ => materialized.clone(),
    }
}

pub(crate) fn component_meta_registry_should_keep_raw_symbolic_non_object_alias(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    fn ref_stays_symbolic_in_registry(
        scope_canonical_id: &str,
        name: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> bool {
        if engine
            .resolve_direct_prepared_type_declaration(scope_canonical_id, name)
            .is_some()
        {
            return false;
        }
        engine
            .resolve_imported_registry_symbol(scope_canonical_id, name)
            .map(|resolved| resolved.canonical_id.contains("/node_modules/"))
            .unwrap_or(true)
    }

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            ref_stays_symbolic_in_registry(scope_canonical_id, name.as_ref(), engine)
                && type_arguments.iter().all(|arg| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        arg,
                        scope_canonical_id,
                        engine,
                    )
                })
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                element,
                scope_canonical_id,
                engine,
            )
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &element.ty,
                scope_canonical_id,
                engine,
            )
        }),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types.iter().all(|ty| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                ty,
                scope_canonical_id,
                engine,
            )
        }),
        TypeExpr::Function(func) => {
            func.parameters.iter().all(|param| {
                component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    &param.ty,
                    scope_canonical_id,
                    engine,
                )
            }) && func.return_type.as_deref().is_none_or(|return_type| {
                component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    return_type,
                    scope_canonical_id,
                    engine,
                )
            }) && func.type_parameters.iter().all(|param| {
                param.constraint.as_deref().is_none_or(|constraint| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        constraint,
                        scope_canonical_id,
                        engine,
                    )
                }) && param.default.as_deref().is_none_or(|default| {
                    component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        default,
                        scope_canonical_id,
                        engine,
                    )
                })
            })
        }
        TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_) => false,
    }
}

// Plan §6.5 / D — the TypeExpr-keyed free package-ref check was
// retired in commit D. The 5 callers migrated to a temporary engine
// method adapter (commit D), which was itself retired in commit O
// after Phase 11 K3 migrated production callers to graph-native
// predicates. The graph-native primitive
// `component_meta_ref_resolves_to_package_node` is the canonical
// authority for package-backed decl identity.

// Plan §6.6 / E — the inline-registry-route candidate family was
// retired in commit E. The inline-registry-route candidate path is
// handled by B1's materialiser registry-route branch, which
// dispatches Pick/Omit + IndexedAccess shapes through dispatch's
// canonical projection. Retired symbols are listed in the
// `RETIRED_SYMBOLS` array of the static-grep gate test (commit I).

// ===========================================================================
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
fn extract_pick_omit_route(
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
fn extract_indexed_access_route(
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
fn collect_string_literal_union_keys_node(
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
fn bfs_compute_inner(
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
fn body_contains_recursive_ref_to_name(
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
fn has_complex_cycle_guard_surface_node(
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

// Plan §6.10 sub-task 4 / §4.19 — registry-route inline composition
// predicate deleted (verified callerless in production; the only
// consumer was a composition test that has also been deleted in this
// commit).

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn build_origin_graph(
    graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>,
    surface_identities: Option<&SurfaceNodeIdentities>,
) -> verter_protocol::types::OriginGraphDto {
    use crate::semantic_query::OriginEdgeKind;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::VecDeque;
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    // Step 9.2 / F6 scoped origin export: when surface_identities are
    // populated, reverse-walk via walk_origin_chain starting from each
    // surface node and collect only the reachable subgraph. Falls back
    // to export_all_origin_edges when surface_identities is None
    // (audit-off path or pre-populated state).
    let all_edges = if let Some(ids) = surface_identities {
        let mut roots: Vec<crate::semantic_query::SemanticNodeId> = Vec::new();
        let push_some =
            |roots: &mut Vec<_>, opt: &Option<crate::semantic_query::SemanticNodeId>| {
                if let Some(id) = opt {
                    roots.push(*id);
                }
            };
        for id in &ids.prop_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.emit_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.slot_binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.registry_node_ids {
            push_some(&mut roots, id);
        }
        if roots.is_empty() {
            return OriginGraphDto::default();
        }
        let mut reached: FxHashSet<crate::semantic_query::SemanticNodeId> = FxHashSet::default();
        let mut worklist: VecDeque<crate::semantic_query::SemanticNodeId> =
            roots.into_iter().collect();
        let mut collected: Vec<(
            crate::semantic_query::SemanticNodeId,
            OriginEdgeKind,
            crate::semantic_query::OriginEdge,
        )> = Vec::new();
        while let Some(node) = worklist.pop_front() {
            if !reached.insert(node) {
                continue;
            }
            graph.walk_origin_chain(node, |kind, edge| {
                collected.push((node, kind, edge.clone()));
                for source in edge.sources.iter() {
                    if !reached.contains(source) {
                        worklist.push_back(*source);
                    }
                }
            });
        }
        collected
    } else {
        graph.export_all_origin_edges()
    };

    if all_edges.is_empty() {
        return OriginGraphDto::default();
    }

    let mut node_index: FxHashMap<crate::semantic_query::SemanticNodeId, u32> =
        FxHashMap::default();
    let mut nodes: Vec<OriginNodeDto> = Vec::new();
    let mut meta_strings: Vec<String> = Vec::new();
    let mut meta_index_map: FxHashMap<String, u32> = FxHashMap::default();

    let mut intern_node = |id: crate::semantic_query::SemanticNodeId,
                           graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>|
     -> u32 {
        if let Some(&idx) = node_index.get(&id) {
            return idx;
        }
        let idx = nodes.len() as u32;
        let (kind, label) = graph
            .node_data(id)
            .map(|d| {
                use crate::semantic_query::SemanticNodeData;
                let k = format!("{:?}", &*d).split_once('{').map_or_else(
                    || {
                        format!("{:?}", &*d)
                            .split_once('(')
                            .map_or_else(|| format!("{:?}", &*d), |(name, _)| name.to_string())
                    },
                    |(name, _)| name.to_string(),
                );
                let l = match &*d {
                    SemanticNodeData::Primitive(p) => Some(format!("{p:?}").to_lowercase()),
                    SemanticNodeData::Object(_) => Some("{...}".to_string()),
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(display_name.to_string())
                    }
                    SemanticNodeData::Literal(lit) => Some(format!("{lit:?}")),
                    SemanticNodeData::Array { readonly, .. } => {
                        Some(if *readonly { "readonly T[]" } else { "T[]" }.to_string())
                    }
                    SemanticNodeData::Tuple { .. } => Some("[...]".to_string()),
                    SemanticNodeData::Union(_) => Some("A | B".to_string()),
                    SemanticNodeData::Intersection(_) => Some("A & B".to_string()),
                    SemanticNodeData::Function { .. } => Some("(...) => R".to_string()),
                    _ => None,
                };
                (k, l)
            })
            .unwrap_or_else(|| ("Unknown".to_string(), None));
        nodes.push(OriginNodeDto {
            id: idx,
            kind,
            label,
        });
        node_index.insert(id, idx);
        idx
    };

    let mut edges_dto: Vec<OriginEdgeDto> = Vec::new();
    for (target_node, kind, edge) in &all_edges {
        let target_idx = intern_node(*target_node, graph);
        let edge_kind = match kind {
            OriginEdgeKind::Instantiate => "instantiate",
            OriginEdgeKind::SubstituteTypeParam => "substituteTypeParam",
            OriginEdgeKind::ConditionalSelect => "conditionalSelect",
            OriginEdgeKind::InferBind => "inferBind",
            OriginEdgeKind::ProjectMember => "projectMember",
            OriginEdgeKind::ProjectIndex => "projectIndex",
            OriginEdgeKind::ProjectPath => "projectPath",
            OriginEdgeKind::Normalize => "normalize",
            OriginEdgeKind::AliasResolve => "aliasResolve",
        };
        let meta_str = format!("{:?}", edge.meta);
        let meta_idx = if meta_str == "None" {
            None
        } else {
            let idx = if let Some(&existing) = meta_index_map.get(&meta_str) {
                existing
            } else {
                let idx = meta_strings.len() as u32;
                meta_strings.push(meta_str.clone());
                meta_index_map.insert(meta_str, idx);
                idx
            };
            Some(idx)
        };
        for source in edge.sources.iter() {
            let source_idx = intern_node(*source, graph);
            edges_dto.push(OriginEdgeDto {
                source: source_idx,
                target: target_idx,
                kind: edge_kind.to_string(),
                meta_index: meta_idx,
            });
        }
    }

    OriginGraphDto {
        nodes,
        edges: edges_dto,
        meta_strings,
    }
}


struct HostComponentMetaResolver<'a> {
    host: &'a VerterHost,
}

impl crate::resolver_core::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        self.host
            .resolve_named_type_export_target(dep_canonical, requested_name)
            .map(
                |(canonical, name)| crate::resolver_core::ResolvedExportTarget {
                    source_canonical_id: (canonical != dep_canonical).then_some(canonical),
                    source_name: name,
                },
            )
    }

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span> {
        self.host
            .get_export_span_follow_reexports(dep_canonical, requested_name)
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        self.host
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(from_canonical, import_source)
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_direct_type_reexport_target(dep_canonical, requested_name)
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_local_import_symbol_target(dep_canonical, resolved_name)
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host
            .resolve_local_export_symbol_target(canonical_source, exported_name)
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self.host.external_type_analysis(canonical_source)?;
        let symbol = analysis.local_type_symbol(resolved_name)?;
        let kind = match symbol.kind {
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                crate::resolver_core::ResolvedDeclarationKind::TypeAlias
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
                crate::resolver_core::ResolvedDeclarationKind::Interface
            }
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
                crate::resolver_core::ResolvedDeclarationKind::Class
            }
        };
        Some(crate::resolver_core::ResolvedLocalTypeSymbolMetadata {
            kind,
            span: symbol.span,
        })
    }
}

impl crate::resolver_core::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        resolve_type_declaration(self.host, dep_canonical, requested_name)
    }

    fn snapshot_imports<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedImport] {
        snapshot.imports.as_slice()
    }

    fn snapshot_macros<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedMacro] {
        snapshot.macros.as_slice()
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        snapshot.macro_type_deps.as_slice()
    }

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ProjectionMode::Expanded,
                snapshot.imports.len(),
                snapshot.macro_type_deps.len(),
            ));
        }
        // Tracked dependencies: snapshot-level candidates + solver-discovered deps.
        // The legacy walker is no longer used for dependency tracking.
        let mut tracked_dependencies = std::collections::BTreeSet::new();
        tracked_dependencies.extend(
            eval_context
                .map(|captured| captured.direct_dependency_candidates.clone())
                .unwrap_or_else(|| {
                    self.host
                        .cache_dependency_candidates_from_snapshot(owner_canonical, snapshot)
                }),
        );
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // D-Cutover §5.8 WIP-W: the retired `shared_owner_engine` path
        // is gone; all callers go through
        // `compute_evaluated_types_with_tracking_from_owner_context`
        // which internally builds any needed host bridge.
        let computed_eval_types = self
            .host
            .compute_evaluated_types_with_tracking_from_owner_context(
                owner_canonical,
                snapshot,
                eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                purpose,
            );
        if let Some(compute_eval_start) = compute_eval_start {
            let elapsed = compute_eval_start.elapsed();
            component_meta_debug(format!(
                "EVAL_TYPES owner={} elapsed_ms={:.1} has_result={}",
                owner_canonical,
                elapsed.as_secs_f64() * 1000.0,
                computed_eval_types.is_some(),
            ));
        }
        if let Some(computed) = computed_eval_types.as_ref() {
            tracked_dependencies.extend(computed.discovered_dependencies.iter().cloned());
        }
        let (evaluated_types, surface_identities) = computed_eval_types
            .map(|computed| (computed.evaluated_types, computed.surface_identities))
            .unwrap_or((None, None));
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ProjectionMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            tracked_dependencies,
            surface_identities,
        }
    }

    fn projectable_owner_local_macro_roots(
        &self,
        owner_canonical: &str,
        mac: &verter_semantic::analysis::types::AnalyzedMacro,
    ) -> Vec<String> {
        fn macro_lacks_direct_local_surface(
            mac: &verter_semantic::analysis::types::AnalyzedMacro,
        ) -> bool {
            match mac.kind {
                verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                | verter_semantic::analysis::AnalyzedMacroKind::DefineModel => {
                    mac.prop_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                    mac.emit_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                    mac.slot_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
            }
        }

        let mut candidate_roots = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for resolved in &mac.resolved_local_types {
            let is_direct_local = mac
                .type_references
                .iter()
                .any(|type_name| type_name == &resolved.name);
            if is_direct_local && seen.insert(resolved.name.as_str()) {
                candidate_roots.push(resolved.name.as_str());
            }
        }

        if candidate_roots.is_empty() && macro_lacks_direct_local_surface(mac) {
            let owner_has_symbol = self.host.route_owned_shallow_state(owner_canonical);
            for type_name in &mac.type_references {
                if type_name.contains('.') || !seen.insert(type_name.as_str()) {
                    continue;
                }
                let owner_local_decl = owner_has_symbol
                    .as_ref()
                    .is_some_and(|state| state.symbol(type_name).is_some())
                    || self
                        .resolve_type_declaration(owner_canonical, type_name)
                        .canonical_source
                        == owner_canonical;
                if owner_local_decl {
                    candidate_roots.push(type_name.as_str());
                }
            }
        }

        if candidate_roots.is_empty() {
            return Vec::new();
        }

        // Phase 5m §5.13a.2 — bridge via per-engine helper.
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);

        candidate_roots
            .into_iter()
            .filter(|root_name| {
                project_prepared_type_surface_shape_via_host_threaded(
                    &mut query_engine,
                    owner_canonical,
                    root_name,
                )
                .is_some_and(|shape| match mac.kind {
                    verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                    | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineModel
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => true,
                    verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                        !shape.properties.is_empty() || !shape.call_signatures.is_empty()
                    }
                    verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
                })
            })
            .map(str::to_string)
            .collect()
    }

    fn resolve_owner_local_macro_surface(
        &self,
        owner_canonical: &str,
        root_name: &str,
        macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
    ) -> Option<crate::resolver_core::surface_projector::ProjectedMacroSurfaces> {
        // Phase 5m §5.13a.2 — bridge via per-engine helper.
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);
        let shape = project_prepared_type_surface_shape_via_host_threaded(
            &mut query_engine,
            owner_canonical,
            root_name,
        )?;
        Some(
            crate::resolver_core::component_meta::project_macro_surfaces_from_expanded_shape(
                macro_kind, &shape,
            ),
        )
    }

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
    }

    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_surface(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        resolve_jsdoc_block(
            self.host,
            canonical_source,
            span,
            if expanded {
                ProjectionMode::Expanded
            } else {
                ProjectionMode::Identity
            },
            tracked_deps,
            cache,
            visiting,
            verter_workspace::ResolveRequestKind::TypeImport,
        )
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) {
        self.host
            .sync_transitive_macro_type_dependencies(canonical_id, tracked_deps);
    }

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host
            .current_dependency_fact_versions(canonical, tracked_deps)
    }
}

pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let resolver = HostComponentMetaResolver { host };
    let key =
        crate::resolver_core::symbol_resolver::declaration_node_key(dep_canonical, requested_name);
    let mut ctx = crate::resolver_core::symbol_resolver::ResolveContext::new();
    let permissive_view = crate::resolver_core::PermissiveStoreView;
    let result =
        host.resolver_runtime()
            .symbol
            .resolve_node(key, &permissive_view, &mut ctx, |_| {
                let declaration = crate::resolver_core::resolve_type_declaration(
                    &resolver,
                    dep_canonical,
                    requested_name,
                );
                let mut tracked_deps = std::collections::BTreeSet::new();
                if !declaration.canonical_source.is_empty()
                    && declaration.canonical_source != dep_canonical
                {
                    tracked_deps.insert(declaration.canonical_source.clone());
                }

                crate::resolver_core::symbol_resolver::SymbolNodeResult {
                    value: crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(
                        declaration,
                    ),
                    facts: host.current_dependency_fact_versions(dep_canonical, &tracked_deps),
                    diagnostics: Vec::new(),
                }
            });

    match result.value {
        crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(declaration) => {
            declaration
        }
        _ => unreachable!("declaration resolution must return a declaration node result"),
    }
}

fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.read_analysis_source(canonical_source)
        .map(|source| source.to_string())
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_workspace::ResolveRequestKind,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
    let (description, tags) =
        verter_semantic::analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn map_jsdoc_tag(
    host: &VerterHost,
    canonical_source: &str,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    _kind: verter_workspace::ResolveRequestKind,
    tag: verter_semantic::analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ProjectionMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(host, canonical_source, raw_type, tracked_deps)
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_type);
    let parsed = if parsed.is_unknown() {
        verter_semantic::analysis::type_expr::TypeExpr::Unknown {
            raw: raw_type.to_string(),
        }
    } else {
        parsed
    };

    // Ensure module facts are materialized so the dispatch path can
    // resolve imports through host-owned caches.
    let _facts = host.ensure_indexed_ready(canonical_source)?;
    tracked_deps.extend(
        host.imported_symbol_dependencies_for_expr(canonical_source, &parsed)
            .into_iter()
            .map(|dependency| dependency.canonical_id),
    );
    // Phase 5d (sub-plan §4.1): route directly through the shared
    // dispatch ProjectPath helper. Falls back to the raw parsed
    // annotation when projection misses so the caller still receives
    // the unresolved TypeExpr rather than `None`.
    Some(project_expr_class_a_via_dispatch(host, canonical_source, &parsed).unwrap_or(parsed))
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
