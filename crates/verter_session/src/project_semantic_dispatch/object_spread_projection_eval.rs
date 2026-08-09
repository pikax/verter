use std::sync::Arc;

use verter_type_expr::{ExcessPropertyOrigin, MemberVisibility, ObjectMethodKind};

use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::resolver_core::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::object_spread_projection::evaluator_support;
use crate::semantic_query::{
    AuthoredAccessorEffect, AuthoredMethodEffect, AuthoredPropertyEffect, AuthoredPropertyKey,
    ExactOptionalPropertyPolicy, ExcessEligibility, IndexDomain, MemberFacets,
    ObjectConstructionEffect, ObjectProjectionFormula, ObjectProjectionIndex,
    ObjectProjectionSelector, ObjectProjectionSignature, ObjectSignatureKind, ObjectSpreadProgram,
    ObjectSpreadProjectionContext, PositiveKeyPresence, PrimitiveKind, ProjectionEvidence,
    PropertyKey, QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, SurfaceMember,
};

const DISTRIBUTION_CAP: usize = 1024;
const SPREAD_DEPTH_CAP: usize = 8;

#[derive(Clone)]
struct MemberState {
    key: PropertyKey,
    presence: PositiveKeyPresence,
    value: ProjectionEvidence<SemanticNodeId>,
    facets: ProjectionEvidence<MemberFacets>,
}

#[derive(Clone)]
struct EvalState {
    members: Vec<MemberState>,
    indices: Vec<ObjectProjectionIndex>,
    signatures: Vec<ObjectProjectionSignature>,
    residual_operands: Vec<SemanticNodeId>,
    possible_writes: Vec<PropertyKey>,
    direct_excess_candidates: Vec<PropertyKey>,
    generic_spread_seen: bool,
    unclassified_spread_seen: bool,
    accessor_getters: Vec<PropertyKey>,
}

impl EvalState {
    fn empty() -> Self {
        Self {
            members: Vec::new(),
            indices: Vec::new(),
            signatures: Vec::new(),
            residual_operands: Vec::new(),
            possible_writes: Vec::new(),
            direct_excess_candidates: Vec::new(),
            generic_spread_seen: false,
            unclassified_spread_seen: false,
            accessor_getters: Vec::new(),
        }
    }

    /// JS property identity: `{1: x}` and `{"1": y}` address the SAME
    /// property, so a later write under either spelling replaces the
    /// earlier fact (element-access collision, not strict key equality).
    fn member_position(&self, key: &PropertyKey) -> Option<usize> {
        self.members
            .iter()
            .position(|member| member.key.element_access_collides(key))
    }

    fn remember_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
        if !values.contains(&value) {
            values.push(value);
        }
    }

    /// Property-key dedup under JS property identity (see
    /// [`Self::member_position`]).
    fn remember_unique_key(values: &mut Vec<PropertyKey>, value: PropertyKey) {
        if !values
            .iter()
            .any(|candidate| candidate.element_access_collides(&value))
        {
            values.push(value);
        }
    }
}

enum EvalFailure {
    /// An operand or nested query failed with its own typed reason. The
    /// reason is propagated unchanged so operational partials stay
    /// distinguishable from a genuine miss.
    Operational(QueryError),
    /// The correlated alternative product exceeded the distribution cap.
    DistributionLimit { actual: usize },
    /// Nested spread expansion exceeded the depth cap.
    DepthLimit { depth: usize },
    /// A nested program projection re-entered an in-flight query.
    Recursive(SemanticNodeId),
}

impl EvalFailure {
    fn into_query_error(self) -> QueryError {
        match self {
            EvalFailure::Operational(error) => error,
            EvalFailure::DistributionLimit { actual } => {
                QueryError::BudgetExceeded(BudgetExceededFailure {
                    domain: BudgetDomain::ProjectionOperation,
                    limit: DISTRIBUTION_CAP,
                    actual: actual as u64,
                    context: "object spread alternative product".into(),
                })
            }
            EvalFailure::DepthLimit { depth } => {
                QueryError::BudgetExceeded(BudgetExceededFailure {
                    domain: BudgetDomain::ProjectionOperation,
                    limit: SPREAD_DEPTH_CAP,
                    actual: depth as u64,
                    context: "object spread nesting depth".into(),
                })
            }
            EvalFailure::Recursive(_) => unreachable!("recursion is not an error channel"),
        }
    }
}

/// How a residual spread operand classifies for excess eligibility.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResidualExcessClass {
    /// A semantically generic operand: suppresses every direct candidate.
    Generic,
    /// An operand that cannot be classified: indeterminate, not suppression.
    Unclassified,
    /// A residual that carries no generic-spread evidence (for example an
    /// unresolved computed key): no eligibility effect.
    Neutral,
}

struct SpreadAlternative {
    members: Vec<MemberState>,
    indices: Vec<ObjectProjectionIndex>,
    residual: bool,
}

impl<'a> ProjectSemanticDispatch<'a> {
    pub(crate) fn project_object_spread_for_consumer(
        &self,
        program: SemanticNodeId,
        selector: ObjectProjectionSelector,
        projection_reduction: crate::semantic_query::ProjectionReductionContext,
    ) -> QueryResult<ObjectProjectionFormula> {
        let canonical = self
            .graph()
            .node_scope(program)
            .and_then(|scope| scope.canonical_file())
            .unwrap_or_else(|| Arc::from(""));
        let policy = if self.relation_strict_config().exact_optional_property_types {
            ExactOptionalPropertyPolicy::Enabled
        } else {
            ExactOptionalPropertyPolicy::Disabled
        };
        let context = self.object_spread_projection_context_for(
            &canonical,
            projection_reduction,
            crate::semantic_query::SubstitutionCanonicalHash::empty(),
            policy,
        );
        match self
            .execute_via_cold_build_helper(SemanticQueryKey::ProjectObjectSpread {
                program,
                selector,
                context,
            })
            .value
        {
            QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)) => {
                QueryResult::Value(formula)
            }
            QueryResult::Value(_) => QueryResult::Error(QueryError::Miss),
            QueryResult::Error(error) => QueryResult::Error(error),
            QueryResult::Recursive(node) => QueryResult::Recursive(node),
        }
    }

    pub(super) fn build_project_object_spread(
        &self,
        program_id: SemanticNodeId,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let Some(data) = self.graph().node_data(program_id) else {
            return self.object_projection_failure(QueryError::Miss);
        };
        let SemanticNodeData::ObjectSpreadProgram(program) = data.as_ref() else {
            return self.object_projection_failure(QueryError::Miss);
        };
        let program = program.clone();
        drop(data);

        let start = selector_live_start(&program, selector);
        let mut states = vec![EvalState::empty()];
        for effect in program.effects.iter().skip(start) {
            let next = match self.apply_object_effect(states, effect, selector, context) {
                Ok(states) => states,
                Err(EvalFailure::Recursive(node)) => {
                    return self.object_projection_recursive(node);
                }
                Err(failure) => {
                    return self.object_projection_failure(failure.into_query_error());
                }
            };
            states = next;
        }

        let alternatives = states
            .into_iter()
            .map(|state| self.finish_object_alternative(state, selector))
            .collect::<Vec<_>>();
        (
            QueryResult::Value(SemanticQueryValue::ObjectProjection(
                evaluator_support::formula(alternatives),
            )),
            self.project_generation_signature(),
        )
            .into()
    }

    fn object_projection_failure(&self, error: QueryError) -> QueryBuildOutput<SemanticQueryValue> {
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Error(error),
            self.project_generation_signature(),
        )
            .into();
        output.result_is_partial = true;
        output.cache_suppress = true;
        output
    }

    fn object_projection_recursive(
        &self,
        node: SemanticNodeId,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Recursive(node),
            self.project_generation_signature(),
        )
            .into();
        output.result_is_partial = true;
        output.cache_suppress = true;
        output
    }

    fn finish_object_alternative(
        &self,
        state: EvalState,
        selector: &ObjectProjectionSelector,
    ) -> crate::semantic_query::ObjectProjectionAlternative {
        let mut positive = state
            .members
            .into_iter()
            .filter(|member| {
                !matches!(
                    &member.facets,
                    ProjectionEvidence::Proven(facets) if !facets.visibility().is_public()
                )
            })
            .filter(|member| selector_keeps_member(selector, &member.key))
            .map(|member| {
                evaluator_support::positive_key(
                    member.key,
                    member.presence,
                    member.value,
                    member.facets,
                )
            })
            .collect::<Vec<_>>();
        positive.sort_unstable_by(|left, right| left.key().cmp(right.key()));

        let indices = match selector {
            ObjectProjectionSelector::IndexDomain(domain)
            | ObjectProjectionSelector::EnumerableValueEnvelope(domain) => state
                .indices
                .into_iter()
                .filter(|index| index.domain() == *domain)
                .collect(),
            ObjectProjectionSelector::Surface | ObjectProjectionSelector::RelationShape(_) => {
                state.indices
            }
            _ => Vec::new(),
        };
        let signatures = match selector {
            ObjectProjectionSelector::Signature(kind) => state
                .signatures
                .into_iter()
                .filter(|signature| signature.kind() == *kind)
                .collect(),
            ObjectProjectionSelector::Surface | ObjectProjectionSelector::RelationShape(_) => {
                state.signatures
            }
            _ => Vec::new(),
        };
        let excess = if state.generic_spread_seen {
            ExcessEligibility::SuppressedByGenericSpread
        } else if state.unclassified_spread_seen {
            ExcessEligibility::Indeterminate
        } else {
            ExcessEligibility::Eligible {
                direct_candidates: Arc::from(state.direct_excess_candidates),
            }
        };
        evaluator_support::alternative(evaluator_support::AlternativeInput {
            positive: Arc::from(positive),
            selector: selector.clone(),
            closed: state.residual_operands.is_empty(),
            residual_operands: Arc::from(state.residual_operands),
            indeterminate_possible_writes: Arc::from(state.possible_writes),
            indices: Arc::from(indices),
            signatures: Arc::from(signatures),
            excess,
        })
    }

    fn apply_object_effect(
        &self,
        states: Vec<EvalState>,
        effect: &ObjectConstructionEffect,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) -> Result<Vec<EvalState>, EvalFailure> {
        match effect {
            ObjectConstructionEffect::Spread(operand) => {
                self.apply_spread_operand(states, *operand, context, 0)
            }
            ObjectConstructionEffect::DirectProperty(effect) => Ok(states
                .into_iter()
                .map(|mut state| {
                    self.apply_direct_property(&mut state, effect, selector, context);
                    state
                })
                .collect()),
            ObjectConstructionEffect::DirectMethod(effect) => Ok(states
                .into_iter()
                .map(|mut state| {
                    self.apply_direct_method(&mut state, effect, selector, context);
                    state
                })
                .collect()),
            ObjectConstructionEffect::DirectGet(effect) => Ok(states
                .into_iter()
                .map(|mut state| {
                    self.apply_direct_accessor(&mut state, effect, true, selector, context);
                    state
                })
                .collect()),
            ObjectConstructionEffect::DirectSet(effect) => Ok(states
                .into_iter()
                .map(|mut state| {
                    self.apply_direct_accessor(&mut state, effect, false, selector, context);
                    state
                })
                .collect()),
            ObjectConstructionEffect::DirectIndex(effect) => {
                let domains = self.index_domains(effect.key_type);
                Ok(states
                    .into_iter()
                    .map(|mut state| {
                        for domain in &domains {
                            replace_index(
                                &mut state.indices,
                                evaluator_support::index(
                                    *domain,
                                    effect.key_type,
                                    ProjectionEvidence::Proven(effect.value_type),
                                    ProjectionEvidence::Proven(effect.readonly),
                                    effect.spans,
                                    effect.declaration_origin.clone(),
                                ),
                            );
                        }
                        state
                    })
                    .collect())
            }
            ObjectConstructionEffect::DirectCall(node) => Ok(states
                .into_iter()
                .map(|mut state| {
                    state.signatures.push(evaluator_support::signature(
                        ObjectSignatureKind::Call,
                        *node,
                    ));
                    state
                })
                .collect()),
            ObjectConstructionEffect::DirectConstruct(node) => Ok(states
                .into_iter()
                .map(|mut state| {
                    state.signatures.push(evaluator_support::signature(
                        ObjectSignatureKind::Construct,
                        *node,
                    ));
                    state
                })
                .collect()),
        }
    }

    fn apply_direct_property(
        &self,
        state: &mut EvalState,
        effect: &AuthoredPropertyEffect,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) {
        let Some(key) = self.resolve_authored_key(&effect.key) else {
            self.apply_unknown_direct_key(state, effect.value, selector);
            return;
        };
        state
            .accessor_getters
            .retain(|candidate| !candidate.element_access_collides(&key));
        let facets = evaluator_support::member_facets(
            effect.readonly,
            None,
            false,
            effect.visibility,
            effect.spans,
            effect.declaration_origin.clone(),
            effect.declared_in_macro_type_arg,
            effect.merge_role,
            effect.excess_origin,
        );
        self.apply_named_write(
            state,
            key.clone(),
            effect.value,
            effect.optional,
            facets,
            false,
            context.optional_property_policy(),
        );
        state
            .direct_excess_candidates
            .retain(|candidate| !candidate.element_access_collides(&key));
        if effect.excess_origin == ExcessPropertyOrigin::FreshOwn {
            EvalState::remember_unique_key(&mut state.direct_excess_candidates, key);
        }
    }

    fn apply_direct_method(
        &self,
        state: &mut EvalState,
        effect: &AuthoredMethodEffect,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) {
        let Some(key) = self.resolve_authored_key(&effect.key) else {
            self.apply_unknown_direct_key(state, effect.signature, selector);
            return;
        };
        state
            .accessor_getters
            .retain(|candidate| !candidate.element_access_collides(&key));
        let facets = evaluator_support::member_facets(
            false,
            Some(ObjectMethodKind::Method),
            effect.has_implementation_body,
            effect.visibility,
            effect.spans,
            effect.declaration_origin.clone(),
            effect.declared_in_macro_type_arg,
            effect.merge_role,
            effect.excess_origin,
        );
        self.apply_named_write(
            state,
            key.clone(),
            effect.signature,
            effect.optional,
            facets,
            false,
            context.optional_property_policy(),
        );
        state
            .direct_excess_candidates
            .retain(|candidate| !candidate.element_access_collides(&key));
        if effect.excess_origin == ExcessPropertyOrigin::FreshOwn {
            EvalState::remember_unique_key(&mut state.direct_excess_candidates, key);
        }
    }

    fn apply_direct_accessor(
        &self,
        state: &mut EvalState,
        effect: &AuthoredAccessorEffect,
        getter: bool,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) {
        let value = self.accessor_value(effect.signature, getter);
        let Some(key) = self.resolve_authored_key(&effect.key) else {
            self.apply_unknown_direct_key(state, value, selector);
            return;
        };
        let paired_getter = !getter
            && state
                .accessor_getters
                .iter()
                .any(|candidate| candidate.element_access_collides(&key));
        let facets = evaluator_support::member_facets(
            false,
            None,
            effect.has_implementation_body,
            effect.visibility,
            effect.spans,
            effect.declaration_origin.clone(),
            effect.declared_in_macro_type_arg,
            effect.merge_role,
            effect.excess_origin,
        );
        if !paired_getter {
            self.apply_named_write(
                state,
                key.clone(),
                value,
                effect.optional,
                facets,
                false,
                context.optional_property_policy(),
            );
        } else if let Some(index) = state.member_position(&key) {
            state.members[index].facets = ProjectionEvidence::Proven(facets);
        }
        if getter {
            EvalState::remember_unique_key(&mut state.accessor_getters, key.clone());
        } else {
            state
                .accessor_getters
                .retain(|candidate| !candidate.element_access_collides(&key));
        }
        state
            .direct_excess_candidates
            .retain(|candidate| !candidate.element_access_collides(&key));
        if effect.excess_origin == ExcessPropertyOrigin::FreshOwn {
            // Candidates track the FACT key spelling: a paired setter
            // folds facets into the getter-spelled fact in place, so the
            // candidacy key is the fact's (getter) spelling — the excess
            // gate matches candidates against fact keys.
            let candidate_key = if paired_getter {
                state
                    .member_position(&key)
                    .map(|index| state.members[index].key.clone())
                    .unwrap_or_else(|| key.clone())
            } else {
                key.clone()
            };
            EvalState::remember_unique_key(&mut state.direct_excess_candidates, candidate_key);
        }
    }

    fn accessor_value(&self, signature: SemanticNodeId, getter: bool) -> SemanticNodeId {
        match self.graph().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) => {
                if getter {
                    *return_type
                } else {
                    params.first().map_or(*return_type, |param| param.ty)
                }
            }
            _ => signature,
        }
    }

    fn apply_unknown_direct_key(
        &self,
        state: &mut EvalState,
        child: SemanticNodeId,
        selector: &ObjectProjectionSelector,
    ) {
        state.residual_operands.push(child);
        match selector {
            ObjectProjectionSelector::Key(key) => {
                EvalState::remember_unique(&mut state.possible_writes, key.clone());
            }
            ObjectProjectionSelector::RelationShape(keys) => {
                for key in keys.iter() {
                    EvalState::remember_unique(&mut state.possible_writes, key.clone());
                }
            }
            _ => {}
        }
        taint_named_members(state);
    }

    fn apply_named_write(
        &self,
        state: &mut EvalState,
        key: PropertyKey,
        value: SemanticNodeId,
        optional: bool,
        facets: MemberFacets,
        copied: bool,
        policy: ExactOptionalPropertyPolicy,
    ) {
        let value = if optional {
            self.optional_present_value(value, policy)
        } else {
            value
        };
        let incoming_value = ProjectionEvidence::Proven(value);
        let incoming_facets = ProjectionEvidence::Proven(if copied {
            copied_member_facets(&facets)
        } else {
            facets
        });
        let presence = if optional {
            PositiveKeyPresence::Optional
        } else {
            PositiveKeyPresence::Required
        };
        match state.member_position(&key) {
            None => state.members.push(MemberState {
                key,
                presence,
                value: incoming_value,
                facets: incoming_facets,
            }),
            Some(index) if !optional => {
                state.members[index] = MemberState {
                    key,
                    presence,
                    value: incoming_value,
                    facets: incoming_facets,
                };
            }
            Some(index) => {
                let left = state.members[index].clone();
                let merged_value = match (left.value, incoming_value) {
                    (ProjectionEvidence::Proven(left), ProjectionEvidence::Proven(right)) => {
                        ProjectionEvidence::Proven(self.structural_union(left, right))
                    }
                    _ => ProjectionEvidence::Indeterminate,
                };
                state.members[index] = MemberState {
                    key,
                    presence: if left.presence == PositiveKeyPresence::Required {
                        PositiveKeyPresence::Required
                    } else {
                        PositiveKeyPresence::Optional
                    },
                    value: merged_value,
                    facets: if left.facets == incoming_facets {
                        incoming_facets
                    } else {
                        ProjectionEvidence::Indeterminate
                    },
                };
            }
        }
    }

    fn optional_present_value(
        &self,
        value: SemanticNodeId,
        policy: ExactOptionalPropertyPolicy,
    ) -> SemanticNodeId {
        if policy == ExactOptionalPropertyPolicy::Enabled {
            return value;
        }
        let Some(SemanticNodeData::Union(arms)) = self.graph().node_data(value).as_deref().cloned()
        else {
            return value;
        };
        let kept = arms
            .iter()
            .copied()
            .filter(|arm| {
                !matches!(
                    self.graph().node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
                )
            })
            .collect::<Vec<_>>();
        match kept.as_slice() {
            [] => value,
            [only] => *only,
            _ => self
                .graph()
                .intern_node(SemanticNodeData::Union(Arc::from(kept))),
        }
    }

    fn structural_union(&self, left: SemanticNodeId, right: SemanticNodeId) -> SemanticNodeId {
        if left == right {
            return left;
        }
        let mut arms = Vec::new();
        for node in [left, right] {
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Union(nested)) => {
                    for arm in nested.iter().copied() {
                        if !arms.contains(&arm) {
                            arms.push(arm);
                        }
                    }
                }
                _ if !arms.contains(&node) => arms.push(node),
                _ => {}
            }
        }
        match arms.as_slice() {
            [only] => *only,
            _ => self
                .graph()
                .intern_node(SemanticNodeData::Union(Arc::from(arms))),
        }
    }

    /// Normalize a spread operand before classification: transparent aliases
    /// and identity carriers (`DeclRef` / `InstantiationRef` / placeholder /
    /// bare `typeof` references) resolve to their concrete declaration
    /// surface, so a named interface or alias spreads its real closed
    /// surface instead of staying an open residual. Unresolvable carriers
    /// stay as-is and classify as open residuals downstream.
    fn normalize_spread_operand_node(&self, operand: SemanticNodeId) -> SemanticNodeId {
        let mut current = operand;
        let mut seen = rustc_hash::FxHashSet::default();
        while seen.insert(current) {
            match self.graph().node_data(current).as_deref() {
                Some(
                    SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
                ) => match self.unwrap_identity_carrier_for_relation(current) {
                    super::relation::IdentityCarrierUnwrap::Concrete(next) if next != current => {
                        current = next;
                    }
                    _ => return current,
                },
                Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
                    let transit =
                        crate::semantic_query::ProjectionReductionContext::structural_transit();
                    let (resolved, _, _) =
                        self.resolve_carrier_subject_node_capturing_suppress(current, transit);
                    if resolved == current {
                        return current;
                    }
                    current = resolved;
                }
                _ => return current,
            }
        }
        current
    }

    fn apply_spread_operand(
        &self,
        states: Vec<EvalState>,
        operand: SemanticNodeId,
        context: ObjectSpreadProjectionContext,
        depth: usize,
    ) -> Result<Vec<EvalState>, EvalFailure> {
        if depth > SPREAD_DEPTH_CAP {
            return Err(EvalFailure::DepthLimit { depth });
        }
        let operand = self.normalize_spread_operand_node(operand);
        let Some(data) = self.graph().node_data(operand) else {
            return Err(EvalFailure::Operational(QueryError::Miss));
        };
        match data.as_ref() {
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let product = states.len().saturating_mul(arms.len());
                if arms.len() > DISTRIBUTION_CAP || product > DISTRIBUTION_CAP {
                    return Err(EvalFailure::DistributionLimit {
                        actual: product.max(arms.len()),
                    });
                }
                let mut distributed = Vec::with_capacity(product);
                for state in states {
                    for arm in arms.iter().copied() {
                        distributed.extend(self.apply_spread_operand(
                            vec![state.clone()],
                            arm,
                            context,
                            depth + 1,
                        )?);
                    }
                }
                Ok(distributed)
            }
            SemanticNodeData::Object(surface) => {
                let surface = surface.clone();
                drop(data);
                let alternative = self.surface_spread_alternative(&surface);
                Ok(states
                    .into_iter()
                    .map(|mut state| {
                        self.merge_spread_alternative(
                            &mut state,
                            &alternative,
                            context.optional_property_policy(),
                        );
                        state
                    })
                    .collect())
            }
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                let nested =
                    self.execute_via_cold_build_helper(SemanticQueryKey::ProjectObjectSpread {
                        program: operand,
                        selector: ObjectProjectionSelector::Surface,
                        context,
                    });
                let formula = match nested.value {
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)) => formula,
                    QueryResult::Value(_) => {
                        return Err(EvalFailure::Operational(QueryError::Miss))
                    }
                    QueryResult::Error(error) => return Err(EvalFailure::Operational(error)),
                    QueryResult::Recursive(node) => return Err(EvalFailure::Recursive(node)),
                };
                self.merge_nested_formula(states, operand, &formula, context)
            }
            SemanticNodeData::Primitive(
                PrimitiveKind::Never | PrimitiveKind::Null | PrimitiveKind::Undefined,
            ) => Ok(states),
            data if right_spreads_to_nothing(data) => Ok(states),
            SemanticNodeData::Opaque(
                error @ (QueryError::RecursiveRef { .. }
                | QueryError::AliasCycle { .. }
                | QueryError::Cancelled
                | QueryError::BudgetExceeded(_)),
            ) => Err(EvalFailure::Operational(error.clone())),
            other => {
                let class = if matches!(other, SemanticNodeData::TypeParam { .. }) {
                    ResidualExcessClass::Generic
                } else {
                    ResidualExcessClass::Unclassified
                };
                drop(data);
                Ok(states
                    .into_iter()
                    .map(|mut state| {
                        retain_semantic_residual(&mut state, operand, class);
                        state
                    })
                    .collect())
            }
        }
    }

    fn merge_nested_formula(
        &self,
        states: Vec<EvalState>,
        operand: SemanticNodeId,
        formula: &ObjectProjectionFormula,
        context: ObjectSpreadProjectionContext,
    ) -> Result<Vec<EvalState>, EvalFailure> {
        let product = states.len().saturating_mul(formula.alternatives().len());
        if product > DISTRIBUTION_CAP {
            return Err(EvalFailure::DistributionLimit { actual: product });
        }
        let mut merged = Vec::with_capacity(product);
        for state in states {
            for alternative in formula.alternatives() {
                let mut spread = SpreadAlternative {
                    members: Vec::new(),
                    indices: alternative.indices().to_vec(),
                    residual: alternative.closed().is_none(),
                };
                alternative.positive().visit(|fact| {
                    spread.members.push(MemberState {
                        key: fact.key().clone(),
                        presence: fact.presence(),
                        value: fact.value().clone(),
                        facets: fact.facets().clone(),
                    });
                });
                let mut branch = state.clone();
                self.merge_spread_alternative(
                    &mut branch,
                    &spread,
                    context.optional_property_policy(),
                );
                if spread.residual {
                    // The nested program already classified its own residual:
                    // propagate that classification instead of re-guessing.
                    // `merge_spread_alternative` already tainted the
                    // pre-existing state before writing the nested proven
                    // facts; do not re-taint them.
                    let class = match alternative.excess() {
                        ExcessEligibility::SuppressedByGenericSpread => {
                            ResidualExcessClass::Generic
                        }
                        ExcessEligibility::Indeterminate => ResidualExcessClass::Unclassified,
                        ExcessEligibility::Eligible { .. } => ResidualExcessClass::Neutral,
                    };
                    retain_residual_bookkeeping(&mut branch, operand, class);
                }
                merged.push(branch);
            }
        }
        Ok(merged)
    }

    fn surface_spread_alternative(
        &self,
        surface: &crate::semantic_query::SurfaceView,
    ) -> SpreadAlternative {
        let members = surface
            .positive_members()
            .iter()
            .filter(|member| member.visibility.is_public())
            .filter_map(|member| {
                let key = member.key.cloned_known()?;
                Some(MemberState {
                    key,
                    presence: if member.optional {
                        PositiveKeyPresence::Optional
                    } else {
                        PositiveKeyPresence::Required
                    },
                    value: ProjectionEvidence::Proven(member.value),
                    facets: ProjectionEvidence::Proven(member_facets_from_surface(member)),
                })
            })
            .collect();
        let mut indices = Vec::new();
        for index in surface.index_signatures.iter() {
            for domain in self.index_domains(index.key_type) {
                indices.push(evaluator_support::index(
                    domain,
                    index.key_type,
                    ProjectionEvidence::Proven(index.value_type),
                    ProjectionEvidence::Proven(false),
                    index.spans,
                    index.declaration_origin.clone(),
                ));
            }
        }
        SpreadAlternative {
            members,
            indices,
            // A closed `Object` surface contributes no residual openness;
            // only a nested program's own residual propagates (see
            // `merge_nested_formula`).
            residual: false,
        }
    }

    fn merge_spread_alternative(
        &self,
        state: &mut EvalState,
        alternative: &SpreadAlternative,
        policy: ExactOptionalPropertyPolicy,
    ) {
        if alternative.residual {
            taint_named_members(state);
            taint_indices(state);
        }
        for member in &alternative.members {
            if member.presence == PositiveKeyPresence::Required {
                state
                    .direct_excess_candidates
                    .retain(|candidate| !candidate.element_access_collides(&member.key));
            }
            let facets = match &member.facets {
                ProjectionEvidence::Proven(facets) => facets.clone(),
                ProjectionEvidence::Indeterminate => evaluator_support::member_facets(
                    false,
                    None,
                    false,
                    MemberVisibility::Public,
                    verter_type_expr::MemberSpans::default(),
                    None,
                    crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    ExcessPropertyOrigin::SpreadTainted,
                ),
            };
            match &member.value {
                ProjectionEvidence::Proven(value) => self.apply_named_write(
                    state,
                    member.key.clone(),
                    *value,
                    member.presence == PositiveKeyPresence::Optional,
                    facets,
                    true,
                    policy,
                ),
                ProjectionEvidence::Indeterminate => {
                    let position = state.member_position(&member.key);
                    if member.presence == PositiveKeyPresence::Required || position.is_none() {
                        let presence = member.presence;
                        let entry = MemberState {
                            key: member.key.clone(),
                            presence,
                            value: ProjectionEvidence::Indeterminate,
                            facets: ProjectionEvidence::Indeterminate,
                        };
                        if let Some(position) = position {
                            state.members[position] = entry;
                        } else {
                            state.members.push(entry);
                        }
                    } else if let Some(position) = position {
                        state.members[position].value = ProjectionEvidence::Indeterminate;
                        state.members[position].facets = ProjectionEvidence::Indeterminate;
                    }
                }
            }
            state
                .accessor_getters
                .retain(|candidate| !candidate.element_access_collides(&member.key));
        }
        for index in &alternative.indices {
            replace_index(
                &mut state.indices,
                evaluator_support::index(
                    index.domain(),
                    index.key_type(),
                    index.value().clone(),
                    ProjectionEvidence::Proven(false),
                    index.spans(),
                    index.declaration_origin().cloned(),
                ),
            );
        }
    }

    fn resolve_authored_key(&self, key: &AuthoredPropertyKey) -> Option<PropertyKey> {
        if let Some(known) = key.cloned_known() {
            return Some(known);
        }
        let AuthoredPropertyKey::Computed(node) = key else {
            return None;
        };
        match self.graph().node_data(*node).as_deref() {
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(value))) => {
                Some(PropertyKey::string_literal(value.as_str()))
            }
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(value))) => {
                Some(PropertyKey::from_js_number(*value))
            }
            _ => None,
        }
    }

    fn index_domains(&self, key_type: SemanticNodeId) -> Vec<IndexDomain> {
        match self.graph().node_data(key_type).as_deref() {
            Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => {
                vec![IndexDomain::String]
            }
            Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => {
                vec![IndexDomain::Number]
            }
            Some(SemanticNodeData::Primitive(PrimitiveKind::Symbol)) => {
                vec![IndexDomain::Symbol]
            }
            Some(SemanticNodeData::Union(arms)) => arms
                .iter()
                .flat_map(|arm| self.index_domains(*arm))
                .fold(Vec::new(), |mut domains, domain| {
                    EvalState::remember_unique(&mut domains, domain);
                    domains
                }),
            _ => Vec::new(),
        }
    }
}

fn selector_live_start(
    program: &ObjectSpreadProgram,
    selector: &ObjectProjectionSelector,
) -> usize {
    let ObjectProjectionSelector::Key(selected) = selector else {
        return 0;
    };
    program
        .effects
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, effect)| {
            let (key, optional) = match effect {
                ObjectConstructionEffect::DirectProperty(effect) => {
                    (effect.key.cloned_known(), effect.optional)
                }
                ObjectConstructionEffect::DirectMethod(effect) => {
                    (effect.key.cloned_known(), effect.optional)
                }
                ObjectConstructionEffect::DirectGet(effect)
                | ObjectConstructionEffect::DirectSet(effect) => {
                    (effect.key.cloned_known(), effect.optional)
                }
                _ => return None,
            };
            if optional
                || !key
                    .as_ref()
                    .is_some_and(|key| key.element_access_collides(selected))
            {
                return None;
            }
            // A get/set pair is ONE write group for liveness: when the last
            // effect for the key is a setter with a preceding getter, the
            // read value comes from the getter — pruning must not drop it.
            if matches!(effect, ObjectConstructionEffect::DirectSet(_)) {
                let getter_index = program.effects[..index].iter().rposition(|prior| {
                    matches!(
                        prior,
                        ObjectConstructionEffect::DirectGet(prior_get)
                            if prior_get.key.cloned_known().as_ref().is_some_and(|key| key.element_access_collides(selected))
                    )
                });
                if let Some(getter_index) = getter_index {
                    return Some(getter_index);
                }
            }
            Some(index)
        })
        .unwrap_or(0)
}

fn selector_keeps_member(selector: &ObjectProjectionSelector, key: &PropertyKey) -> bool {
    match selector {
        // JS property identity: a selector for either spelling keeps the
        // colliding fact (`Key(Number(1))` keeps the `"1"` write).
        ObjectProjectionSelector::Key(selected) => selected.element_access_collides(key),
        ObjectProjectionSelector::RelationShape(keys) => keys
            .iter()
            .any(|selected| selected.element_access_collides(key)),
        ObjectProjectionSelector::Surface => true,
        ObjectProjectionSelector::IndexDomain(_)
        | ObjectProjectionSelector::Signature(_)
        | ObjectProjectionSelector::EnumerableValueEnvelope(_)
        | ObjectProjectionSelector::ExcessEligibility => false,
    }
}

fn member_facets_from_surface(member: &SurfaceMember) -> MemberFacets {
    evaluator_support::member_facets(
        member.readonly,
        member.method_kind,
        member.has_implementation_body,
        member.visibility,
        member.spans,
        member.declaration_origin.clone(),
        member.declared_in_macro_type_arg,
        member.merge_role,
        member.excess_origin,
    )
}

fn copied_member_facets(facets: &MemberFacets) -> MemberFacets {
    evaluator_support::member_facets(
        false,
        None,
        false,
        MemberVisibility::Public,
        facets.spans(),
        facets.declaration_origin().cloned(),
        facets.declared_in_macro_type_arg(),
        facets.merge_role(),
        ExcessPropertyOrigin::SpreadTainted,
    )
}

fn replace_index(indices: &mut Vec<ObjectProjectionIndex>, incoming: ObjectProjectionIndex) {
    if let Some(index) = indices
        .iter()
        .position(|index| index.domain() == incoming.domain())
    {
        indices[index] = incoming;
    } else {
        indices.push(incoming);
    }
}

fn retain_semantic_residual(
    state: &mut EvalState,
    operand: SemanticNodeId,
    class: ResidualExcessClass,
) {
    taint_named_members(state);
    taint_indices(state);
    retain_residual_bookkeeping(state, operand, class);
}

/// Residual bookkeeping WITHOUT the member/index taint. Used after a nested
/// program merge, where `merge_spread_alternative` already tainted the
/// pre-existing state before writing the nested alternative's proven facts —
/// re-tainting here would wipe the nested post-open exact writes.
fn retain_residual_bookkeeping(
    state: &mut EvalState,
    operand: SemanticNodeId,
    class: ResidualExcessClass,
) {
    EvalState::remember_unique(&mut state.residual_operands, operand);
    match class {
        ResidualExcessClass::Generic => state.generic_spread_seen = true,
        ResidualExcessClass::Unclassified => state.unclassified_spread_seen = true,
        ResidualExcessClass::Neutral => {}
    }
    state.accessor_getters.clear();
}

fn taint_named_members(state: &mut EvalState) {
    for member in &mut state.members {
        member.value = ProjectionEvidence::Indeterminate;
        member.facets = ProjectionEvidence::Indeterminate;
    }
}

fn taint_indices(state: &mut EvalState) {
    for index in &mut state.indices {
        *index = evaluator_support::index(
            index.domain(),
            index.key_type(),
            ProjectionEvidence::Indeterminate,
            ProjectionEvidence::Indeterminate,
            index.spans(),
            index.declaration_origin().cloned(),
        );
    }
}

fn right_spreads_to_nothing(data: &SemanticNodeData) -> bool {
    matches!(
        data,
        SemanticNodeData::Primitive(
            PrimitiveKind::Boolean
                | PrimitiveKind::Number
                | PrimitiveKind::BigInt
                | PrimitiveKind::String
                | PrimitiveKind::Object,
        ) | SemanticNodeData::Literal(_)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Signature { .. }
    )
}
