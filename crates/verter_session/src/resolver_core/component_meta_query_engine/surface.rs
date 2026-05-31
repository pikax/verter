//! Surface-projection helpers, prepared-substitution machinery, and
//! arc cache-key constructors used by `ComponentMetaQueryEngine`.
//!
//! Free functions (not engine methods) that operate on
//! `TypeExpr` / `ProjectedSurface` values produced by the engine and
//! dispatch layers; no engine-state dependencies beyond a borrowed
//! `VerterHost` reference.
//!
//! Cross-callers reach the public-API symbols here via the parent
//! module's `pub(crate) use surface::{...};` re-export at the bottom of
//! `component_meta_query_engine/mod.rs`. Internal helpers stay
//! parent-private (no visibility relaxation).

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
use verter_type_expr::TypeExpr;

use super::{SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE, SEMANTIC_SURFACE_MEMBER};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};

pub(crate) fn projected_surface_from_semantic_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<ProjectedSurface> {
    let mut active = FxHashSet::default();
    projected_surface_from_semantic_node_inner(ctx, node, &mut active)
}

fn projected_surface_from_semantic_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<ProjectedSurface> {
    let data = ctx.dispatch_node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return None;
            }
            let result = projected_surface_from_semantic_node_inner(ctx, *target, active);
            active.remove(&node);
            result
        }
        SemanticNodeData::Object(surface) => Some(surface_view_to_projected_surface(ctx, surface)),
        // Compound roots (`A | B`, `A & B` / heritage overlay, `Foo<Bar>`)
        // carry no single `Object` surface on the post-`Published(Expanded)`
        // instantiated node, and that node can collapse a generic heritage /
        // `Omit` carrier arm to `Opaque(Miss)`. So this projector returns
        // `None` here; the seam (`dispatch_projected_surface`) composes the
        // compound root via `projected_compound_root_surface_via_dispatch`
        // driven from the decl anchor (carrier intact).
        _ => None,
    }
}

/// Compose the shallow surface of a compound root node (`Union` /
/// `Intersection` / `InstantiationRef`) through the shared empty-path
/// Shallow surface walker: drives `ProjectPath { base: node, path: [],
/// macro_object_surface(Shallow, Structural) }` via
/// `resolve_typeinfo_surface_view`, then reconstructs the terminal
/// `SurfaceView` into a `ProjectedSurface`.
///
/// `node` is the decl-anchor base the seam supplies — NOT the
/// post-`Published(Expanded)` instantiated root, which can collapse a
/// generic heritage / `Omit` carrier arm to `Opaque(Miss)` (the shared
/// walker cannot re-resolve an already-collapsed node, whereas the decl
/// anchor still carries the carrier intact). Returns `None` when the walker
/// resolves no `Object` terminal OR the composed surface is empty (an empty
/// surface is never a COMPLETE compound-root projection).
pub(super) fn projected_compound_root_surface_via_dispatch(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<ProjectedSurface> {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let surface = ctx.dispatch().resolve_typeinfo_surface_view(
        node,
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    )?;
    let projected = surface_view_to_projected_surface(ctx, &surface);
    if projected_surface_is_empty(&projected) {
        return None;
    }
    Some(projected)
}

pub(crate) fn surface_view_to_projected_surface(
    ctx: &dyn ResolverContext,
    surface: &SurfaceView,
) -> ProjectedSurface {
    let dispatch = ctx.dispatch();
    let members = surface
        .members
        .iter()
        .map(|member| ProjectedMember {
            name: member.name.as_ref().to_string(),
            ty: dispatch
                .raise_node_to_type_expr(member.value)
                .unwrap_or(TypeExpr::Unknown {
                    raw: SEMANTIC_SURFACE_MEMBER.to_string(),
                }),
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
            // Graph `SurfaceMember` carries the real OXC declaration-site spans
            // (stamped during shallow lowering) AND the member's declaration
            // file; carry both verbatim so the reconstruction re-emits the spans
            // paired with the correct file (a cross-file surface's members keep
            // their own declaring file, not the projection scope).
            spans: member.spans,
            declaration_origin: member.declaration_origin.clone(),
        })
        .collect();
    let call_signatures = surface
        .call_signatures
        .iter()
        .filter_map(|signature| dispatch.raise_node_to_type_expr(*signature))
        .collect();
    let construct_signatures = surface
        .construct_signatures
        .iter()
        .filter_map(|signature| dispatch.raise_node_to_type_expr(*signature))
        .collect();
    // Graph `SurfaceView::index_signatures` carries the declared key/value
    // nodes + real OXC spans + the declaration file. Raise the key/value nodes
    // to `TypeExpr` and carry the spans/origin verbatim so the reconstruction
    // re-emits a real `[k: K]: V` rather than the synthetic open placeholder.
    let index_signatures = surface
        .index_signatures
        .iter()
        .map(|signature| {
            use verter_semantic::analysis::type_solver::query_engine::ProjectedIndexSignature;
            ProjectedIndexSignature {
                key_name: "key".to_string(),
                key_type: dispatch
                    .raise_node_to_type_expr(signature.key_type)
                    .unwrap_or(TypeExpr::Unknown {
                        raw: SEMANTIC_SURFACE_MEMBER.to_string(),
                    }),
                value_type: dispatch
                    .raise_node_to_type_expr(signature.value_type)
                    .unwrap_or(TypeExpr::Unknown {
                        raw: SEMANTIC_SURFACE_MEMBER.to_string(),
                    }),
                readonly: signature.readonly,
                spans: signature.spans,
                declaration_origin: signature.declaration_origin.clone(),
            }
        })
        .collect();
    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        index_signatures,
        has_index_signature: surface.has_index_signature,
    }
}

pub(super) fn filtered_projected_surface(
    mut surface: ProjectedSurface,
    keep: impl Fn(&str) -> bool,
) -> ProjectedSurface {
    surface.members.retain(|member| keep(member.name.as_str()));
    surface
}

pub(super) fn dispatch_route_expr_is_materialized(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { raw } => {
            // Every sentinel emitted by `raise_node_to_type_expr_inner`
            // (exact matches) or by `semantic_query_error_raw` (prefix
            // matches for parameterised errors) must round-trip to
            // "not materialised" so the dispatch-first path falls back
            // to `owner_engine` for fuller expansion.
            let is_exact_sentinel = matches!(
                raw.as_str(),
                SEMANTIC_MISS
                    | SEMANTIC_OBJECT_SURFACE
                    | SEMANTIC_SURFACE_MEMBER
                    | "semanticAliasCycle"
                    | "semanticFunction"
                    | "VueMacroElements"
                    | "projectedOpenSurface"
            );
            let is_prefix_sentinel = raw.starts_with("materialize:")
                || raw.starts_with("unsupportedIntrinsic(")
                || raw.starts_with("budgetExceeded(")
                || raw.starts_with("unstableState(")
                || raw.starts_with("aliasCycle(");
            !is_exact_sentinel && !is_prefix_sentinel
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => dispatch_route_expr_is_materialized(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| dispatch_route_expr_is_materialized(&element.ty)),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => {
                dispatch_route_expr_is_materialized(&property.ty)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && method
                        .function
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::CallSignature(signature)
            | verter_type_expr::ObjectMember::ConstructSignature(signature) => {
                signature
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && signature
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::IndexSignature(signature) => {
                dispatch_route_expr_is_materialized(&signature.key_type)
                    && dispatch_route_expr_is_materialized(&signature.value_type)
            }
        }),
        TypeExpr::Function(function) => {
            function
                .return_type
                .as_deref()
                .is_none_or(dispatch_route_expr_is_materialized)
                && function
                    .parameters
                    .iter()
                    .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
        }
        TypeExpr::IndexedAccess { object, index } => {
            dispatch_route_expr_is_materialized(object)
                && dispatch_route_expr_is_materialized(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            dispatch_route_expr_is_materialized(check)
                && dispatch_route_expr_is_materialized(extends)
                && dispatch_route_expr_is_materialized(true_type)
                && dispatch_route_expr_is_materialized(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            dispatch_route_expr_is_materialized(source)
                && dispatch_route_expr_is_materialized(value)
                && name_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        // Synthetic carriers are fully materialised at the projector
        // surface — they ARE the published leaf, not a deferred token.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

/// Detects sentinel tokens emitted by `raise_node_to_type_expr_inner`
/// when dispatch cannot materialise a node. Dispatch-first paths fall
/// back to `owner_engine` when the sentinel is present — transitional
/// until §5.8 retires the owner_engine bridge.
pub(crate) fn type_expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    !dispatch_route_expr_is_materialized(expr)
}

/// Returns `true` when `expr` still carries open deferred shell shapes
/// (`KeyOf`, `IndexedAccess`, `Mapped`, `TypeOf`, `Conditional`) that
/// indicate dispatch could not structurally expand the surface further.
pub(crate) fn type_expr_is_expanded_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. } => false,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(type_expr_is_expanded_surface)
        }
        _ => true,
    }
}

pub(crate) fn semantic_query_error_raw(err: &QueryError) -> String {
    match err {
        QueryError::Miss => SEMANTIC_MISS.to_string(),
        QueryError::Other(text) => text.as_ref().to_string(),
        QueryError::UnsupportedIntrinsic { name } => format!("unsupportedIntrinsic({name})"),
        QueryError::BudgetExceeded(failure) => format!("budgetExceeded({:?})", failure.domain),
        QueryError::UnstableState { attempts } => format!("unstableState({attempts})"),
        QueryError::AliasCycle { chain } => format!("aliasCycle({})", chain.len()),
        QueryError::RecursiveRef { name } => format!("recursiveRef({name})"),
        QueryError::DeclPlaceholder { name, .. } => format!("declPlaceholder({name})"),
    }
}

pub(super) fn projected_surface_is_empty(surface: &ProjectedSurface) -> bool {
    surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
}

pub(super) fn build_default_type_param_substitutions(
    prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
    type_arguments: &[TypeExpr],
) -> Option<FxHashMap<String, TypeExpr>> {
    if type_arguments.len() > prepared.type_parameters.len() {
        return None;
    }

    let mut substitutions = FxHashMap::default();
    for (index, type_parameter) in prepared.type_parameters.iter().enumerate() {
        let arg = if let Some(arg) = type_arguments.get(index) {
            arg.clone()
        } else if let Some(default) = type_parameter.default.as_deref() {
            default.clone()
        } else {
            continue;
        };
        if is_identity_type_param_binding(&arg, &type_parameter.name) {
            continue;
        }
        substitutions.insert(type_parameter.name.clone(), arg);
    }
    Some(substitutions)
}

fn is_identity_type_param_binding(expr: &TypeExpr, param_name: &str) -> bool {
    matches!(
        expr,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && name.as_ref() == param_name
    )
}

pub(super) fn apply_type_param_substitutions(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> TypeExpr {
    if substitutions.is_empty() || !type_expr_references_substitutions(expr, substitutions) {
        expr.clone()
    } else {
        // Live slow-lane trip-wire: a real substitution is about to run.
        // Placed after the empty/no-reference fast-return so it never trips
        // when no substitution would occur.
        super::assert_prepared_structural_substitution_slow_lane_allowed(expr);
        substitute_type_expr(expr, substitutions)
    }
}

fn substitute_type_expr(expr: &TypeExpr, substitutions: &FxHashMap<String, TypeExpr>) -> TypeExpr {
    use verter_type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => substitutions
            .get(name.as_ref())
            .cloned()
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: std::sync::Arc::from(
                type_arguments
                    .iter()
                    .map(|arg| substitute_type_expr(arg, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
            substitute_type_expr(inner, substitutions),
        )),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(substitute_type_expr(element, substitutions)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                elements
                    .iter()
                    .map(|element| verter_type_expr::TupleElement {
                        label: element.label.clone(),
                        ty: substitute_type_expr(&element.ty, substitutions),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => TypeExpr::Union(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(types) => TypeExpr::Intersection(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Object(object) => {
            TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                properties: object
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(property) => {
                            // Structure-preserving substitution: keep the
                            // member's OXC declaration-site spans verbatim.
                            ObjectMember::Property(verter_type_expr::ObjectProperty::with_spans(
                                property.name.clone(),
                                substitute_type_expr(&property.ty, substitutions),
                                property.optional,
                                property.readonly,
                                property.spans,
                            ))
                        }
                        ObjectMember::Method(method) => {
                            let mut method = method.clone();
                            for parameter in &mut method.function.parameters {
                                parameter.ty = substitute_type_expr(&parameter.ty, substitutions);
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = std::sync::Arc::new(substitute_type_expr(
                                    return_type,
                                    substitutions,
                                ));
                            }
                            ObjectMember::Method(method)
                        }
                        ObjectMember::IndexSignature(signature) => {
                            ObjectMember::IndexSignature(
                                verter_type_expr::IndexSignature::with_spans(
                                    signature.key_name.clone(),
                                    substitute_type_expr(&signature.key_type, substitutions),
                                    substitute_type_expr(&signature.value_type, substitutions),
                                    signature.readonly,
                                    signature.spans,
                                ),
                            )
                        }
                        ObjectMember::CallSignature(function) => ObjectMember::CallSignature(
                            substitute_function_expr(function, substitutions),
                        ),
                        ObjectMember::ConstructSignature(function) => {
                            ObjectMember::ConstructSignature(substitute_function_expr(
                                function,
                                substitutions,
                            ))
                        }
                    })
                    .collect(),
            }))
        }
        TypeExpr::Function(function) => TypeExpr::Function(std::sync::Arc::new(
            substitute_function_expr(function, substitutions),
        )),
        TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(substitute_type_expr(object, substitutions)),
            index: std::sync::Arc::new(substitute_type_expr(index, substitutions)),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: std::sync::Arc::new(substitute_type_expr(check, substitutions)),
            extends: std::sync::Arc::new(substitute_type_expr(extends, substitutions)),
            true_type: std::sync::Arc::new(substitute_type_expr(true_type, substitutions)),
            false_type: std::sync::Arc::new(substitute_type_expr(false_type, substitutions)),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let mut scoped_substitutions = substitutions.clone();
            scoped_substitutions.remove(parameter.as_str());
            TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: std::sync::Arc::new(substitute_type_expr(source, &scoped_substitutions)),
                value: std::sync::Arc::new(substitute_type_expr(value, &scoped_substitutions)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_deref().map(|inner| {
                    std::sync::Arc::new(substitute_type_expr(inner, &scoped_substitutions))
                }),
            }
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: std::sync::Arc::from(
                expressions
                    .iter()
                    .map(|inner| substitute_type_expr(inner, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::Rest(inner) => TypeExpr::Rest(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::TypeParameter(type_parameter) => {
            if let Some(substituted) = substitutions.get(type_parameter.name.as_str()) {
                return substituted.clone();
            }
            let mut type_parameter = type_parameter.clone();
            if let Some(constraint) = type_parameter.constraint.as_mut() {
                *constraint = std::sync::Arc::new(substitute_type_expr(constraint, substitutions));
            }
            if let Some(default) = type_parameter.default.as_mut() {
                *default = std::sync::Arc::new(substitute_type_expr(default, substitutions));
            }
            TypeExpr::TypeParameter(type_parameter)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::RecursiveRef { .. }
        // Synthetic carriers carry no embedded type parameters; passthrough.
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Infer { .. } => expr.clone(),
    }
}

fn substitute_function_expr(
    function: &verter_type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> verter_type_expr::FunctionExpr {
    let mut scoped_substitutions = substitutions.clone();
    for type_parameter in &function.type_parameters {
        scoped_substitutions.remove(type_parameter.name.as_str());
    }

    let mut function = function.clone();
    for parameter in &mut function.parameters {
        parameter.ty = substitute_type_expr(&parameter.ty, &scoped_substitutions);
    }
    if let Some(return_type) = function.return_type.as_mut() {
        *return_type =
            std::sync::Arc::new(substitute_type_expr(return_type, &scoped_substitutions));
    }
    for type_parameter in &mut function.type_parameters {
        if let Some(constraint) = type_parameter.constraint.as_mut() {
            *constraint =
                std::sync::Arc::new(substitute_type_expr(constraint, &scoped_substitutions));
        }
        if let Some(default) = type_parameter.default.as_mut() {
            *default = std::sync::Arc::new(substitute_type_expr(default, &scoped_substitutions));
        }
    }
    function
}

pub(crate) fn projected_surface_to_type_expr(surface: &ProjectedSurface) -> Option<TypeExpr> {
    use std::sync::Arc;
    use verter_type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
        PrimitiveName,
    };

    if surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
    {
        return None;
    }

    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return surface.call_signatures.first().cloned();
    }

    // `ProjectedMember` carries the real OXC declaration-site spans
    // (`member.spans`), threaded from the graph `SurfaceMember` / `PreparedMember`
    // / IR source the surface was projected from. Re-emit them verbatim onto the
    // reconstructed IR member so the projection path is span-lossless end-to-end.
    let mut properties = surface
        .members
        .iter()
        .map(|member| {
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    return ObjectMember::Method(MethodSignature::with_spans(
                        member.name.clone(),
                        (**function).clone(),
                        member.optional,
                        member.spans,
                    ));
                }
            }

            ObjectMember::Property(ObjectProperty::with_spans(
                member.name.clone(),
                member.ty.clone(),
                member.optional,
                member.readonly,
                member.spans,
            ))
        })
        .collect::<Vec<_>>();

    for signature in &surface.call_signatures {
        if let TypeExpr::Function(function) = signature {
            // Preserve the call-signature function shape's OXC spans verbatim.
            properties.push(ObjectMember::CallSignature(FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.clone(),
                function.type_parameters.clone(),
                function.spans,
            )));
        }
    }

    for signature in &surface.construct_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::ConstructSignature(FunctionExpr::with_spans(
                function.parameters.clone(),
                function.return_type.clone(),
                function.type_parameters.clone(),
                function.spans,
            )));
        }
    }

    // A REAL `[k: K]: V` index signature (sourced from an OXC declaration site,
    // carried structurally on `ProjectedSurface::index_signatures`) re-emits its
    // declared key/value shape AND its real spans — losslessly. Reverting this
    // to the synthetic-`None` placeholder (the pre-fix state) drops both the
    // shape and the spans.
    for signature in &surface.index_signatures {
        properties.push(ObjectMember::IndexSignature(IndexSignature::with_spans(
            signature.key_name.clone(),
            signature.key_type.clone(),
            signature.value_type.clone(),
            signature.readonly,
            signature.spans,
        )));
    }

    // Emit the synthetic open-surface placeholder ONLY when the surface is
    // GENUINELY OPEN — `has_index_signature` is set but no concrete signature
    // payload was carried (e.g. a mapped/inferred open surface). This placeholder
    // has no single OXC declaration site, so its spans stay `None` by design
    // (not a deferral): there is no source range to anchor to.
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        properties.push(ObjectMember::IndexSignature(IndexSignature::synthetic(
            "key".to_string(),
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            false,
        )));
    }

    Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
}

pub(crate) fn projected_surface_to_expanded_shape(
    surface: &ProjectedSurface,
) -> verter_semantic::analysis::type_expand::ExpandedObjectShape {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_type_expr::PrimitiveName;

    let properties = surface
        .members
        .iter()
        .map(|member| ExpandedProperty {
            name: member.name.clone(),
            ty: member.ty.clone(),
            optional: member.optional,
            readonly: member.readonly,
            declared_in_macro_type_arg: member.declared_in_macro_type_arg,
        })
        .collect::<Vec<_>>();

    let mut call_signatures = surface
        .call_signatures
        .iter()
        .chain(surface.construct_signatures.iter())
        .filter_map(|signature| match signature {
            TypeExpr::Function(function) => Some(ExpandedCallSignature {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| ExpandedParameter {
                        name: parameter.name.clone().unwrap_or_default(),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect(),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.as_ref().clone())
                    .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                type_parameters: function.type_parameters.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut index_signatures = Vec::new();
    // Concrete declared index signatures preserve their real key/value shape
    // (the expand layer does not track spans).
    for signature in &surface.index_signatures {
        index_signatures.push(ExpandedIndexSignature {
            key_type: signature.key_type.clone(),
            value_type: signature.value_type.clone(),
            readonly: signature.readonly,
        });
    }
    // Genuinely-open surface (flag set, no concrete payload) → open placeholder.
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        index_signatures.push(ExpandedIndexSignature {
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        });
    }

    // Preserve previous round-trip behavior: call and construct signatures
    // both become call signatures after object-shape extraction.
    if !surface.call_signatures.is_empty() && !surface.construct_signatures.is_empty() {
        call_signatures.shrink_to_fit();
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

pub(super) fn type_expr_references_substitutions(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> bool {
    type_expr_references_names(expr, &|name| substitutions.contains_key(name))
}

pub(super) fn type_expr_references_names(
    expr: &TypeExpr,
    contains_name: &impl Fn(&str) -> bool,
) -> bool {
    fn visit(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeOf(_)
            // Synthetic carriers reference no substitutable names —
            // their identity is closed and intrinsic to the carrier
            // tuple.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Infer { .. } => false,
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                contains_name(name.as_ref())
                    || type_arguments.iter().any(|arg| visit(arg, contains_name))
            }
            TypeExpr::TypeParameter(param) => {
                contains_name(param.name.as_str())
                    || param
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, contains_name))
                    || param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, contains_name))
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => visit(inner, contains_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, contains_name)),
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, contains_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, contains_name)
                }
                verter_type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, contains_name)
                        || visit(&signature.value_type, contains_name)
                }
                verter_type_expr::ObjectMember::CallSignature(function)
                | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
            }),
            TypeExpr::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, contains_name))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, contains_name))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, contains_name))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, contains_name))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, contains_name) || visit(index, contains_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, contains_name)
                    || visit(extends, contains_name)
                    || visit(true_type, contains_name)
                    || visit(false_type, contains_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, contains_name)
                    || visit(value, contains_name)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, contains_name))
            }
        }
    }

    visit(expr, contains_name)
}
