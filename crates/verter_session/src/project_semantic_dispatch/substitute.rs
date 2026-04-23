//! `substitute_semantic_type_param` + `substitute_index_key` — generic
//! type-parameter substitution into the semantic graph (plan §3 Change
//! Split + §2 guard contract row for `substitute_semantic_type_param`).
//!
//! Both helpers operate on immutable `SemanticNodeData` and publish
//! new shell identity via [`SemanticGraphStore::intern_preserving_scope`]
//! (Path C C6a items 4-5) so the rebuilt shell's scope is preserved
//! from the origin shell. The caller's completion fence observes the
//! new dep-signature through the shared memo once the substituted
//! result enters a build flow.
//!
//! **Path C C6a items 6-8.** Binder matching is done by `SemanticNodeId`
//! equality (the binder's interned `TypeParam` node id) rather than
//! by `display_name` string equality. This makes substitution
//! correct even when two binders in the same file share a display
//! name (`K`) but are otherwise distinct identities — the substitute
//! only touches the binder whose node id matches the caller's
//! `parameter_node` argument.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    FunctionParam, IndexKey, IndexSignature, MapperKey, QueryError, SemanticNodeData,
    SemanticNodeId, SurfaceMember, SurfaceView, TypeParamDecl,
};

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn substitute_index_key(
        &self,
        index: &IndexKey,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> IndexKey {
        match index {
            IndexKey::String(text) => IndexKey::String(Arc::clone(text)),
            IndexKey::Number(number) => IndexKey::Number(*number),
            IndexKey::TypeNode(node) => self.normalized_index_key_node(
                self.substitute_semantic_type_param(*node, parameter_node, arg),
            ),
        }
    }

    pub(super) fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        // Path C C6a item 8 — node-id equality match BEFORE
        // destructuring. The pre-C6a string-match arm
        // (`TypeParam { display_name, .. } if display_name == parameter`)
        // would substitute every binder sharing the parameter's
        // display_name; under C6a's complete identity model, only the
        // binder whose `SemanticNodeId` matches gets substituted.
        if node == parameter_node {
            return arg;
        }
        let Some(data) = self.graph().node_data(node) else {
            return self.opaque(QueryError::Miss);
        };
        // Path C C6a item 8 footnote — cross-variant name fallback.
        // `parameter_node` may be a `TypeParam` (the post-C6a
        // primary path) OR an `Infer` (the build.rs:1432 Infer-arm
        // cross-variant consumer that interns an Infer node and
        // calls substitute). Extract the binder's "name" from
        // either variant so the Infer arm matches Infer references
        // AND any TypeParam references in `node` whose display_name
        // matches the binder's name. The latter handles cases where
        // a TypeScript `infer X` is referenced in the true_branch
        // as a regular TypeParam (lowered from a name-only
        // reference), not as a literal Infer node.
        //
        // Plan §14.2 item 8's "leave Infer string-match alone" is
        // preserved; the addition is purely cross-variant
        // bridging — it does NOT fall back for ordinary TypeParam
        // substitutions (those go through the node-id branch above).
        // C11a re-evaluates whether nested-infer needs full
        // node-id matching.
        let parameter_name: Option<Arc<str>> =
            self.graph()
                .node_data(parameter_node)
                .and_then(|param_data| match param_data.as_ref() {
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(Arc::clone(display_name))
                    }
                    SemanticNodeData::Infer { name } => Some(Arc::clone(name)),
                    _ => None,
                });
        // Cross-variant TypeParam-by-name fallback: when the caller
        // passed an Infer parameter_node, true_branch references
        // may be TypeParam nodes with the matching display_name
        // (the unresolved-TypeParameter lowering path). Match them
        // by name. This branch fires only when parameter_node is
        // an Infer (not a TypeParam) — for TypeParam parameter_node
        // the node-id branch above is the sole authority.
        if let Some(SemanticNodeData::Infer { .. }) =
            self.graph().node_data(parameter_node).as_deref()
        {
            if let SemanticNodeData::TypeParam { display_name, .. } = data.as_ref() {
                if let Some(name) = parameter_name.as_ref() {
                    if display_name.as_ref() == name.as_ref() {
                        return arg;
                    }
                }
            }
        }
        match data.as_ref() {
            SemanticNodeData::Infer { name }
                if parameter_name
                    .as_ref()
                    .map(|n| n.as_ref() == name.as_ref())
                    .unwrap_or(false) =>
            {
                arg
            }
            SemanticNodeData::Alias(target) => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Alias(self.substitute_semantic_type_param(
                    *target,
                    parameter_node,
                    arg,
                )),
            ),
            SemanticNodeData::Union(members) => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Union(Arc::from(
                    members
                        .iter()
                        .map(|member| {
                            self.substitute_semantic_type_param(*member, parameter_node, arg)
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                )),
            ),
            SemanticNodeData::Intersection(members) => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Intersection(Arc::from(
                    members
                        .iter()
                        .map(|member| {
                            self.substitute_semantic_type_param(*member, parameter_node, arg)
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                )),
            ),
            SemanticNodeData::Array { element, readonly } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Array {
                    element: self.substitute_semantic_type_param(*element, parameter_node, arg),
                    readonly: *readonly,
                },
            ),
            SemanticNodeData::Tuple { elements, readonly } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Tuple {
                    elements: Arc::from(
                        elements
                            .iter()
                            .map(|element| crate::semantic_query::TupleElement {
                                label: element.label.clone(),
                                value: self.substitute_semantic_type_param(
                                    element.value,
                                    parameter_node,
                                    arg,
                                ),
                                optional: element.optional,
                                rest: element.rest,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    readonly: *readonly,
                },
            ),
            SemanticNodeData::Object(surface) => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Object(SurfaceView {
                    members: Arc::from(
                        surface
                            .members
                            .iter()
                            .map(|member| SurfaceMember {
                                name: Arc::clone(&member.name),
                                value: self.substitute_semantic_type_param(
                                    member.value,
                                    parameter_node,
                                    arg,
                                ),
                                optional: member.optional,
                                readonly: member.readonly,
                                is_method: member.is_method,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    call_signatures: Arc::from(
                        surface
                            .call_signatures
                            .iter()
                            .map(|signature| {
                                self.substitute_semantic_type_param(*signature, parameter_node, arg)
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    construct_signatures: Arc::from(
                        surface
                            .construct_signatures
                            .iter()
                            .map(|signature| {
                                self.substitute_semantic_type_param(*signature, parameter_node, arg)
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    index_signatures: Arc::from(
                        surface
                            .index_signatures
                            .iter()
                            .map(|signature| IndexSignature {
                                key_type: self.substitute_semantic_type_param(
                                    signature.key_type,
                                    parameter_node,
                                    arg,
                                ),
                                value_type: self.substitute_semantic_type_param(
                                    signature.value_type,
                                    parameter_node,
                                    arg,
                                ),
                                readonly: signature.readonly,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    keyspace: surface.keyspace.map(|keyspace| {
                        self.substitute_semantic_type_param(keyspace, parameter_node, arg)
                    }),
                    has_index_signature: surface.has_index_signature,
                }),
            ),
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::TemplateLiteral {
                    quasis: Arc::clone(quasis),
                    expressions: Arc::from(
                        expressions
                            .iter()
                            .map(|expr| {
                                self.substitute_semantic_type_param(*expr, parameter_node, arg)
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                },
            ),
            SemanticNodeData::KeyOf { base } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::KeyOf {
                    base: self.substitute_semantic_type_param(*base, parameter_node, arg),
                },
            ),
            SemanticNodeData::IndexedAccess { object, index } => {
                self.graph().intern_preserving_scope(
                    node,
                    SemanticNodeData::IndexedAccess {
                        object: self.substitute_semantic_type_param(*object, parameter_node, arg),
                        index: self.substitute_index_key(index, parameter_node, arg),
                    },
                )
            }
            SemanticNodeData::Mapped { source, mapper } => {
                // Path C C6a item 9b: shadowing check by node-id
                // equality. Pre-C6a this compared `mapper.parameter`
                // (Arc<str>) to `parameter` (&str); post-C6a both are
                // `SemanticNodeId`s, so the comparison is direct.
                let shadowed = mapper.parameter_node == parameter_node;
                self.graph().intern_preserving_scope(
                    node,
                    SemanticNodeData::Mapped {
                        source: self.substitute_semantic_type_param(*source, parameter_node, arg),
                        mapper: MapperKey {
                            parameter_node: mapper.parameter_node,
                            key_space: self.substitute_semantic_type_param(
                                mapper.key_space,
                                parameter_node,
                                arg,
                            ),
                            value_expr: if shadowed {
                                mapper.value_expr
                            } else {
                                self.substitute_semantic_type_param(
                                    mapper.value_expr,
                                    parameter_node,
                                    arg,
                                )
                            },
                            optionality: mapper.optionality,
                            readonly: mapper.readonly,
                            name_remap: mapper.name_remap.map(|n| {
                                if shadowed {
                                    n
                                } else {
                                    self.substitute_semantic_type_param(n, parameter_node, arg)
                                }
                            }),
                            // Path C C5: propagate the lowering-time
                            // kind through substitution. Identity is
                            // preserved across substitution because
                            // substitution applies uniformly to both
                            // source and value_expr.object for
                            // non-shadowed substitutions.
                            kind: mapper.kind,
                        },
                    },
                )
            }
            SemanticNodeData::TypeOf { value_root, path } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::TypeOf {
                    value_root: value_root.clone(),
                    path: Arc::clone(path),
                },
            ),
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Conditional {
                    check: self.substitute_semantic_type_param(*check, parameter_node, arg),
                    extends: self.substitute_semantic_type_param(*extends, parameter_node, arg),
                    true_branch_ref: self.substitute_semantic_type_param(
                        *true_branch_ref,
                        parameter_node,
                        arg,
                    ),
                    false_branch_ref: self.substitute_semantic_type_param(
                        *false_branch_ref,
                        parameter_node,
                        arg,
                    ),
                    distributive: *distributive,
                },
            ),
            // Path C C11a — Function arm. Pre-C11a the catch-all left
            // Function shells untouched, so `T` / `infer X` references
            // inside `(x: T, y: infer X) => R` leaked through
            // substitution unchanged. This is the primary materialisation
            // path for nested-infer in TS conditional `extends` clauses
            // (plan §2 Pass C11a).
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
            } => self.graph().intern_preserving_scope(
                node,
                SemanticNodeData::Function {
                    params: Arc::from(
                        params
                            .iter()
                            .map(|param| FunctionParam {
                                name: param.name.clone(),
                                ty: self.substitute_semantic_type_param(
                                    param.ty,
                                    parameter_node,
                                    arg,
                                ),
                                optional: param.optional,
                                rest: param.rest,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    return_type: self.substitute_semantic_type_param(
                        *return_type,
                        parameter_node,
                        arg,
                    ),
                    type_parameters: Arc::from(
                        type_parameters
                            .iter()
                            .map(|tp| TypeParamDecl {
                                name: Arc::clone(&tp.name),
                                constraint: tp.constraint.map(|c| {
                                    self.substitute_semantic_type_param(c, parameter_node, arg)
                                }),
                                default: tp.default.map(|d| {
                                    self.substitute_semantic_type_param(d, parameter_node, arg)
                                }),
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                },
            ),
            _ => node,
        }
    }
}
