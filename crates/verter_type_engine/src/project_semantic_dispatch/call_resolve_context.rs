//! The contextual reads of a call's arguments: whether a context-sensitive
//! argument's contextual result reads an inference, the const context of
//! a literal member whose contextual type is a `const` type parameter nested
//! in the parameter's type, and the literal context a type parameter's
//! constraint makes for the literals an argument nests at its positions.

use std::sync::Arc;

use super::super::inference::session::{InferenceInfoSetup, LiteralContext};
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, PrimitiveKind, SemanticNodeData, SemanticNodeId, SignatureKind, TypeParamDecl,
};

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The inference setup of a call candidate's type parameters, each with
    /// its const policy and the literal context its constraint makes.
    pub(super) fn call_inference_infos(
        &self,
        type_parameters: &[TypeParamDecl],
    ) -> Arc<[InferenceInfoSetup]> {
        type_parameters
            .iter()
            .map(|decl| {
                InferenceInfoSetup::for_call(
                    decl.param,
                    Arc::clone(&decl.name),
                    if decl.is_const {
                        ConstParamPolicy::Const
                    } else {
                        ConstParamPolicy::NonConst
                    },
                    decl.constraint.is_some(),
                    self.constraint_literal_context(decl.constraint, type_parameters),
                )
            })
            .collect()
    }

    /// The literal kinds `constraint` makes a literal context for
    /// (`isLiteralOfContextualType` over a type variable): a primitive, a
    /// literal, a template literal or `keyof` member admits its kind, a
    /// union or intersection the kinds of its members, and a type parameter
    /// the kinds of its own constraint — read off the node, else off the
    /// clause's declaration (`T extends U`, `U extends string`). Any other
    /// member (an object, an unresolved carrier) admits none, so its
    /// literals widen as before.
    fn constraint_literal_context(
        &self,
        constraint: Option<SemanticNodeId>,
        type_parameters: &[TypeParamDecl],
    ) -> LiteralContext {
        let graph = self.graph();
        let mut context = LiteralContext::empty();
        let mut pending: Vec<SemanticNodeId> = constraint.into_iter().collect();
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(node) = pending.pop() {
            if !seen.insert(node) {
                continue;
            }
            let kinds = match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Primitive(primitive)) => match primitive {
                    PrimitiveKind::String => LiteralContext::STRING,
                    PrimitiveKind::Number => LiteralContext::NUMBER,
                    PrimitiveKind::BigInt => LiteralContext::BIGINT,
                    PrimitiveKind::Boolean => LiteralContext::BOOLEAN,
                    _ => LiteralContext::empty(),
                },
                Some(SemanticNodeData::Literal(value)) => match value {
                    crate::semantic_query::LiteralValue::String(_) => LiteralContext::STRING,
                    crate::semantic_query::LiteralValue::Number(_) => LiteralContext::NUMBER,
                    crate::semantic_query::LiteralValue::BigInt(_) => LiteralContext::BIGINT,
                    crate::semantic_query::LiteralValue::Boolean(_) => LiteralContext::BOOLEAN,
                },
                Some(SemanticNodeData::TemplateLiteral { .. }) => LiteralContext::STRING,
                Some(SemanticNodeData::KeyOf { .. }) => {
                    LiteralContext::STRING.union(LiteralContext::NUMBER)
                }
                Some(SemanticNodeData::Alias(inner)) => {
                    pending.push(*inner);
                    continue;
                }
                Some(
                    composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                ) => {
                    pending.extend(
                        composite
                            .composite_members()
                            .expect("composite arm")
                            .iter()
                            .copied(),
                    );
                    continue;
                }
                Some(SemanticNodeData::TypeParam {
                    constraint: own, ..
                }) => {
                    let declared = type_parameters
                        .iter()
                        .find(|decl| decl.param == node)
                        .and_then(|decl| decl.constraint);
                    pending.extend(own.or(declared));
                    continue;
                }
                _ => LiteralContext::empty(),
            };
            context = context.union(kinds);
        }
        context
    }

    /// The candidate a NESTED call-argument deposit into a parameter whose
    /// constraint makes `context` takes: a literal of an admitted kind as
    /// written (its contextual type is the parameter), every other literal
    /// widened under `policy`, a union member by member.
    pub(in crate::project_semantic_dispatch) fn contextual_literal_candidate(
        &self,
        bound: SemanticNodeId,
        context: LiteralContext,
        policy: ConstParamPolicy,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let admitted = |node: SemanticNodeId| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Literal(value)) if context.admits(value)
            )
        };
        if admitted(bound) {
            return bound;
        }
        let members = match graph.node_data(bound).as_deref() {
            Some(SemanticNodeData::Union(members)) if members.iter().any(|m| admitted(*m)) => {
                members.iter().copied().collect::<Vec<_>>()
            }
            _ => return self.call_inference_candidate(bound, policy),
        };
        let members: Vec<SemanticNodeId> = members
            .into_iter()
            .map(|member| {
                if admitted(member) {
                    member
                } else {
                    self.call_inference_candidate(member, policy)
                }
            })
            .collect();
        self.relation_combine_candidates(&members, crate::semantic_query::VariancePhase::Covariant)
    }

    /// An unfixed binder retains its constraint as a contextual literal type.
    pub(super) fn contextual_uninferred_parameter(
        &self,
        param: SemanticNodeId,
        constraint: Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        let Some(constraint) = constraint else {
            return param;
        };
        let graph = self.graph();
        let Some(SemanticNodeData::TypeParam {
            decl,
            param_index,
            default,
            display_name,
            constraint: None,
        }) = graph
            .node_data(param)
            .as_deref()
            .map(crate::semantic_query::SemanticNodeData::clone)
        else {
            return param;
        };
        graph.intern_node(SemanticNodeData::TypeParam {
            decl,
            param_index,
            default,
            display_name,
            constraint: Some(constraint),
        })
    }
    /// The object literal argument `argument` with each member whose
    /// position in the parameter type `target` is one of the `const` type
    /// parameters `const_params` read as that member of the argument's const
    /// view (`isConstContext` holds for a literal whose contextual type is a
    /// `const` type parameter, however deeply the parameter's type nests it).
    /// `argument` itself when no member is.
    pub(super) fn nested_const_context_view(
        &self,
        argument: SemanticNodeId,
        const_view: SemanticNodeId,
        target: SemanticNodeId,
        const_params: &[SemanticNodeId],
    ) -> SemanticNodeId {
        if const_params.contains(&target) {
            return const_view;
        }
        let graph = self.graph();
        let (
            Some(SemanticNodeData::Object(argument_view)),
            Some(SemanticNodeData::Object(view)),
            Some(SemanticNodeData::Object(target_view)),
        ) = (
            graph
                .node_data(argument)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
            graph
                .node_data(const_view)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
            graph
                .node_data(target)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
        )
        else {
            return argument;
        };
        let member_value =
            |surface: &crate::semantic_query::SurfaceView,
             key: &crate::semantic_query::AuthoredPropertyKey| {
                surface
                    .positive_members()
                    .iter()
                    .find(|member| member.key == *key)
                    .map(|member| member.value)
            };
        let mut changed = false;
        let entries: Vec<crate::semantic_query::SurfaceEntry> = argument_view
            .entries
            .iter()
            .map(|entry| match entry {
                crate::semantic_query::SurfaceEntry::Member(member) => {
                    let (Some(const_value), Some(target_value)) = (
                        member_value(&view, &member.key),
                        member_value(&target_view, &member.key),
                    ) else {
                        return entry.clone();
                    };
                    let value = self.nested_const_context_view(
                        member.value,
                        const_value,
                        target_value,
                        const_params,
                    );
                    if value == member.value {
                        return entry.clone();
                    }
                    changed = true;
                    let mut member = member.clone();
                    member.value = value;
                    crate::semantic_query::SurfaceEntry::Member(member)
                }
                other => other.clone(),
            })
            .collect();
        if !changed {
            return argument;
        }
        graph.intern_preserving_scope(
            argument,
            SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                entries,
                argument_view.keyspace,
                argument_view.closed().has_index_signature(),
            )),
        )
    }

    /// Whether the result of `target`'s call signatures — its return, or
    /// the type its predicate names, the position the checker infers a
    /// guard's type parameter from (`value is S`) — mentions one of
    /// `params`.
    pub(super) fn contextual_return_mentions(
        &self,
        target: SemanticNodeId,
        params: &[SemanticNodeId],
    ) -> bool {
        match self.shared_signature_nodes(target, SignatureKind::Call) {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => {
                nodes.iter().any(|node| {
                    let (return_type, predicate) = match self.graph().node_data(*node).as_deref() {
                        Some(SemanticNodeData::Signature {
                            return_type,
                            predicate,
                            ..
                        }) => (*return_type, predicate.and_then(|predicate| predicate.ty)),
                        _ => return false,
                    };
                    params.iter().any(|param| {
                        self.mentions_node(return_type, *param)
                            || predicate.is_some_and(|target| self.mentions_node(target, *param))
                    })
                })
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => true,
        }
    }
}
