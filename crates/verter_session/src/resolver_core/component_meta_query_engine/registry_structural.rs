//! Registry structural materialisation, node-domain.
//!
//! `ComponentMetaQueryEngine::materialize_registry_structural_candidate` is the
//! owner-confined home of the registry's structure-preserving materialiser: it
//! walks a registry type's structure, resolving each reference / route LEAF to
//! its surface through the node-domain Class-A / whole-surface helpers (which
//! materialise ONCE at the surface sink), preserves package-backed references
//! symbolically, and threads each leaf's object-surface fact — decided off the
//! producing NODE — up through the recursion, never a semantic decision on a
//! materialised `TypeExpr`.
//!
//! The structural walk is driven over the input `TypeExpr` so the registry route
//! detectors (`component_meta_registry_public_{utility,indexed_access}_route`)
//! classify routes on the untainted input — node-domain route classification
//! would require a forbidden decl-name check. Each leaf resolution carries its
//! own node-domain object-surface fact (the whole-surface candidate's producing
//! node, the Class-A route's projected node), and the compound arms compose
//! those facts (an `Object` is an object surface; a `Union` / `Intersection` is
//! one iff any arm is; every other shape is not) so the top-level fact mirrors
//! the object-surface frontier walk WITHOUT re-lowering the materialised result.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::surface::project_class_a_published_threaded;
use super::ComponentMetaQueryEngine;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_registry::{
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route,
};
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};

impl ComponentMetaQueryEngine<'_> {
    /// Structure-preserving registry materialisation for `raw` in `scope`.
    ///
    /// Returns the materialised structural `TypeExpr` PLUS its object-surface
    /// fact, both produced by the same node-domain walk. Reference / route leaves
    /// resolve through the node-domain Class-A / whole-surface helpers (sink-routed
    /// materialisation) and carry their producing node's object-surface fact;
    /// package-backed references stay symbolic; compound arms compose the leaf
    /// facts. The fact is NEVER recovered by re-lowering the materialised result.
    pub(crate) fn materialize_registry_structural_candidate(
        &mut self,
        scope_canonical_id: &str,
        raw: &TypeExpr,
    ) -> (TypeExpr, bool) {
        let mut active: FxHashSet<SemanticNodeId> = FxHashSet::default();
        structural_materialize(raw, scope_canonical_id, self, &mut active, true)
    }
}

/// Graph-native package check on a lowered `Ref { name, [] }`: lower the bare
/// reference (Navigate), and when it resolves to a `DeclRef` / `InstantiationRef`
/// whose root canonical id is package-backed (via the workspace classifier),
/// report `true` so the caller keeps the reference symbolic. Falls back to
/// `false` when lowering fails or produces a non-reference node.
fn ref_is_package_backed_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    name: &str,
) -> bool {
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let probe = TypeExpr::Ref {
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
            ctx.workspace_is_package_backed(identity.canonical_id.as_ref())
        }
        SemanticNodeData::InstantiationRef { base, .. } => {
            ctx.workspace_is_package_backed(base.canonical_id.as_ref())
        }
        _ => false,
    }
}

/// Structure-preserving recursion: rebuild `expr`, resolving reference / route
/// leaves to their surfaces in node domain. A graph-native cycle guard interns
/// the current expression's Navigate-mode node id and short-circuits a repeat
/// visit by returning the input unchanged (symbolic).
fn structural_materialize(
    expr: &TypeExpr,
    scope_canonical_id: &str,
    engine: &mut ComponentMetaQueryEngine<'_>,
    active: &mut FxHashSet<SemanticNodeId>,
    publish_operators: bool,
) -> (TypeExpr, bool) {
    use verter_type_expr::{ObjectMember, TypeExpr};

    // Graph-native cycle guard: intern the current expr's Navigate-mode node id
    // and use structural identity for cycle tracking. When lowering fails we
    // cannot intern a key — proceed without tracking for this visit.
    let cycle_key = ProjectSemanticDispatch::new(engine.ctx).lower_type_expr_in_scope_with_context(
        scope_canonical_id,
        expr,
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            ProjectionMode::Navigate,
        ),
    );
    if let Some(key) = cycle_key {
        if !active.insert(key) {
            // A revisit short-circuits to the symbolic input: a bare reference
            // back-edge is not itself an object surface.
            return (expr.clone(), false);
        }
    }

    let result: (TypeExpr, bool) = if let Some((root_symbol, _route)) = publish_operators
        .then(|| {
            component_meta_registry_public_utility_route(expr)
                .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        })
        .flatten()
    {
        // Route the utility / indexed expression through the node-domain
        // Class-A helper (registry route fast-path + terminal), materialising
        // the accepted route node ONCE at the surface sink and carrying the
        // route node's object-surface fact. The two-step resolution (try the
        // request scope, then the declaration scope) is preserved so
        // re-exported / barrel-routed declarations resolve.
        project_class_a_published_threaded(engine, scope_canonical_id, expr)
            .or_else(|| {
                let declaration = engine.resolve_type_declaration(scope_canonical_id, &root_symbol);
                (!declaration.canonical_source.is_empty())
                    .then(|| {
                        project_class_a_published_threaded(
                            engine,
                            declaration.canonical_source.as_str(),
                            expr,
                        )
                    })
                    .flatten()
            })
            .unwrap_or_else(|| (expr.clone(), false))
    } else {
        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                // Graph-native package check: a package-backed reference stays
                // symbolic (and is not an object surface); a local reference
                // projects to its whole surface through the node-domain
                // whole-surface candidate, which carries its producing node's
                // object-surface fact.
                if ref_is_package_backed_node(engine.ctx, scope_canonical_id, name) {
                    (expr.clone(), false)
                } else {
                    engine
                        .materialize_registry_whole_surface_candidate(scope_canonical_id, name)
                        .or_else(|| {
                            let declaration =
                                engine.resolve_type_declaration(scope_canonical_id, name);
                            (!declaration.canonical_source.is_empty())
                                .then(|| {
                                    let declaration_name = if declaration.resolved_name.is_empty() {
                                        name.to_string()
                                    } else {
                                        declaration.resolved_name.clone()
                                    };
                                    engine.materialize_registry_whole_surface_candidate(
                                        declaration.canonical_source.as_str(),
                                        declaration_name.as_str(),
                                    )
                                })
                                .flatten()
                        })
                        .unwrap_or_else(|| (expr.clone(), false))
                }
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => (
                TypeExpr::Ref {
                    name: name.clone(),
                    type_arguments: Arc::from(
                        type_arguments
                            .iter()
                            .map(|arg| {
                                structural_materialize(
                                    arg,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0
                            })
                            .collect::<Vec<_>>(),
                    ),
                },
                false,
            ),
            TypeExpr::Parenthesized(inner_expr) => {
                let (inner, is_object) = structural_materialize(
                    inner_expr,
                    scope_canonical_id,
                    engine,
                    active,
                    publish_operators,
                );
                (TypeExpr::Parenthesized(Arc::new(inner)), is_object)
            }
            TypeExpr::Array { element, readonly } => (
                TypeExpr::Array {
                    element: Arc::new(
                        structural_materialize(
                            element,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    ),
                    readonly: *readonly,
                },
                false,
            ),
            TypeExpr::Tuple { elements, readonly } => (
                TypeExpr::Tuple {
                    elements: Arc::from(
                        elements
                            .iter()
                            .map(|element| verter_type_expr::TupleElement {
                                label: element.label.clone(),
                                ty: structural_materialize(
                                    &element.ty,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0,
                                optional: element.optional,
                                rest: element.rest,
                            })
                            .collect::<Vec<_>>(),
                    ),
                    readonly: *readonly,
                },
                false,
            ),
            // A union / intersection is an object surface iff ANY arm is — the
            // composition of the per-arm node-domain facts that mirrors the
            // object-surface frontier walk over `Union` / `Intersection` arms.
            TypeExpr::Union(types) => {
                let mut any_object = false;
                let arms = types
                    .iter()
                    .map(|ty| {
                        let (arm, is_object) = structural_materialize(
                            ty,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        );
                        any_object |= is_object;
                        arm
                    })
                    .collect::<Vec<_>>();
                (TypeExpr::Union(Arc::from(arms)), any_object)
            }
            TypeExpr::Intersection(types) => {
                let mut any_object = false;
                let arms = types
                    .iter()
                    .map(|ty| {
                        let (arm, is_object) = structural_materialize(
                            ty,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        );
                        any_object |= is_object;
                        arm
                    })
                    .collect::<Vec<_>>();
                (TypeExpr::Intersection(Arc::from(arms)), any_object)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => (
                TypeExpr::Conditional {
                    check: Arc::new(
                        structural_materialize(check, scope_canonical_id, engine, active, false).0,
                    ),
                    extends: Arc::new(
                        structural_materialize(extends, scope_canonical_id, engine, active, false)
                            .0,
                    ),
                    true_type: Arc::new(
                        structural_materialize(
                            true_type,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    ),
                    false_type: Arc::new(
                        structural_materialize(
                            false_type,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    ),
                },
                false,
            ),
            TypeExpr::Mapped {
                parameter,
                source,
                optional,
                readonly,
                name_type,
                value,
            } => (
                TypeExpr::Mapped {
                    parameter: parameter.clone(),
                    source: Arc::new(
                        structural_materialize(
                            source,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    ),
                    optional: *optional,
                    readonly: *readonly,
                    name_type: name_type.as_deref().map(|name_type| {
                        Arc::new(
                            structural_materialize(
                                name_type,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0,
                        )
                    }),
                    value: Arc::new(
                        structural_materialize(
                            value,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    ),
                },
                false,
            ),
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => (
                TypeExpr::TemplateLiteral {
                    quasis: quasis.clone(),
                    expressions: Arc::from(
                        expressions
                            .iter()
                            .map(|expr| {
                                structural_materialize(
                                    expr,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0
                            })
                            .collect::<Vec<_>>(),
                    ),
                },
                false,
            ),
            // Both a function type and a constructor type carry the same
            // `FunctionExpr` payload and rewrite their signature identically;
            // only the reconstructed variant differs (a `ConstructorType` is
            // never flattened to a plain `Function`).
            TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
                let is_constructor = matches!(expr, TypeExpr::ConstructorType(_));
                let mut function = function.as_ref().clone();
                for parameter in &mut function.parameters {
                    parameter.ty = structural_materialize(
                        &parameter.ty,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    )
                    .0;
                }
                if let Some(return_type) = function.return_type.as_mut() {
                    *return_type = Arc::new(
                        structural_materialize(
                            return_type,
                            scope_canonical_id,
                            engine,
                            active,
                            publish_operators,
                        )
                        .0,
                    );
                }
                for type_parameter in &mut function.type_parameters {
                    if let Some(constraint) = type_parameter.constraint.as_mut() {
                        *constraint = Arc::new(
                            structural_materialize(
                                constraint,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0,
                        );
                    }
                    if let Some(default) = type_parameter.default.as_mut() {
                        *default = Arc::new(
                            structural_materialize(
                                default,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0,
                        );
                    }
                }
                let function = Arc::new(function);
                let materialized = if is_constructor {
                    TypeExpr::ConstructorType(function)
                } else {
                    TypeExpr::Function(function)
                };
                (materialized, false)
            }
            TypeExpr::KeyOf(inner_expr) => {
                let materialized = if publish_operators {
                    project_class_a_published_threaded(engine, scope_canonical_id, expr)
                        .map(|(type_expr, _)| type_expr)
                        .unwrap_or_else(|| {
                            TypeExpr::KeyOf(Arc::new(
                                structural_materialize(
                                    inner_expr,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0,
                            ))
                        })
                } else {
                    TypeExpr::KeyOf(Arc::new(
                        structural_materialize(
                            inner_expr,
                            scope_canonical_id,
                            engine,
                            active,
                            false,
                        )
                        .0,
                    ))
                };
                // A `keyof` yields a key union, never an explicit object surface.
                (materialized, false)
            }
            TypeExpr::Rest(inner_expr) => (
                TypeExpr::Rest(Arc::new(
                    structural_materialize(
                        inner_expr,
                        scope_canonical_id,
                        engine,
                        active,
                        publish_operators,
                    )
                    .0,
                )),
                false,
            ),
            TypeExpr::Object(object) => {
                let mut object = object.as_ref().clone();
                for member in &mut object.properties {
                    match member {
                        ObjectMember::Property(property) => {
                            property.ty = structural_materialize(
                                &property.ty,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0;
                        }
                        ObjectMember::IndexSignature(signature) => {
                            signature.key_type = structural_materialize(
                                &signature.key_type,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0;
                            signature.value_type = structural_materialize(
                                &signature.value_type,
                                scope_canonical_id,
                                engine,
                                active,
                                publish_operators,
                            )
                            .0;
                        }
                        ObjectMember::CallSignature(function)
                        | ObjectMember::ConstructSignature(function) => {
                            for parameter in &mut function.parameters {
                                parameter.ty = structural_materialize(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0;
                            }
                            if let Some(return_type) = function.return_type.as_mut() {
                                *return_type = Arc::new(
                                    structural_materialize(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    )
                                    .0,
                                );
                            }
                        }
                        ObjectMember::Method(method) => {
                            for parameter in &mut method.function.parameters {
                                parameter.ty = structural_materialize(
                                    &parameter.ty,
                                    scope_canonical_id,
                                    engine,
                                    active,
                                    publish_operators,
                                )
                                .0;
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = Arc::new(
                                    structural_materialize(
                                        return_type,
                                        scope_canonical_id,
                                        engine,
                                        active,
                                        publish_operators,
                                    )
                                    .0,
                                );
                            }
                        }
                    }
                }
                (TypeExpr::Object(Arc::new(object)), true)
            }
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::ImportType { .. }
            | TypeExpr::Infer { .. } => (expr.clone(), false),
        }
    };

    if let Some(key) = cycle_key {
        active.remove(&key);
    }
    result
}
