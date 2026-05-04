//! Resolved-state types + small TypeExpr substitution helpers.
//!
//! Domain 5 — `ResolvedComponentMetaState`,
//! `SurfaceNodeIdentities`, type aliases, and 9 standalone TypeExpr
//! substitution / scope-selection helpers.

use super::dep_signature::accumulate_dispatch_dep_signature;
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use std::sync::Arc;

// `ResolvedDeclarationKind`, `ResolvedTypeDeclaration`,
// `ResolvedTypeRegistryMeta`, `ResolvedMacroMeta`, `ResolvedNativeProp`,
// `ResolvedJsdocBlock`, `ResolvedJsdocTag`, and
// `ResolvedComponentMetaComputeAudit` live in the request-ctx sibling
// (`super::request_host`); this module imports them via `super::*`
// re-exports through the shell.
use super::{ResolvedComponentMetaComputeAudit, ResolvedMacroMeta, ResolvedTypeRegistryMeta};

/// Vector-aligned sidecar carrying the producing `SemanticNodeId`
/// for each output entry in `ExpandedComponentTypes` /
/// `ResolvedTypeRegistry`.
///
/// Populated when audit is on so `build_origin_graph` can scope the
/// reachable-subgraph walk to the actual surface nodes the request
/// touched, rather than exporting every edge ever recorded by the
/// shared graph store. `None` entries indicate synthetic /
/// inline-annotation results that bypassed dispatch (no
/// `SemanticNodeId` available).
///
/// Index alignment is invariant: `prop_node_ids[i]` corresponds to
/// `evaluated_types.props[i]`, etc. Length-equality checked at
/// construction time inside `compute_component_meta_state_inner`.
///
/// Stored on `ResolvedComponentMetaState.surface_identities` —
/// session-layer only (per crate-layering §1.3 + D19, NOT pushed
/// upstream into `verter_semantic` types).
#[derive(Debug, Clone, Default)]
pub struct SurfaceNodeIdentities {
    /// Index-aligned with `ExpandedComponentTypes.props`.
    pub prop_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.emits`.
    pub emit_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.slot_bindings`.
    pub slot_binding_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.bindings`.
    pub binding_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ResolvedComponentMetaState.resolved_type_registry`.
    pub registry_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ProjectionMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry:
        Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    /// Semantic fact versions consumed while producing this resolved state.
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    /// Non-semantic compute audit captured only when native audit is enabled.
    pub compute_audit: Option<ResolvedComponentMetaComputeAudit>,
    /// Surface-id sidecar. Populated only
    /// when audit is on; the scoped origin export reads `prop_node_ids`
    /// etc. as starting points for the reachable-subgraph walk.
    pub surface_identities: Option<SurfaceNodeIdentities>,
    /// Origin subgraph for semantic results. Populated in `Expanded` mode
    /// by walking the `SemanticGraphStore` after dispatch resolution.
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
    /// Request identifier stamped by the ctx at the entry of
    /// `get_component_meta_with_resolution`. Non-zero. Consumers (the
    /// `AuditedRequest` harness and NAPI/WASM/LSP wrappers) use this
    /// to retrieve the matching `RequestAuditRecord` via
    /// `VerterHost::take_audit_record(resolution.request_id)`.
    ///
    /// Zero is reserved for "not populated" — emitted by internal
    /// tests / FFI fixtures that do not stamp a real request id.
    pub request_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryMaterialization {
    Full,
    SkipAppend,
}

pub(crate) fn collect_expanded_slot_binding_param_types<'a>(
    ty: &'a verter_semantic::analysis::type_expr::TypeExpr,
    out: &mut Vec<&'a verter_semantic::analysis::type_expr::TypeExpr>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_binding_param_types(inner, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_binding_param_types(inner, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Function(func) => {
            if let Some(first) = func.parameters.first() {
                out.push(&first.ty);
            }
        }
        // Path C C11-residual-A: deferred Conditional whose extends has
        // `infer X` in a Function position represents a TS conditional
        // that the dispatch couldn't decide (typically due to an
        // in-flight sentinel during the upstream evaluation context).
        // For slot-binding extraction we use the conventional
        // TS-truthy semantics: walk the true_type as the slot-shape
        // contributor. The infer bindings extracted from the check's
        // matching Function position are folded into the true_type via
        // `decide_typeexpr_conditional_with_function_extends`, which
        // the caller (`enrich_missing_slot_bindings`) invokes before
        // collection.
        verter_semantic::analysis::type_expr::TypeExpr::Conditional { true_type, .. } => {
            collect_expanded_slot_binding_param_types(true_type, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                    | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                        function,
                    ) => {
                        if let Some(first) = function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                        if let Some(first) = method.function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_semantic::analysis::type_expr::ObjectMember::Property(_)
                    | verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(_) => {}
                }
            }
        }
        _ => {}
    }
}

/// Path C C11-residual-B: shallow substitution for owner-local generic
/// alias refs at the registry-publish boundary. When a registry entry's
/// raw body is `Ref { name, [args..] }` and the alias is declared in the
/// SAME canonical scope as the registry consumer, look up the alias's
/// prepared body. If the body is an Object, substitute the type
/// arguments into its members and return the substituted Object. The
/// substituted Object preserves owner-local helper Refs (e.g.,
/// `ComponentVariants<T>` stays as `Ref { name: "ComponentVariants", ..}`)
/// rather than recursively expanding them — the registry consumer can
/// follow the helper Refs through the registry.
///
/// Returns `None` when the raw body is not a Ref, the alias is
/// cross-file, the alias has no prepared body, or the body is not an
/// Object.
pub(crate) fn component_meta_owner_local_shallow_substituted_alias_body(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = raw_body?
    else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }
    let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
    let target_canonical = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    if target_canonical != scope_canonical_id {
        // Cross-file alias — let the imported_generic_alias_root path
        // handle it via materialisation + per-member refinement.
        return None;
    }
    let resolved_name = if declaration.resolved_name.is_empty() {
        name.as_ref().to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let prepared = query_engine.prepared_type_decl(&target_canonical, &resolved_name)?;
    if prepared.type_parameters.len() < type_arguments.len() {
        return None;
    }
    let mut substitutions: rustc_hash::FxHashMap<String, TypeExpr> =
        rustc_hash::FxHashMap::default();
    for (index, param) in prepared.type_parameters.iter().enumerate() {
        let arg = type_arguments
            .get(index)
            .or(param.default.as_deref())
            .cloned();
        if let Some(arg) = arg {
            substitutions.insert(param.name.clone(), arg);
        }
        // Partial substitution still useful when later params have no
        // arg and no default — leave them unsubstituted in the body.
    }
    let body = &prepared.body;
    let TypeExpr::Object(_) = body else {
        return None;
    };
    Some(component_meta_substitute_typeexpr(body, &substitutions))
}

/// Recursive TypeExpr substitution walker. Walks every variant and
/// delegates leaf replacement to `try_replace`: return `Some(expr)` to
/// replace, `None` to recurse structurally.
pub(crate) fn walk_substitute_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    try_replace: &impl Fn(
        &verter_semantic::analysis::type_expr::TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{
        FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
        ObjectProperty, TupleElement, TypeExpr,
    };
    if let Some(replaced) = try_replace(expr) {
        return replaced;
    }
    let recurse = |e: &TypeExpr| -> TypeExpr { walk_substitute_typeexpr(e, try_replace) };
    let recurse_fn = |f: &FunctionExpr| -> FunctionExpr {
        FunctionExpr {
            parameters: f
                .parameters
                .iter()
                .map(|fp| FunctionParam {
                    name: fp.name.clone(),
                    ty: recurse(&fp.ty),
                    optional: fp.optional,
                    rest: fp.rest,
                })
                .collect(),
            return_type: f
                .return_type
                .as_ref()
                .map(|rt| std::sync::Arc::new(recurse(rt))),
            type_parameters: f.type_parameters.clone(),
        }
    };
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: std::sync::Arc::from(
                type_arguments.iter().map(&recurse).collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Parenthesized(inner) => {
            TypeExpr::Parenthesized(std::sync::Arc::new(recurse(inner)))
        }
        TypeExpr::Union(parts) => TypeExpr::Union(std::sync::Arc::from(
            parts.iter().map(&recurse).collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(parts) => TypeExpr::Intersection(std::sync::Arc::from(
            parts.iter().map(&recurse).collect::<Vec<_>>(),
        )),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(recurse(element)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                elements
                    .iter()
                    .map(|element| TupleElement {
                        label: element.label.clone(),
                        ty: recurse(&element.ty),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Object(obj) => TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .map(|member| match member {
                    ObjectMember::Property(p) => ObjectMember::Property(ObjectProperty {
                        name: p.name.clone(),
                        ty: recurse(&p.ty),
                        optional: p.optional,
                        readonly: p.readonly,
                    }),
                    ObjectMember::Method(m) => ObjectMember::Method(MethodSignature {
                        name: m.name.clone(),
                        function: recurse_fn(&m.function),
                        optional: m.optional,
                    }),
                    ObjectMember::CallSignature(f) => ObjectMember::CallSignature(recurse_fn(f)),
                    ObjectMember::ConstructSignature(f) => {
                        ObjectMember::ConstructSignature(recurse_fn(f))
                    }
                    ObjectMember::IndexSignature(sig) => {
                        ObjectMember::IndexSignature(IndexSignature {
                            key_name: sig.key_name.clone(),
                            key_type: recurse(&sig.key_type),
                            value_type: recurse(&sig.value_type),
                            readonly: sig.readonly,
                        })
                    }
                })
                .collect(),
        })),
        TypeExpr::Function(func) => TypeExpr::Function(std::sync::Arc::new(recurse_fn(func))),
        _ => expr.clone(),
    }
}

pub(crate) fn component_meta_substitute_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    substitutions: &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::TypeExpr;
    walk_substitute_typeexpr(expr, &|e| match e {
        TypeExpr::TypeParameter(param) => substitutions.get(&param.name).cloned(),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => substitutions.get(name.as_ref()).cloned(),
        _ => None,
    })
}

/// TypeExpr-level conditional decision (Path C C11-residual-A workaround).
///
/// When the dispatch fails to decide a `T extends (props: infer P) => any
/// ? F<P, ...> : ...` pattern at evaluation time (typically because a
/// same-path sentinel suppressed the cross-file `T[K]` evaluation), the
/// resulting `slot.ty` is left as a deferred `TypeExpr::Conditional`.
/// This helper applies the same nested-Function-Infer reduction that
/// `build_conditional`'s C11a path performs, but at the `TypeExpr`
/// level so slot-binding extraction can proceed without re-running the
/// dispatch.
///
/// Returns:
/// - `Some(decided_true_type_with_infer_substituted)` when the
///   conditional has a concrete Function check, a Function extends with
///   at least one `infer X` position, and the corresponding check
///   parameter types can be bound to those infer names.
/// - `None` when the conditional cannot be decided at this layer (no
///   infer-bearing Function extends, no Function check, or empty
///   bindings).
pub(crate) fn decide_typeexpr_conditional_with_function_extends(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    use verter_semantic::analysis::type_expr::TypeExpr;
    let TypeExpr::Conditional {
        check,
        extends,
        true_type,
        ..
    } = expr
    else {
        return None;
    };
    let TypeExpr::Function(check_fn) = check.as_ref() else {
        return None;
    };
    let TypeExpr::Function(extends_fn) = extends.as_ref() else {
        return None;
    };
    let mut bindings: rustc_hash::FxHashMap<String, TypeExpr> = rustc_hash::FxHashMap::default();
    for (e_param, c_param) in extends_fn.parameters.iter().zip(check_fn.parameters.iter()) {
        if let TypeExpr::Infer { name } = &e_param.ty {
            bindings.insert(name.clone(), c_param.ty.clone());
        }
    }
    if let (Some(TypeExpr::Infer { name }), Some(check_ret)) = (
        extends_fn.return_type.as_deref(),
        check_fn.return_type.as_deref(),
    ) {
        bindings.insert(name.clone(), check_ret.clone());
    }
    if bindings.is_empty() {
        return None;
    }
    Some(substitute_infer_in_typeexpr(true_type, &bindings))
}

pub(crate) fn substitute_infer_in_typeexpr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    bindings: &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::TypeExpr;
    walk_substitute_typeexpr(expr, &|e| match e {
        TypeExpr::Infer { name } => bindings.get(name).cloned(),
        // Replace `semanticMiss` sentinel with the unique bound infer
        // when there is exactly one — recovers an inferred prop whose
        // SemanticNode-level position was lost during dispatch.
        TypeExpr::Unknown { raw }
            if raw == crate::resolver_core::component_meta_query_engine::SEMANTIC_MISS
                && bindings.len() == 1 =>
        {
            bindings.values().next().cloned()
        }
        _ => None,
    })
}

pub(crate) fn collect_expanded_slot_bindings_from_object_type(
    ty: &verter_semantic::analysis::type_expr::TypeExpr,
    seen: &mut rustc_hash::FxHashSet<String>,
    out: &mut Vec<(String, verter_semantic::analysis::type_expr::TypeExpr, bool)>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_bindings_from_object_type(inner, seen, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_bindings_from_object_type(inner, seen, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Object(obj) => {
            for member in &obj.properties {
                let verter_semantic::analysis::type_expr::ObjectMember::Property(prop) = member
                else {
                    continue;
                };
                if !seen.insert(prop.name.clone()) {
                    continue;
                }
                out.push((prop.name.clone(), prop.ty.clone(), prop.optional));
            }
        }
        _ => {}
    }
}

pub(crate) fn enrich_missing_slot_bindings(
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) {
    let mut seen_names: rustc_hash::FxHashSet<String> = evaluated_types
        .slot_bindings
        .iter()
        .map(|binding| binding.name.clone())
        .collect();

    for entry in &evaluated_types.define_slots {
        for slot in &entry.result.value.properties {
            // Path C C11-residual-A: normalize a deferred Conditional
            // slot.ty by performing TS truthy-branch reduction at the
            // TypeExpr level before extracting binding params. The
            // dispatch may have left a deferred Conditional in the
            // slot value when an upstream sentinel suppressed
            // evaluation; recovering the true_type here lets the
            // caller's `collect_expanded_slot_bindings_from_object_type`
            // reach into the Function param and surface the binding
            // names.
            let normalized_ty;
            let slot_ty_for_collect = if let Some(decided) =
                decide_typeexpr_conditional_with_function_extends(&slot.ty)
            {
                normalized_ty = decided;
                &normalized_ty
            } else {
                &slot.ty
            };
            let mut binding_param_types = Vec::new();
            collect_expanded_slot_binding_param_types(
                slot_ty_for_collect,
                &mut binding_param_types,
            );
            if binding_param_types.is_empty() {
                continue;
            }

            let mut seen_bindings = rustc_hash::FxHashSet::default();
            let mut bindings = Vec::new();
            for binding_param_ty in binding_param_types {
                collect_expanded_slot_bindings_from_object_type(
                    binding_param_ty,
                    &mut seen_bindings,
                    &mut bindings,
                );
            }

            for (binding_name, binding_type, optional) in bindings {
                let field_name = format!("{}.{}", slot.name, binding_name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                evaluated_types.slot_bindings.push(
                    verter_semantic::analysis::type_expand::ExpandedField {
                        name: field_name,
                        r#type: binding_type,
                        raw_type: None,
                        optional,
                        exactness: entry.result.exactness,
                        execution_status: entry.result.execution_status,
                        diagnostics: Vec::new(),
                    },
                );
            }
        }
    }

    for resolved in resolved_macros.iter().filter(|resolved| {
        resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
    }) {
        for slot in &resolved.slots {
            for binding in &slot.bindings {
                let field_name = format!("{}.{}", slot.name, binding.name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                let raw_type = binding.type_annotation.clone();
                let parsed_type = raw_type
                    .as_deref()
                    .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                    .unwrap_or_else(|| verter_semantic::analysis::type_expr::TypeExpr::Unknown {
                        raw: "unknown".to_string(),
                    });
                evaluated_types
                    .slot_bindings
                    .push(verter_semantic::analysis::type_expand::ExpandedField {
                    name: field_name,
                    r#type: parsed_type,
                    raw_type,
                    optional: false,
                    exactness:
                        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                    execution_status:
                        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                    diagnostics: Vec::new(),
                });
            }
        }
    }
}

pub(crate) fn select_imported_materialization_scope(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    owner_canonical: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<String> {
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };
    let route_root_name = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .map(|(root_name, _)| root_name);
    let root_name = match expr {
        verter_semantic::analysis::type_expr::TypeExpr::Ref { name, .. } => name.as_ref(),
        _ => route_root_name.as_deref()?,
    };

    let declaration = query_engine.resolve_type_declaration(owner_canonical, root_name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        owner_canonical.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (final_scope, _) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());

    (!final_scope.is_empty() && final_scope != owner_canonical).then_some(final_scope)
}

/// Migration helper. Lowers `expr` via Navigate to a
/// `SemanticNodeId`, extracts the root identity (DeclRef or
/// InstantiationRef base), and delegates to the canonical graph-native
/// [`crate::meta_resolve::ref_root_reaches_transitive_cycle_node`]
/// predicate. The cycle-BFS dep-signature facts are accumulated into
/// the per-request thread-local dispatch accumulator so completion
/// fences stay complete.
///
/// Returns `false` when (a) lowering fails or (b) the lowered node is
/// neither a `DeclRef` nor an `InstantiationRef` — neither shape carries
/// a route root identity and the legacy adapter behaved the same way.
pub(crate) fn lowered_root_reaches_transitive_cycle(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData};
    let dispatch = ProjectSemanticDispatch::new(query_engine.ctx);
    let Some(node_id) = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Navigate,
    ) else {
        return false;
    };
    let identity = match query_engine
        .ctx
        .project_type_store()
        .semantic_graph()
        .node_data(node_id)
        .as_deref()
    {
        Some(SemanticNodeData::DeclRef { identity }) => identity.clone(),
        Some(SemanticNodeData::InstantiationRef { base, .. }) => base.clone(),
        _ => return false,
    };
    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let result =
        super::ref_root_reaches_transitive_cycle_node(&identity, query_engine.ctx, &mut fence);
    accumulate_dispatch_dep_signature(&Arc::from(fence.into_boxed_slice()));
    result
}
