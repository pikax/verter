//! `substitute_semantic_type_param` — generic type-parameter
//! substitution into the semantic graph (plan §3 Change Split + §2
//! guard contract row for `substitute_semantic_type_param`).
//!
//! Phase 6 / Fix D wraps the public substitute in
//! `substitute_with_change_tracking`, which returns `(result,
//! changed)`. Each match arm short-circuits the rebuild when no
//! descendant produced a different `SemanticNodeId`, skipping
//! `intern_preserving_scope` and the per-arm `Vec<>` allocations.
//! The output is identical to the pre-Fix-D path (the existing
//! shard dedup collapses identical rebuilds back to the same id);
//! the optimization avoids the wasted recursive walk and allocations.
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
    pub(super) fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        // Phase 6 (plan §8 / Fix D) — change-tracking through
        // recursion. The internal helper returns (result, changed)
        // so each branch can short-circuit the rebuild path when no
        // descendant produced a different node id. The public
        // signature is unchanged; callers see only the result id.
        self.substitute_with_change_tracking(node, parameter_node, arg)
            .0
    }

    /// Phase 6 / Fix D internal helper. Returns `(result, changed)`
    /// where `changed` is `true` if any descendant produced a
    /// different `SemanticNodeId` from its input. When `changed` is
    /// `false`, every recursive arm returns `(node, false)` directly
    /// so the rebuild + `intern_preserving_scope` allocations are
    /// skipped entirely.
    ///
    /// Identity preservation: the existing
    /// `intern_preserving_scope` shard dedup also collapses
    /// rebuilt-but-identical structures back to the same
    /// `SemanticNodeId`, so the OUTPUT is identical between the
    /// pre-Fix-D and post-Fix-D paths. The change-tracking
    /// optimization avoids the wasted recursive walk + Vec<>
    /// allocations on the hot substitute path.
    fn substitute_with_change_tracking(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> (SemanticNodeId, bool) {
        // Path C C6a item 8 — node-id equality match BEFORE
        // destructuring. The pre-C6a string-match arm
        // (`TypeParam { display_name, .. } if display_name == parameter`)
        // would substitute every binder sharing the parameter's
        // display_name; under C6a's complete identity model, only the
        // binder whose `SemanticNodeId` matches gets substituted.
        if node == parameter_node {
            return (arg, true);
        }
        let Some(data) = self.graph().node_data(node) else {
            return (self.opaque(QueryError::Miss), true);
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
                        return (arg, true);
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
                (arg, true)
            }
            SemanticNodeData::Alias(target) => {
                let (sub, changed) =
                    self.substitute_with_change_tracking(*target, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph()
                        .intern_preserving_scope(node, SemanticNodeData::Alias(sub)),
                    true,
                )
            }
            SemanticNodeData::Union(members) => {
                let mut new_members = Vec::with_capacity(members.len());
                let mut any_changed = false;
                for member in members.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*member, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(Arc::from(new_members.into_boxed_slice())),
                    ),
                    true,
                )
            }
            SemanticNodeData::Intersection(members) => {
                let mut new_members = Vec::with_capacity(members.len());
                let mut any_changed = false;
                for member in members.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*member, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(Arc::from(new_members.into_boxed_slice())),
                    ),
                    true,
                )
            }
            SemanticNodeData::Array { element, readonly } => {
                let (sub_element, changed) =
                    self.substitute_with_change_tracking(*element, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element: sub_element,
                            readonly: *readonly,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                let mut new_elements = Vec::with_capacity(elements.len());
                let mut any_changed = false;
                for element in elements.iter() {
                    let (sub_value, c) =
                        self.substitute_with_change_tracking(element.value, parameter_node, arg);
                    any_changed |= c;
                    new_elements.push(crate::semantic_query::TupleElement {
                        label: element.label.clone(),
                        value: sub_value,
                        optional: element.optional,
                        rest: element.rest,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Tuple {
                            elements: Arc::from(new_elements.into_boxed_slice()),
                            readonly: *readonly,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Object(surface) => {
                let mut any_changed = false;
                let mut new_members = Vec::with_capacity(surface.members.len());
                for member in surface.members.iter() {
                    let (sub_value, c) =
                        self.substitute_with_change_tracking(member.value, parameter_node, arg);
                    any_changed |= c;
                    new_members.push(SurfaceMember {
                        name: Arc::clone(&member.name),
                        value: sub_value,
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    });
                }
                let mut new_call_signatures = Vec::with_capacity(surface.call_signatures.len());
                for signature in surface.call_signatures.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*signature, parameter_node, arg);
                    any_changed |= c;
                    new_call_signatures.push(sub);
                }
                let mut new_construct_signatures =
                    Vec::with_capacity(surface.construct_signatures.len());
                for signature in surface.construct_signatures.iter() {
                    let (sub, c) =
                        self.substitute_with_change_tracking(*signature, parameter_node, arg);
                    any_changed |= c;
                    new_construct_signatures.push(sub);
                }
                let mut new_index_signatures = Vec::with_capacity(surface.index_signatures.len());
                for signature in surface.index_signatures.iter() {
                    let (sub_key, ck) = self.substitute_with_change_tracking(
                        signature.key_type,
                        parameter_node,
                        arg,
                    );
                    let (sub_value, cv) = self.substitute_with_change_tracking(
                        signature.value_type,
                        parameter_node,
                        arg,
                    );
                    any_changed |= ck || cv;
                    new_index_signatures.push(IndexSignature {
                        key_type: sub_key,
                        value_type: sub_value,
                        readonly: signature.readonly,
                    });
                }
                let new_keyspace = match surface.keyspace {
                    Some(k) => {
                        let (sub, c) = self.substitute_with_change_tracking(k, parameter_node, arg);
                        any_changed |= c;
                        Some(sub)
                    }
                    None => None,
                };
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Object(SurfaceView {
                            members: Arc::from(new_members.into_boxed_slice()),
                            call_signatures: Arc::from(new_call_signatures.into_boxed_slice()),
                            construct_signatures: Arc::from(
                                new_construct_signatures.into_boxed_slice(),
                            ),
                            index_signatures: Arc::from(new_index_signatures.into_boxed_slice()),
                            keyspace: new_keyspace,
                            has_index_signature: surface.has_index_signature,
                        }),
                    ),
                    true,
                )
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let mut new_expressions = Vec::with_capacity(expressions.len());
                let mut any_changed = false;
                for expr in expressions.iter() {
                    let (sub, c) = self.substitute_with_change_tracking(*expr, parameter_node, arg);
                    any_changed |= c;
                    new_expressions.push(sub);
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::TemplateLiteral {
                            quasis: Arc::clone(quasis),
                            expressions: Arc::from(new_expressions.into_boxed_slice()),
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::KeyOf { base } => {
                let (sub_base, changed) =
                    self.substitute_with_change_tracking(*base, parameter_node, arg);
                if !changed {
                    return (node, false);
                }
                (
                    self.graph()
                        .intern_preserving_scope(node, SemanticNodeData::KeyOf { base: sub_base }),
                    true,
                )
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let (sub_object, oc) =
                    self.substitute_with_change_tracking(*object, parameter_node, arg);
                let (sub_index, ic) =
                    self.substitute_index_key_with_change_tracking(index, parameter_node, arg);
                if !(oc || ic) {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: sub_object,
                            index: sub_index,
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::Mapped { source, mapper } => {
                // Path C C6a item 9b: shadowing check by node-id
                // equality. Pre-C6a this compared `mapper.parameter`
                // (Arc<str>) to `parameter` (&str); post-C6a both are
                // `SemanticNodeId`s, so the comparison is direct.
                let shadowed = mapper.parameter_node == parameter_node;
                let (sub_source, source_changed) =
                    self.substitute_with_change_tracking(*source, parameter_node, arg);
                let (sub_key_space, key_space_changed) =
                    self.substitute_with_change_tracking(mapper.key_space, parameter_node, arg);
                let (sub_value_expr, value_expr_changed) = if shadowed {
                    (mapper.value_expr, false)
                } else {
                    self.substitute_with_change_tracking(mapper.value_expr, parameter_node, arg)
                };
                let (sub_name_remap, name_remap_changed) = match mapper.name_remap {
                    Some(n) if !shadowed => {
                        let (sub, c) = self.substitute_with_change_tracking(n, parameter_node, arg);
                        (Some(sub), c)
                    }
                    other => (other, false),
                };
                let any_changed =
                    source_changed || key_space_changed || value_expr_changed || name_remap_changed;
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Mapped {
                            source: sub_source,
                            mapper: MapperKey {
                                parameter_node: mapper.parameter_node,
                                key_space: sub_key_space,
                                value_expr: sub_value_expr,
                                optionality: mapper.optionality,
                                readonly: mapper.readonly,
                                name_remap: sub_name_remap,
                                // Path C C5: propagate the lowering-time
                                // kind through substitution. Identity is
                                // preserved across substitution because
                                // substitution applies uniformly to both
                                // source and value_expr.object for
                                // non-shadowed substitutions.
                                kind: mapper.kind,
                            },
                        },
                    ),
                    true,
                )
            }
            SemanticNodeData::TypeOf { .. } => {
                // TypeOf carries opaque value-root + path; no
                // substitution descends into it. Returns the input
                // node unchanged. This was a re-intern in the pre-
                // Fix-D code (which dedup'd to the same id anyway);
                // the change-tracking version returns the input
                // directly without any work.
                (node, false)
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                let (sub_check, cc) =
                    self.substitute_with_change_tracking(*check, parameter_node, arg);
                let (sub_extends, ec) =
                    self.substitute_with_change_tracking(*extends, parameter_node, arg);
                let (sub_true, tc) =
                    self.substitute_with_change_tracking(*true_branch_ref, parameter_node, arg);
                let (sub_false, fc) =
                    self.substitute_with_change_tracking(*false_branch_ref, parameter_node, arg);
                if !(cc || ec || tc || fc) {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Conditional {
                            check: sub_check,
                            extends: sub_extends,
                            true_branch_ref: sub_true,
                            false_branch_ref: sub_false,
                            distributive: *distributive,
                        },
                    ),
                    true,
                )
            }
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
            } => {
                let mut any_changed = false;
                let mut new_params = Vec::with_capacity(params.len());
                for param in params.iter() {
                    let (sub_ty, c) =
                        self.substitute_with_change_tracking(param.ty, parameter_node, arg);
                    any_changed |= c;
                    new_params.push(FunctionParam {
                        name: param.name.clone(),
                        ty: sub_ty,
                        optional: param.optional,
                        rest: param.rest,
                    });
                }
                let (sub_return, return_changed) =
                    self.substitute_with_change_tracking(*return_type, parameter_node, arg);
                any_changed |= return_changed;
                let mut new_type_parameters = Vec::with_capacity(type_parameters.len());
                for tp in type_parameters.iter() {
                    let new_constraint = match tp.constraint {
                        Some(c) => {
                            let (sub, ch) =
                                self.substitute_with_change_tracking(c, parameter_node, arg);
                            any_changed |= ch;
                            Some(sub)
                        }
                        None => None,
                    };
                    let new_default = match tp.default {
                        Some(d) => {
                            let (sub, ch) =
                                self.substitute_with_change_tracking(d, parameter_node, arg);
                            any_changed |= ch;
                            Some(sub)
                        }
                        None => None,
                    };
                    new_type_parameters.push(TypeParamDecl {
                        name: Arc::clone(&tp.name),
                        constraint: new_constraint,
                        default: new_default,
                    });
                }
                if !any_changed {
                    return (node, false);
                }
                (
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Function {
                            params: Arc::from(new_params.into_boxed_slice()),
                            return_type: sub_return,
                            type_parameters: Arc::from(new_type_parameters.into_boxed_slice()),
                        },
                    ),
                    true,
                )
            }
            _ => (node, false),
        }
    }

    /// Phase 6 / Fix D companion of `substitute_index_key`. Returns
    /// `(result, changed)`.
    fn substitute_index_key_with_change_tracking(
        &self,
        index: &IndexKey,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> (IndexKey, bool) {
        match index {
            IndexKey::String(text) => (IndexKey::String(Arc::clone(text)), false),
            IndexKey::Number(number) => (IndexKey::Number(*number), false),
            IndexKey::TypeNode(node) => {
                let (sub, changed) =
                    self.substitute_with_change_tracking(*node, parameter_node, arg);
                if !changed {
                    return (IndexKey::TypeNode(*node), false);
                }
                let normalised = self.normalized_index_key_node(sub);
                (normalised, true)
            }
        }
    }
}
