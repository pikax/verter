//! Registry structural materialization + member-route preservers.
//!
//! Domain 11 — owns the graph-native structural materialiser
//! (`component_meta_registry_prefers_structural_materialization_node` +
//! `materialize_component_meta_registry_structural_expr`) plus the seven
//! registry-route preserver predicates that gate it
//! (`preserve_package_backed_symbolic_refs_node`,
//! `preserve_registry_callable_param_member_routes`,
//! `nested_symbolic_member_route_should_stay_symbolic`,
//! `type_expr_contains_public_member_route`,
//! `type_expr_needs_nested_symbolic_route_preservation`,
//! `preserve_nested_symbolic_member_routes`,
//! `component_meta_registry_should_keep_raw_symbolic_non_object_alias`).
//!
//! Lines 167-1289 of the post-commit-10 `meta_resolve.rs` shell.
//! Visibility escalation: the formerly-private free fns are escalated
//! to `pub(crate)` so the host_methods.rs / macro_member_walk.rs siblings
//! and `meta_resolve_tests.rs` can keep calling them via the shell's
//! `pub(crate) use registry_materialize::*;` re-export.

use crate::resolver_core::ResolverContext;
use std::sync::Arc;

use super::dispatch_helpers::{
    project_expr_class_a_via_dispatch_threaded, project_type_surface_expr_via_host_threaded,
};

use super::graph_predicates::component_meta_ref_resolves_to_package_node;

use crate::resolver_core::component_meta_registry::{
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};

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
        | SemanticNodeData::KeyOf { .. }
        // Structural fidelity carrier (constructor signature) materialises
        // structurally like the function shape it wraps.
        | SemanticNodeData::ConstructorType { .. } => true,
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
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. }
        // A merged declaration is a concrete peer-merged object surface — like
        // `Object`, it does not prefer structural materialization.
        | SemanticNodeData::MergedDecl { .. }
        | SemanticNodeData::VueMacroElements(_)
        // Unresolved bare-name / dynamic-import / raw-fallback /
        // synthetic-binding carriers are references / terminals, not
        // structural shapes to materialise.
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::RawFallback { .. }
        | SemanticNodeData::SyntheticBinding { .. } => false,
    }
}

pub(crate) fn materialize_component_meta_registry_structural_expr(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> (verter_type_expr::TypeExpr, bool) {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};

    /// Graph-native package check on a lowered
    /// `Ref { name, [] }`. Lowers via Navigate to a DeclRef /
    /// InstantiationRef, extracts the canonical identity, and
    /// delegates to the J0 / commit-C primitive
    /// `component_meta_ref_resolves_to_package_node`. Falls back to
    /// `false` (not package-backed) when lowering fails or produces a
    /// non-Ref node — the closure's structural recursion path then
    /// projects through `project_type_surface_expr` like any other
    /// local Ref.
    fn ref_is_package_backed_node(
        ctx: &dyn ResolverContext,
        scope_canonical_id: &str,
        name: &str,
    ) -> bool {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let probe = verter_type_expr::TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        };
        let Some(node_id) = dispatch.lower_type_expr_in_scope_with_context(
            scope_canonical_id,
            &probe,
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                ProjectionMode::Navigate,
            ),
        ) else {
            return false;
        };
        let graph = ctx.project_type_store().semantic_graph();
        let Some(data) = graph.node_data(node_id) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                component_meta_ref_resolves_to_package_node(ctx, identity)
            }
            SemanticNodeData::InstantiationRef { base, .. } => {
                component_meta_ref_resolves_to_package_node(ctx, base)
            }
            _ => false,
        }
    }

    fn inner(
        expr: &verter_type_expr::TypeExpr,
        scope_canonical_id: &str,
        engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
        active: &mut rustc_hash::FxHashSet<SemanticNodeId>,
        publish_operators: bool,
    ) -> (verter_type_expr::TypeExpr, bool) {
        use verter_type_expr::{ObjectMember, TypeExpr};

        // Graph-native cycle guard. Lower the
        // current expr to a Navigate-mode SemanticNodeId and use
        // structural identity (interned node id) for cycle tracking
        // instead of TypeExpr-equality hashing. When lowering fails
        // (None), we cannot intern a key — proceed without cycle
        // tracking for this visit (TypeExpr-equality cycle tracking
        // would not have terminated either; the structural recursion
        // remains safe under the existing structural bounds).
        let dispatch_for_cycle = ProjectSemanticDispatch::new(engine.ctx);
        let cycle_key = dispatch_for_cycle.lower_type_expr_in_scope_with_context(
            scope_canonical_id,
            expr,
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                ProjectionMode::Navigate,
            ),
        );
        if let Some(key) = cycle_key {
            if !active.insert(key) {
                return (
                    expr.clone(),
                    component_meta_registry_has_explicit_object_surface(expr),
                );
            }
        }

        let result = if let Some((root_symbol, route)) = publish_operators
            .then(|| {
                component_meta_registry_public_utility_route(expr)
                    .or_else(|| component_meta_registry_public_indexed_access_route(expr))
            })
            .flatten()
        {
            // Migrate route-target callers to dispatch
            // (sub- D-T recipe). Route the utility/indexed
            // expression through the Class A dispatch helper, which handles
            // Whole/MemberPath via its registry-route fast-path AND falls
            // back to the generic ProjectPath{[],Expanded} dispatch. Pick/Omit
            // route-targets reach the dispatch's `Instantiate` builtin
            // utility path internally via lower_type_expr_in_scope_with_mode.
            //
            // The two-step engine resolution (try direct scope, then
            // declaration scope) is preserved so re-exported / barrel-routed
            // declarations resolve correctly.
            let _ = &route; // route demand carrier preserved for parity reads
            project_expr_class_a_via_dispatch_threaded(
                engine.ctx,
                Some(engine),
                scope_canonical_id,
                expr,
            )
            .or_else(|| {
                let declaration = engine.resolve_type_declaration(scope_canonical_id, &root_symbol);
                (!declaration.canonical_source.is_empty())
                    .then(|| {
                        project_expr_class_a_via_dispatch_threaded(
                            engine.ctx,
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
                    // Graph-native package check.
                    if ref_is_package_backed_node(engine.ctx, scope_canonical_id, name) {
                        expr.clone()
                    } else {
                        // Bridge via per-engine
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
                            .map(|arg| {
                                inner(
                                    arg,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0
                            })
                            .collect::<Vec<_>>(),
                    ),
                },
                TypeExpr::Parenthesized(inner_expr) => TypeExpr::Parenthesized(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                    publish_operators,
                ).0)),
                TypeExpr::Array { element, readonly } => TypeExpr::Array {
                    element: Arc::new(inner(
                        element,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    ).0),
                    readonly: *readonly,
                },
                TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
                    elements: Arc::from(
                        elements
                            .iter()
                            .map(|element| verter_type_expr::TupleElement {
                                label: element.label.clone(),
                                ty: inner(
                                    &element.ty,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0,
                                optional: element.optional,
                                rest: element.rest,
                            })
                            .collect::<Vec<_>>(),
                    ),
                    readonly: *readonly,
                },
                TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
                    types
                        .iter()
                        .map(|ty| {
                            inner(ty, scope_canonical_id, engine, active, publish_operators).0
                        })
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
                    types
                        .iter()
                        .map(|ty| {
                            inner(ty, scope_canonical_id, engine, active, publish_operators).0
                        })
                        .collect::<Vec<_>>(),
                )),
                TypeExpr::Conditional {
                    check,
                    extends,
                    true_type,
                    false_type,
                } => TypeExpr::Conditional {
                    check: Arc::new(inner(check, scope_canonical_id, engine, active, false).0),
                    extends: Arc::new(inner(extends, scope_canonical_id, engine, active, false).0),
                    true_type: Arc::new(inner(
                        true_type,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    ).0),
                    false_type: Arc::new(inner(
                        false_type,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    ).0),
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
                    source: Arc::new(inner(
                        source,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    ).0),
                    optional: *optional,
                    readonly: *readonly,
                    name_type: name_type.as_deref().map(|name_type| {
                        Arc::new(inner(
                            name_type,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        ).0)
                    }),
                    value: Arc::new(inner(
                        value,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    ).0),
                },
                TypeExpr::TemplateLiteral {
                    quasis,
                    expressions,
                } => TypeExpr::TemplateLiteral {
                    quasis: quasis.clone(),
                    expressions: Arc::from(
                        expressions
                            .iter()
                            .map(|expr| {
                                inner(
                                    expr,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0
                            })
                            .collect::<Vec<_>>(),
                    ),
                },
                // Both a function type and a constructor type carry the same
                // `FunctionExpr` payload and rewrite their signature identically;
                // only the reconstructed variant differs, so the constructor-ness
                // is preserved (a `ConstructorType` is never flattened to a plain
                // `Function`). `is_constructor` selects the wrap at the end.
                TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                    let is_constructor = matches!(expr, TypeExpr::ConstructorType(_));
                    let mut function = function.as_ref().clone();
                    for parameter in &mut function.parameters {
                        parameter.ty = inner(
                            &parameter.ty,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        ).0;
                    }
                    if let Some(return_type) = function.return_type.as_mut() {
                        *return_type = Arc::new(inner(
                            return_type,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        ).0);
                    }
                    for type_parameter in &mut function.type_parameters {
                        if let Some(constraint) = type_parameter.constraint.as_mut() {
                            *constraint = Arc::new(inner(
                                constraint,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            ).0);
                        }
                        if let Some(default) = type_parameter.default.as_mut() {
                            *default = Arc::new(inner(
                                default,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            ).0);
                        }
                    }
                    let function = Arc::new(function);
                    if is_constructor {
                        TypeExpr::ConstructorType(function)
                    } else {
                        TypeExpr::Function(function)
                    }
                }
                TypeExpr::KeyOf(inner_expr) => {
                    if publish_operators {
                        if let Some(projected) = project_expr_class_a_via_dispatch_threaded(
                            engine.ctx,
                            Some(engine),
                            scope_canonical_id,
                            expr,
                        ) {
                            projected
                        } else {
                            TypeExpr::KeyOf(Arc::new(inner(
                                inner_expr,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            ).0))
                        }
                    } else {
                        TypeExpr::KeyOf(Arc::new(inner(
                            inner_expr,
                            scope_canonical_id,
                            engine,
                            active,
                            false,
                        ).0))
                    }
                }
                TypeExpr::Rest(inner_expr) => TypeExpr::Rest(Arc::new(inner(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                    publish_operators,
                ).0)),
                TypeExpr::Object(object) => {
                    let mut object = object.as_ref().clone();
                    for member in &mut object.properties {
                        match member {
                            ObjectMember::Property(property) => {
                                property.ty = inner(
                                    &property.ty,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0;
                            }
                            ObjectMember::IndexSignature(signature) => {
                                signature.key_type = inner(
                                    &signature.key_type,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0;
                                signature.value_type = inner(
                                    &signature.value_type,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                ).0;
                            }
                            ObjectMember::CallSignature(function)
                            | ObjectMember::ConstructSignature(function) => {
                                for parameter in &mut function.parameters {
                                    parameter.ty = inner(
                                        &parameter.ty,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    ).0;
                                }
                                if let Some(return_type) = function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    ).0);
                                }
                            }
                            ObjectMember::Method(method) => {
                                for parameter in &mut method.function.parameters {
                                    parameter.ty = inner(
                                        &parameter.ty,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    ).0;
                                }
                                if let Some(return_type) = method.function.return_type.as_mut() {
                                    *return_type = Arc::new(inner(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    ).0);
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
                // Synthetic carriers are intrinsic terminal leaves —
                // the structural materialiser passes them through
                // unchanged.
                | TypeExpr::SyntheticSlotBinding(_)
                // An import-type is a cross-file resolution carrier, passed
                // through unchanged here like the other operator terminals
                // (`IndexedAccess` / `TypeOf`) — its shallow carrier is
                // preserved through structural materialisation.
                | TypeExpr::ImportType { .. }
                | TypeExpr::Infer { .. } => expr.clone(),
            }
        };

        if let Some(key) = cycle_key {
            active.remove(&key);
        }
        // Interim TypeExpr-derived object-surface fact: this structural path still
        // materialises a `TypeExpr` (the fence flags it at its own site), so the fact
        // is `component_meta_registry_has_explicit_object_surface(&result)` on the FINAL
        // materialised value — NOT a node-domain decision. It is threaded out to the
        // host so the caller reads it as a precomputed fact rather than re-deriving it;
        // a later block converts this path node-native and the fact with it.
        let explicit_object_surface = component_meta_registry_has_explicit_object_surface(&result);
        (result, explicit_object_surface)
    }

    let mut active: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    inner(expr, scope_canonical_id, engine, &mut active, true)
}

/// Graph-native predicate. Walks two parallel `SemanticNodeId`
/// trees (materialised + raw) and, when the raw surface exposes a
/// package-backed `DeclRef` / `InstantiationRef` at a given member,
/// overrides the materialised member's value with the raw graph
/// node so the symbolic Ref is preserved through materialisation.
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
    ctx: &dyn ResolverContext,
    materialized: crate::semantic_query::SemanticNodeId,
    raw: crate::semantic_query::SemanticNodeId,
    depth: u32,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{SemanticNodeData, SurfaceMember, SurfaceView};
    use rustc_hash::FxHashMap;

    if depth > 256 {
        return materialized;
    }
    let graph = ctx.project_type_store().semantic_graph();
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
                            component_meta_ref_resolves_to_package_node(ctx, identity)
                        }
                        SemanticNodeData::InstantiationRef { base, .. } => {
                            component_meta_ref_resolves_to_package_node(ctx, base)
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
                        // Value-node rewrite preserves the member's declared
                        // accessibility verbatim.
                        visibility: materialised_member.visibility,
                        spans: materialised_member.spans,
                        declaration_origin: materialised_member.declaration_origin.clone(),
                        declared_in_macro_type_arg: materialised_member.declared_in_macro_type_arg,
                        merge_role: materialised_member.merge_role,
                    });
                    continue;
                }
                // Recurse into the parallel pair.
                let recursed = preserve_package_backed_symbolic_refs_node(
                    ctx,
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
                    // Value-node rewrite preserves the member's declared
                    // accessibility verbatim.
                    visibility: materialised_member.visibility,
                    spans: materialised_member.spans,
                    declaration_origin: materialised_member.declaration_origin.clone(),
                    declared_in_macro_type_arg: materialised_member.declared_in_macro_type_arg,
                    merge_role: materialised_member.merge_role,
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

/// Borrow the inner [`FunctionExpr`](verter_type_expr::FunctionExpr) of a
/// function-like `TypeExpr` — a [`TypeExpr::Function`] OR a
/// [`TypeExpr::ConstructorType`] — together with the constructor-ness flag.
///
/// Both variants carry the identical `FunctionExpr` payload and walk their
/// signature identically; only the reconstructed variant differs. The
/// structure-preserving route preservers use this so a function-like pair is
/// walked regardless of whether either side is a plain function or a bare
/// constructor type — and so a `ConstructorType` is reconstructed as a
/// `ConstructorType` (never flattened to a `Function`).
fn function_like_inner(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(&std::sync::Arc<verter_type_expr::FunctionExpr>, bool)> {
    use verter_type_expr::TypeExpr;
    match expr {
        TypeExpr::Function(function) => Some((function, false)),
        TypeExpr::ConstructorType(function) => Some((function, true)),
        _ => None,
    }
}

/// Re-wrap a rewritten [`FunctionExpr`](verter_type_expr::FunctionExpr) as
/// either [`TypeExpr::ConstructorType`] (when `is_constructor`) or
/// [`TypeExpr::Function`]. Mirrors the wrap decision in
/// `materialize_component_meta_registry_structural_expr`.
fn wrap_function_like(
    function: verter_type_expr::FunctionExpr,
    is_constructor: bool,
) -> verter_type_expr::TypeExpr {
    use verter_type_expr::TypeExpr;
    let function = std::sync::Arc::new(function);
    if is_constructor {
        TypeExpr::ConstructorType(function)
    } else {
        TypeExpr::Function(function)
    }
}

pub(crate) fn preserve_registry_callable_param_member_routes(
    materialized: &verter_type_expr::TypeExpr,
    raw: &verter_type_expr::TypeExpr,
) -> verter_type_expr::TypeExpr {
    use rustc_hash::FxHashMap;
    use verter_type_expr::{ObjectMember, TypeExpr};

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
                            // A property whose value is a function-like type
                            // (plain `Function` OR a bare `ConstructorType`)
                            // seeds the callable map. Capture the
                            // constructor-ness so a Property→Method remap
                            // reconstructs the correct variant rather than
                            // flattening a constructor type to a function.
                            if let Some((function, is_constructor)) =
                                function_like_inner(&property.ty)
                            {
                                raw_callables.insert(
                                    property.name.as_str(),
                                    (function.as_ref().clone(), is_constructor),
                                );
                            }
                        }
                        ObjectMember::Method(method) => {
                            raw_methods.insert(method.name.as_str(), method);
                            // A method signature is function-like (never a
                            // constructor type).
                            raw_callables
                                .insert(method.name.as_str(), (method.function.clone(), false));
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
                            } else if let Some((materialized_function, materialized_is_ctor)) =
                                function_like_inner(&property.ty)
                            {
                                if let Some((raw_callable, raw_is_ctor)) =
                                    raw_callables.get(property.name.as_str())
                                {
                                    property.ty = inner(
                                        &wrap_function_like(
                                            materialized_function.as_ref().clone(),
                                            materialized_is_ctor,
                                        ),
                                        &wrap_function_like(raw_callable.clone(), *raw_is_ctor),
                                        preserve_routes,
                                    );
                                }
                            }
                        }
                        ObjectMember::Method(method) => {
                            if let Some(raw_method) = raw_methods.get(method.name.as_str()) {
                                method.function = match &inner(
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
                            } else if let Some((raw_callable, raw_is_ctor)) =
                                raw_callables.get(method.name.as_str())
                            {
                                // A method's value is always function-like; the
                                // raw callable may be a constructor-valued
                                // property. Walk both function-like and take the
                                // rewritten `FunctionExpr` payload back (the
                                // method stays a method — its container, not the
                                // signature, carries the constructor-ness).
                                method.function = match &inner(
                                    &TypeExpr::Function(std::sync::Arc::new(
                                        method.function.clone(),
                                    )),
                                    &wrap_function_like(raw_callable.clone(), *raw_is_ctor),
                                    preserve_routes,
                                ) {
                                    TypeExpr::Function(function)
                                    | TypeExpr::ConstructorType(function) => {
                                        function.as_ref().clone()
                                    }
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
            // Function-like preservation covers a plain function OR a bare
            // constructor type on EITHER side (both carry the same
            // `FunctionExpr` payload). The constructor-ness of the RAW side is
            // authoritative — the materialised side is the post-dispatch form,
            // where a constructor type was raised function-like — so the
            // rewritten signature is reconstructed as a `ConstructorType`
            // whenever raw was one, never flattened to a plain `Function`.
            _ if function_like_inner(materialized).is_some()
                && function_like_inner(raw).is_some() =>
            {
                let (materialized_function, _) =
                    function_like_inner(materialized).expect("guarded above");
                let (raw_function, raw_is_constructor) =
                    function_like_inner(raw).expect("guarded above");
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
                wrap_function_like(function, raw_is_constructor)
            }
            _ => materialized.clone(),
        }
    }

    inner(materialized, raw, false)
}

pub(crate) fn nested_symbolic_member_route_should_stay_symbolic(
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_type_expr::TypeExpr;

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

pub(crate) fn type_expr_contains_public_member_route(expr: &verter_type_expr::TypeExpr) -> bool {
    use verter_type_expr::{ObjectMember, TypeExpr};

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
        // A constructor type's signature is searched for public member routes
        // identically to a function type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
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
        // Mirrors the `Ref` arm: recurse the nested `type_arguments` (which
        // may carry member routes); the specifier/qualifier are leaf strings.
        TypeExpr::ImportType { type_arguments, .. } => type_arguments
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
        // Synthetic carriers carry no public member route — they are
        // intrinsic terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => false,
    }
}

pub(crate) fn type_expr_needs_nested_symbolic_route_preservation(
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    use verter_type_expr::{ObjectMember, TypeExpr};

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
        // A constructor type's signature is searched identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function
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
        // Mirrors the `Ref` arm: recurse the nested `type_arguments`; the
        // specifier/qualifier are leaf strings.
        TypeExpr::ImportType { type_arguments, .. } => type_arguments
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
        // Synthetic carriers carry no public member route — they are
        // intrinsic terminal leaves.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => false,
    }
}

pub(crate) fn preserve_nested_symbolic_member_routes(
    materialized: &verter_type_expr::TypeExpr,
    raw: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested: bool,
) -> verter_type_expr::TypeExpr {
    use std::sync::Arc;
    use verter_type_expr::{ObjectMember, TypeExpr};

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
        // Function-like preservation covers a plain function OR a bare
        // constructor type on EITHER side. The constructor-ness of the RAW side
        // is authoritative (the materialised side is the post-dispatch
        // function-like form), so the rewritten signature is reconstructed as a
        // `ConstructorType` whenever raw was one — never flattened to a plain
        // `Function`. Without this the scanner at
        // `type_expr_needs_nested_symbolic_route_preservation` flags a
        // constructor type as needing preservation, but a `(Function,
        // ConstructorType)` pair fell to the `_ => materialized.clone()` arm and
        // SILENTLY DROPPED the nested member routes.
        _ if function_like_inner(materialized).is_some() && function_like_inner(raw).is_some() => {
            let (materialized_function, _) =
                function_like_inner(materialized).expect("guarded above");
            let (raw_function, raw_is_constructor) =
                function_like_inner(raw).expect("guarded above");
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
            wrap_function_like(function, raw_is_constructor)
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
    expr: &verter_type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    use verter_type_expr::TypeExpr;

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
        let Some(resolved) = engine.resolve_imported_registry_symbol(scope_canonical_id, name)
        else {
            return true;
        };
        engine
            .ctx
            .workspace_is_package_backed(&resolved.canonical_id)
    }

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        // Synthetic carriers are intrinsic closed terminals — the
        // registry keeps them verbatim (they are never resolved as a
        // type alias).
        | TypeExpr::SyntheticSlotBinding(_)
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
        // Mirrors the `Ref` arm: an import-type is a cross-file symbolic
        // reference the registry keeps verbatim — like a `Ref` that stays
        // symbolic — provided its nested `type_arguments` also stay symbolic.
        // The specifier/qualifier are leaf strings.
        TypeExpr::ImportType { type_arguments, .. } => type_arguments.iter().all(|arg| {
            component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                arg,
                scope_canonical_id,
                engine,
            )
        }),
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
        // A constructor type's signature is evaluated identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
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

// The TypeExpr-keyed free package-ref check was
// The 5 callers migrated to a temporary engine
// method adapter, which was itself
// after migrated production callers to graph-native
// predicates. The graph-native primitive
// `component_meta_ref_resolves_to_package_node` is the canonical
// authority for package-backed decl identity.

// The inline-registry-route candidate family was
// The inline-registry-route candidate path is
// handled by B1's materialiser registry-route branch, which
// dispatches Pick/Omit + IndexedAccess shapes through dispatch's
// canonical projection. Retired symbols are listed in the
// `RETIRED_SYMBOLS` array of the static-grep gate test.

// ===========================================================================
