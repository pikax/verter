use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::{ExcessPropertyOrigin, MemberVisibility, ObjectMethodKind};

use super::object_spread_product::{
    MemberState, NarrowFailure, NestedProduct, PathStep, ProductLeaf, ProductNode,
    ResidualExcessClass, SpreadFragment, SpreadProduct,
};
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::semantic_query::checker_policy::{cross_product_union, OperationRefusal, ProductFactor};
use crate::semantic_query::object_spread_projection::{
    evaluator_support, note_collision_comparison, note_member_visit,
};
use crate::semantic_query::{
    AuthoredAccessorEffect, AuthoredMethodEffect, AuthoredPropertyEffect, AuthoredPropertyKey,
    CheckerDiagnosticOperation, ExactOptionalPropertyPolicy, ExcessEligibility, IndexDomain,
    MemberFacets, ObjectConstructionEffect, ObjectProjectionAlternative, ObjectProjectionFormula,
    ObjectProjectionIndex, ObjectProjectionSelector, ObjectSignatureKind, ObjectSpreadProgram,
    ObjectSpreadProjectionContext, PartialReasonSet, PositiveKeyPresence, PrimitiveKind,
    ProjectionEvidence, PropertyKey, QueryError, QueryResult, RecoveryBasis, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SemanticQueryValue, SurfaceMember,
};

/// Hash lookup uses JS property identity; the stored member keeps its authored
/// spelling for enumeration. Count actual hash-table equality comparisons.
struct CollisionKey(PropertyKey);

impl CollisionKey {
    fn new(key: &PropertyKey) -> Self {
        Self(match key {
            PropertyKey::Number(number) => {
                PropertyKey::String(verter_type_expr::OwnedStr::from(number.to_string()))
            }
            _ => key.clone(),
        })
    }
}

impl PartialEq for CollisionKey {
    fn eq(&self, other: &Self) -> bool {
        note_collision_comparison();
        self.0 == other.0
    }
}

impl Eq for CollisionKey {}

impl std::hash::Hash for CollisionKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0, state);
    }
}

/// The fold of ONE alternative: built from empty along that alternative's
/// path through the product, read into its alternative, and dropped. No
/// state is shared or copied between alternatives.
struct EvalState {
    members: Vec<MemberState>,
    member_positions: FxHashMap<CollisionKey, usize>,
    indices: Vec<ObjectProjectionIndex>,
    signatures: Vec<crate::semantic_query::ObjectProjectionSignature>,
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
            member_positions: FxHashMap::default(),
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
        self.member_positions.get(&CollisionKey::new(key)).copied()
    }

    fn push_member(&mut self, member: MemberState) {
        self.member_positions
            .insert(CollisionKey::new(&member.key), self.members.len());
        self.members.push(member);
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

pub(super) enum EvalFailure {
    /// An operand or nested query failed with its own typed reason. The
    /// reason is propagated unchanged so operational partials stay
    /// distinguishable from a genuine miss.
    Operational(QueryError),
    /// The connected demand could not pay for the construction.
    Exhausted(PartialReasonSet),
    /// A nested program projection re-entered an in-flight query.
    Recursive(SemanticNodeId),
}

impl From<NarrowFailure> for EvalFailure {
    fn from(failure: NarrowFailure) -> Self {
        match failure {
            NarrowFailure::Query(error) => Self::Operational(error),
            NarrowFailure::Recursive(node) => Self::Recursive(node),
            NarrowFailure::Exhausted(reasons) => Self::Exhausted(reasons),
        }
    }
}

pub(super) struct PropertySpreadChain {
    pub(super) base: SemanticNodeId,
    /// The chain's programs, outermost first.
    pub(super) programs: Vec<SemanticNodeId>,
    /// How many tail writes the programs hold together.
    writes: usize,
}

/// What a selector reads of each spread fragment: the facts kept, or
/// `None` when it reads the whole construction.
#[derive(Clone, Copy)]
enum FragmentReads<'s> {
    Keys(&'s [PropertyKey], bool),
    Indices(Option<IndexDomain>),
}

impl FragmentReads<'_> {
    fn of(selector: &ObjectProjectionSelector) -> Option<FragmentReads<'_>> {
        match selector {
            ObjectProjectionSelector::Surface | ObjectProjectionSelector::ExcessEligibility => None,
            ObjectProjectionSelector::Key(key) => {
                Some(FragmentReads::Keys(std::slice::from_ref(key), false))
            }
            ObjectProjectionSelector::RelationShape(keys) => Some(FragmentReads::Keys(keys, true)),
            ObjectProjectionSelector::IndexDomain(domain)
            | ObjectProjectionSelector::EnumerableValueEnvelope(domain) => {
                Some(FragmentReads::Indices(Some(*domain)))
            }
            ObjectProjectionSelector::Signature(_) => Some(FragmentReads::Indices(None)),
        }
    }

    /// The part of `fragment` the selector can observe. Openness always
    /// survives: it taints every earlier fact, read or not.
    fn narrow(self, fragment: &SpreadFragment) -> SpreadFragment {
        let (members, indices): (Vec<MemberState>, Vec<ObjectProjectionIndex>) = match self {
            FragmentReads::Keys(keys, keep_indices) => (
                fragment
                    .members
                    .iter()
                    .filter(|member| {
                        keys.iter()
                            .any(|selected| selected.element_access_collides(&member.key))
                    })
                    .cloned()
                    .collect(),
                if keep_indices {
                    fragment.indices.to_vec()
                } else {
                    Vec::new()
                },
            ),
            FragmentReads::Indices(domain) => (
                Vec::new(),
                fragment
                    .indices
                    .iter()
                    .filter(|index| domain == Some(index.domain()))
                    .cloned()
                    .collect(),
            ),
        };
        SpreadFragment {
            members: Arc::from(members),
            indices: Arc::from(indices),
            residual: fragment.residual,
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    construction_ordinal_seam! {
        fn project_object_spread_for_consumer(
            &self,
            program: SemanticNodeId,
            selector: ObjectProjectionSelector,
            projection_reduction: crate::semantic_query::ProjectionReductionContext,
        ) -> QueryResult<ObjectProjectionFormula> {
            let canonical = self
                .graph()
                .node_scope(program)
                .and_then(|scope| scope.canonical_file())
                .unwrap_or_else(|| verter_type_expr::OwnedStr::from(""));
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
    }

    /// Build the factored product of `program_id` for `selector`.
    ///
    /// Two stages. The first is selector-independent: every effect's layer
    /// is built over the WHOLE program, and the product's size is checked
    /// against the checker's cross-product limit before any selector prunes
    /// an effect — a construction too complex to build is too complex
    /// whichever of its facts a consumer reads. The second keeps the
    /// selector's live suffix and narrows each fragment to the facts the
    /// selector reads, so arms that differ only in unread facts collapse
    /// into one: a selected-key read never multiplies by unrelated arms.
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
        let program = program.share();
        drop(data);

        // Pay for the per-effect layers before allocating them.
        let layer_bytes = program.effects.len().saturating_mul(
            2 * std::mem::size_of::<ProductNode>()
                + std::mem::size_of::<ProductFactor>()
                + std::mem::size_of::<ProductLeaf>(),
        );
        if let Err(EvalFailure::Exhausted(reasons)) =
            self.charge_spread_construction(program.effects.len(), layer_bytes)
        {
            return self.object_projection_exhausted(reasons);
        }

        let start = selector_live_start(&program, selector);
        let mut factors = Vec::with_capacity(program.effects.len());
        let mut live = Vec::with_capacity(program.effects.len() - start);
        for (index, effect) in program.effects.iter().enumerate() {
            let layer = match effect {
                ObjectConstructionEffect::Spread(operand) => {
                    match self.spread_operand_product(*operand, context, true) {
                        Ok(layer) => layer,
                        Err(EvalFailure::Exhausted(reasons)) => {
                            return self.object_projection_exhausted(reasons);
                        }
                        // A construction refusal or resource failure is
                        // selector-independent and never shadowed.
                        Err(EvalFailure::Operational(error)) if is_construction_refusal(&error) => {
                            return self.object_projection_failure(error);
                        }
                        // A shadowed operand that cannot be classified
                        // contributes nothing the selector reads, and no
                        // factor the product can count.
                        Err(_) if index < start => continue,
                        Err(EvalFailure::Recursive(node)) => {
                            return self.object_projection_recursive(node);
                        }
                        Err(EvalFailure::Operational(error)) => {
                            return self.object_projection_failure(error);
                        }
                    }
                }
                _ => ProductNode::leaf(ProductLeaf::Direct {
                    program: program_id,
                    effect: index as u32,
                    closed: self.direct_effect_is_closed(effect),
                }),
            };
            factors.push(match layer.bounds().width {
                1 => ProductFactor::Single,
                width => ProductFactor::Union(usize::try_from(width).unwrap_or(usize::MAX)),
            });
            if index >= start {
                live.push(layer);
            }
        }
        if let Err(refusal) = cross_product_union(factors, CheckerDiagnosticOperation::ObjectSpread)
        {
            return self.object_projection_refused(refusal, program_id);
        }

        let mut root = ProductNode::overlay(live);
        if let Some(reads) = FragmentReads::of(selector) {
            root = match root.narrowed(
                &|fragment| reads.narrow(fragment),
                &mut |nested| self.narrowed_nested_product(nested, selector, context),
                &mut |units| {
                    self.charge_spread_construction(units, 0)
                        .map_err(|failure| match failure {
                            EvalFailure::Exhausted(reasons) => NarrowFailure::Exhausted(reasons),
                            _ => unreachable!("charging only exhausts"),
                        })
                },
            ) {
                Ok(root) => root,
                Err(NarrowFailure::Recursive(node)) => {
                    return self.object_projection_recursive(node)
                }
                Err(NarrowFailure::Query(error)) => return self.object_projection_failure(error),
                Err(NarrowFailure::Exhausted(reasons)) => {
                    return self.object_projection_exhausted(reasons)
                }
            };
        }
        let mut product =
            SpreadProduct::new(root, selector.clone(), context.optional_property_policy());
        if product.width() == 1 && !product.is_finished() {
            // A single alternative is folded once, here, so every read of
            // the kept formula reuses it instead of folding it again.
            if let Err(reasons) = self.charge_product_alternative(&product) {
                return self.object_projection_exhausted(reasons);
            }
            let alternative = self.product_alternative(&product, 0);
            product = SpreadProduct::new(
                ProductNode::Finished(Arc::new(alternative)),
                selector.clone(),
                context.optional_property_policy(),
            );
        }
        let formula = ObjectProjectionFormula::from_product(product);
        (
            QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)),
            self.project_generation_signature(),
        )
            .into()
    }

    /// A nested program read under the enclosing selector: its own
    /// narrowed product, so a selected-key read stays narrow all the way
    /// down.
    fn narrowed_nested_product(
        &self,
        nested: &NestedProduct,
        selector: &ObjectProjectionSelector,
        context: ObjectSpreadProjectionContext,
    ) -> Result<ProductNode, NarrowFailure> {
        let read = self.execute_via_cold_build_helper(SemanticQueryKey::ProjectObjectSpread {
            program: nested.operand,
            selector: selector.clone(),
            context,
        });
        match read.value {
            QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)) => {
                Ok(ProductNode::nested(nested.operand, formula))
            }
            QueryResult::Value(_) => Err(NarrowFailure::Query(QueryError::Miss)),
            QueryResult::Error(error) => Err(NarrowFailure::Query(error)),
            QueryResult::Recursive(node) => Err(NarrowFailure::Recursive(node)),
        }
    }

    fn object_projection_failure(&self, error: QueryError) -> QueryBuildOutput<SemanticQueryValue> {
        let mut output: QueryBuildOutput<SemanticQueryValue> = (
            QueryResult::Error(error),
            self.project_generation_signature(),
        )
            .into();
        output.mark_partial();
        output.cache_suppress = true;
        output
    }

    /// The connected demand could not pay for the construction: a typed
    /// partial of the demand's own trip, never a smaller product.
    fn object_projection_exhausted(
        &self,
        reasons: PartialReasonSet,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let (limit, actual, context) = self.connected_demand.limit_report(reasons);
        let mut output = self.object_projection_failure(QueryError::BudgetExceeded(
            verter_session_query::inputs::budget::BudgetExceededFailure {
                domain: verter_session_query::inputs::budget::BudgetDomain::ProjectionOperation,
                limit,
                actual,
                context: context.into(),
            },
        ));
        output.mark_partial_with(reasons);
        output
    }

    /// The construction's cross product reaches the checker's limit: its
    /// TS2590 recovery, a resource partial, with the program as the
    /// recovery's authored form. Nothing of the product is kept.
    fn object_projection_refused(
        &self,
        refusal: OperationRefusal,
        program: SemanticNodeId,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        self.fold_local_partial_completeness(PartialReasonSet::OPERATION_BUDGET);
        let mut output = self.object_projection_failure(QueryError::CheckerRecovery {
            diagnostic: refusal.diagnostic(),
            basis: RecoveryBasis::Budget,
            origin: Some(program),
        });
        output.mark_partial_with(PartialReasonSet::OPERATION_BUDGET);
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
        output.mark_partial();
        output.cache_suppress = true;
        output
    }

    /// Pay for `units` of construction work and `bytes` of what it builds
    /// before building it. Outside a connected demand nothing is charged.
    fn charge_spread_construction(&self, units: usize, bytes: usize) -> Result<(), EvalFailure> {
        if !self.connected_demand.is_active() {
            return Ok(());
        }
        self.connected_demand
            .charge_units(units)
            .and_then(|()| self.connected_demand.reserve_bytes(bytes))
            .map_err(|reasons| {
                self.fold_local_partial_completeness(reasons);
                EvalFailure::Exhausted(reasons)
            })
    }

    /// Read alternative `index` of `product`: fold the leaves on its path
    /// from an empty state and finish the fold for the product's selector.
    /// The caller has already paid for it.
    pub(super) fn product_alternative(
        &self,
        product: &SpreadProduct,
        index: u64,
    ) -> ObjectProjectionAlternative {
        let mut state = EvalState::empty();
        let mut finished = None;
        let walked = product.root.walk_path(index, &mut |step| {
            match step {
                PathStep::Leaf(leaf) => self.apply_product_leaf(&mut state, leaf, product),
                PathStep::Nested(nested, local) => {
                    let alternative = self.product_alternative(nested.formula.product(), local);
                    self.merge_nested_alternative(
                        &mut state,
                        nested.operand,
                        &alternative,
                        product.policy,
                    );
                }
                PathStep::Finished(alternative) => finished = Some(alternative.clone()),
            }
            Ok::<(), std::convert::Infallible>(())
        });
        let Ok(()) = walked;
        match finished {
            Some(alternative) => alternative,
            None => self.finish_object_alternative(state, &product.selector),
        }
    }

    fn apply_product_leaf(
        &self,
        state: &mut EvalState,
        leaf: &ProductLeaf,
        product: &SpreadProduct,
    ) {
        let selector = &product.selector;
        let policy = product.policy;
        match leaf {
            ProductLeaf::Direct {
                program, effect, ..
            } => {
                let Some(program) = self.spread_program(*program) else {
                    verter_debug_assert::verter_debug_assert!(
                        false,
                        "a direct leaf's program node is retained"
                    );
                    return;
                };
                if let Some(effect) = program.effects.get(*effect as usize) {
                    self.apply_direct_effect(state, effect, selector, policy);
                }
            }
            ProductLeaf::Spread(fragment) => self.merge_spread_fragment(state, fragment, policy),
            ProductLeaf::CopiedWrites { programs, .. } => {
                // The inner objects' own writes arrive as copied properties.
                // Each required public write overwrites independently of the
                // caller's prior state, so no intermediate surface is needed.
                for program in programs.iter().rev() {
                    let Some(program) = self.spread_program(*program) else {
                        verter_debug_assert::verter_debug_assert!(
                            false,
                            "a copied-writes program node is retained"
                        );
                        continue;
                    };
                    for effect in program.effects.iter().skip(1) {
                        let ObjectConstructionEffect::DirectProperty(effect) = effect else {
                            unreachable!("property_spread_chain validates every tail");
                        };
                        let key = effect.key.cloned_known().expect("validated known key");
                        state
                            .direct_excess_candidates
                            .retain(|candidate| !candidate.element_access_collides(&key));
                        state
                            .accessor_getters
                            .retain(|candidate| !candidate.element_access_collides(&key));
                        self.apply_named_write(
                            state,
                            key,
                            effect.value,
                            false,
                            authored_property_facets(effect),
                            true,
                            policy,
                        );
                    }
                }
            }
            ProductLeaf::Residual { operand, class } => {
                retain_semantic_residual(state, *operand, *class);
            }
        }
    }

    fn spread_program(&self, program: SemanticNodeId) -> Option<ObjectSpreadProgram> {
        match self.graph().node_data(program)?.as_ref() {
            SemanticNodeData::ObjectSpreadProgram(program) => Some(program.share()),
            _ => None,
        }
    }

    fn finish_object_alternative(
        &self,
        state: EvalState,
        selector: &ObjectProjectionSelector,
    ) -> ObjectProjectionAlternative {
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
        // A narrowed read folds only the facts its selector observes, so the
        // direct candidates other keys' copies would retire are not known:
        // excess eligibility is a whole-construction fact.
        let excess = if FragmentReads::of(selector).is_some() {
            ExcessEligibility::Indeterminate
        } else if state.generic_spread_seen {
            ExcessEligibility::SuppressedByGenericSpread
        } else if state.unclassified_spread_seen {
            ExcessEligibility::Indeterminate
        } else {
            ExcessEligibility::Eligible {
                direct_candidates: verter_type_expr::OwnedSlice::from(
                    state.direct_excess_candidates,
                ),
            }
        };
        evaluator_support::alternative(evaluator_support::AlternativeInput {
            positive: verter_type_expr::OwnedSlice::from(positive),
            selector: selector.clone(),
            closed: state.residual_operands.is_empty(),
            residual_operands: verter_type_expr::OwnedSlice::from(state.residual_operands),
            indeterminate_possible_writes: verter_type_expr::OwnedSlice::from(
                state.possible_writes,
            ),
            indices: verter_type_expr::OwnedSlice::from(indices),
            signatures: verter_type_expr::OwnedSlice::from(signatures),
            excess,
        })
    }

    fn apply_direct_effect(
        &self,
        state: &mut EvalState,
        effect: &ObjectConstructionEffect,
        selector: &ObjectProjectionSelector,
        policy: ExactOptionalPropertyPolicy,
    ) {
        match effect {
            ObjectConstructionEffect::Spread(_) => {
                unreachable!("a spread is its own product layer, never a direct leaf")
            }
            ObjectConstructionEffect::DirectProperty(effect) => {
                self.apply_direct_property(state, effect, selector, policy);
            }
            ObjectConstructionEffect::DirectMethod(effect) => {
                self.apply_direct_method(state, effect, selector, policy);
            }
            ObjectConstructionEffect::DirectGet(effect) => {
                self.apply_direct_accessor(state, effect, true, selector, policy);
            }
            ObjectConstructionEffect::DirectSet(effect) => {
                self.apply_direct_accessor(state, effect, false, selector, policy);
            }
            ObjectConstructionEffect::DirectIndex(effect) => {
                for domain in self.index_domains(effect.key_type) {
                    replace_index(
                        &mut state.indices,
                        evaluator_support::index(
                            domain,
                            effect.key_type,
                            ProjectionEvidence::Proven(effect.value_type),
                            ProjectionEvidence::Proven(effect.readonly),
                            effect.spans,
                            effect.declaration_origin.clone(),
                        ),
                    );
                }
            }
            ObjectConstructionEffect::DirectCall(node) => {
                state.signatures.push(evaluator_support::signature(
                    ObjectSignatureKind::Call,
                    *node,
                ));
            }
            ObjectConstructionEffect::DirectConstruct(node) => {
                state.signatures.push(evaluator_support::signature(
                    ObjectSignatureKind::Construct,
                    *node,
                ));
            }
        }
    }

    fn apply_direct_property(
        &self,
        state: &mut EvalState,
        effect: &AuthoredPropertyEffect,
        selector: &ObjectProjectionSelector,
        policy: ExactOptionalPropertyPolicy,
    ) {
        let Some(key) = self.resolve_authored_key(&effect.key) else {
            self.apply_unknown_direct_key(state, effect.value, selector);
            return;
        };
        state
            .accessor_getters
            .retain(|candidate| !candidate.element_access_collides(&key));
        let facets = authored_property_facets(effect);
        self.apply_named_write(
            state,
            key.clone(),
            effect.value,
            effect.optional,
            facets,
            false,
            policy,
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
        policy: ExactOptionalPropertyPolicy,
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
            policy,
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
        policy: ExactOptionalPropertyPolicy,
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
                policy,
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
        let signature = if getter {
            self.materialize_returned_callable(signature)
        } else {
            signature
        };
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
            Some(SemanticNodeData::DeferredCallable(callable)) if !getter => callable
                .parts(&crate::semantic_query::ResolveOverloadSetConsumer::witness())
                .params
                .first()
                .map_or_else(|| self.opaque(QueryError::Miss), |param| param.ty),
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
        note_member_visit();
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
            None => state.push_member(MemberState {
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
                let left = &state.members[index];
                let merged_value = match (&left.value, incoming_value) {
                    (ProjectionEvidence::Proven(left), ProjectionEvidence::Proven(right)) => {
                        ProjectionEvidence::Proven(self.structural_union(*left, right))
                    }
                    _ => ProjectionEvidence::Indeterminate,
                };
                let presence = if left.presence == PositiveKeyPresence::Required {
                    PositiveKeyPresence::Required
                } else {
                    PositiveKeyPresence::Optional
                };
                let facets = if left.facets == incoming_facets {
                    incoming_facets
                } else {
                    ProjectionEvidence::Indeterminate
                };
                state.members[index] = MemberState {
                    key,
                    presence,
                    value: merged_value,
                    facets,
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
        let Some(SemanticNodeData::Union(arms)) = self
            .graph()
            .node_data(value)
            .as_deref()
            .map(crate::semantic_query::SemanticNodeData::clone)
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
            // Canonical construction: the undefined-stripped present-value
            // union routes through the one authority.
            _ => self.intern_normalized_union_or_intersection(&kept, true),
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
        // Canonical construction: the spread-alternative structural union
        // routes through the one authority (flatten + structural dedup
        // replace the hand-rolled one-level splice).
        self.intern_normalized_union_or_intersection(&arms, true)
    }

    /// Normalize a spread operand before classification: transparent aliases
    /// and identity carriers (`DeclRef` / `InstantiationRef` / placeholder /
    /// bare `typeof` references) resolve to their concrete declaration
    /// surface, so a named interface or alias spreads its real closed
    /// surface instead of staying an open residual. Unresolvable carriers
    /// stay as-is and classify as open residuals downstream.
    pub(super) fn normalize_spread_operand_node(&self, operand: SemanticNodeId) -> SemanticNodeId {
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
                    if resolved.node == current
                        || matches!(
                            resolved.status,
                            crate::semantic_query::FactStatus::Unavailable(_)
                        )
                    {
                        return current;
                    }
                    current = resolved.node;
                }
                _ => return current,
            }
        }
        current
    }

    /// The product layer one spread operand contributes. `top` marks the
    /// object literal's own operand, which first merges a union of one
    /// object type and arms that spread into an empty object (the
    /// checker's `tryMergeUnionOfObjectTypeAndEmptyObject`): `{ ...o?.x }`
    /// copies `x`'s properties as optional ones, never a union of the copy
    /// and `{}`. Any other union is a choice over its arms.
    fn spread_operand_product(
        &self,
        operand: SemanticNodeId,
        context: ObjectSpreadProjectionContext,
        top: bool,
    ) -> Result<ProductNode, EvalFailure> {
        let operand = self.normalize_spread_operand_node(operand);
        let Some(data) = self.graph().node_data(operand) else {
            return Err(EvalFailure::Operational(QueryError::Miss));
        };
        match data.as_ref() {
            SemanticNodeData::Union(arms) => {
                let arms = arms.members_arc();
                drop(data);
                if top {
                    match self.merge_object_or_empty_union(&arms) {
                        Some(UnionSpreadMerge::Empty) => {
                            return Ok(ProductNode::overlay(Vec::new()))
                        }
                        Some(UnionSpreadMerge::Partial(surface)) => {
                            let mut fragment = self.surface_spread_fragment(&surface)?;
                            fragment.members = fragment
                                .members
                                .iter()
                                .cloned()
                                .map(|member| MemberState {
                                    presence: PositiveKeyPresence::Optional,
                                    ..member
                                })
                                .collect();
                            return Ok(ProductNode::leaf(ProductLeaf::Spread(fragment)));
                        }
                        None => {}
                    }
                }
                // One choice point per arm, paid before any arm is built.
                self.charge_spread_construction(
                    arms.len(),
                    arms.len()
                        .saturating_mul(std::mem::size_of::<ProductNode>() + 8),
                )?;
                let mut choice = Vec::with_capacity(arms.len());
                for arm in arms.iter().copied() {
                    choice.push(self.spread_operand_product(arm, context, false)?);
                }
                Ok(ProductNode::choice(choice))
            }
            SemanticNodeData::Object(surface) => {
                let surface = surface.clone();
                drop(data);
                Ok(ProductNode::leaf(ProductLeaf::Spread(
                    self.surface_spread_fragment(&surface)?,
                )))
            }
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                if let Some(chain) = self.property_spread_chain(operand)? {
                    // Each inner object starts its own spread operand as the
                    // object literal's own, as the general evaluator does.
                    let base = self.spread_operand_product(chain.base, context, true)?;
                    return Ok(ProductNode::overlay(vec![
                        base,
                        ProductNode::leaf(ProductLeaf::CopiedWrites {
                            programs: Arc::from(chain.programs),
                            writes: chain.writes,
                        }),
                    ]));
                }
                let nested =
                    self.execute_via_cold_build_helper(SemanticQueryKey::ProjectObjectSpread {
                        program: operand,
                        selector: ObjectProjectionSelector::Surface,
                        context,
                    });
                match nested.value {
                    QueryResult::Value(SemanticQueryValue::ObjectProjection(formula)) => {
                        Ok(ProductNode::nested(operand, formula))
                    }
                    QueryResult::Value(_) => Err(EvalFailure::Operational(QueryError::Miss)),
                    QueryResult::Error(error) => Err(EvalFailure::Operational(error)),
                    QueryResult::Recursive(node) => Err(EvalFailure::Recursive(node)),
                }
            }
            SemanticNodeData::Primitive(
                PrimitiveKind::Never | PrimitiveKind::Null | PrimitiveKind::Undefined,
            ) => Ok(ProductNode::overlay(Vec::new())),
            data if right_spreads_to_nothing(data) => Ok(ProductNode::overlay(Vec::new())),
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
                Ok(ProductNode::leaf(ProductLeaf::Residual { operand, class }))
            }
        }
    }

    /// Peel immutable property-only prefixes once, leaving general operands to
    /// the existing correlated evaluator. No prefix is projected or rebuilt.
    /// Optional writes, computed keys and non-public members need the complete
    /// inner-object fold before copying and cannot use this composition rule.
    pub(super) fn property_spread_chain(
        &self,
        mut operand: SemanticNodeId,
    ) -> Result<Option<PropertySpreadChain>, EvalFailure> {
        let mut programs = Vec::new();
        let mut writes = 0usize;
        let mut visited = FxHashSet::default();
        // bounded-loop: visited-set cycle stop returns an uncacheable recursion carrier.
        loop {
            if self.snapshot.flags().is_cancelled() {
                return Err(EvalFailure::Operational(QueryError::Cancelled));
            }
            operand = self.normalize_spread_operand_node(operand);
            let Some(data) = self.graph().node_data(operand) else {
                return Err(EvalFailure::Operational(QueryError::Miss));
            };
            let SemanticNodeData::ObjectSpreadProgram(program) = data.as_ref() else {
                break;
            };
            let [ObjectConstructionEffect::Spread(base), tail @ ..] = program.effects.as_ref()
            else {
                break;
            };
            // Pay for the link (its tail scan and both id lists) before
            // growing them: the chain is as long as the nesting is deep.
            self.charge_spread_construction(
                1 + tail.len(),
                2 * std::mem::size_of::<SemanticNodeId>(),
            )?;
            if !tail.iter().all(|effect| {
                matches!(effect, ObjectConstructionEffect::DirectProperty(property)
                    if !property.optional
                        && property.visibility.is_public()
                        && property.key.as_known().is_some())
            }) {
                break;
            }
            if !visited.insert(operand) {
                return Err(EvalFailure::Recursive(operand));
            }
            programs.push(operand);
            writes += tail.len();
            operand = *base;
        }
        Ok((!programs.is_empty()).then_some(PropertySpreadChain {
            base: operand,
            programs,
            writes,
        }))
    }

    /// Copy one alternative of a nested program onto `state`, as a spread
    /// of that object.
    fn merge_nested_alternative(
        &self,
        state: &mut EvalState,
        operand: SemanticNodeId,
        alternative: &ObjectProjectionAlternative,
        policy: ExactOptionalPropertyPolicy,
    ) {
        let mut members = Vec::new();
        alternative.positive().visit(|fact| {
            note_member_visit();
            members.push(MemberState {
                key: fact.key().clone(),
                presence: fact.presence(),
                value: fact.value().clone(),
                facets: fact.facets().clone(),
            });
        });
        let fragment = SpreadFragment {
            members: Arc::from(members),
            indices: Arc::from(alternative.indices()),
            residual: alternative.closed().is_none(),
        };
        self.merge_spread_fragment(state, &fragment, policy);
        if fragment.residual {
            // The nested program already classified its own residual:
            // propagate that classification instead of re-guessing.
            // `merge_spread_fragment` already tainted the pre-existing
            // state before writing the nested proven facts; do not
            // re-taint them.
            let class = match alternative.excess() {
                ExcessEligibility::SuppressedByGenericSpread => ResidualExcessClass::Generic,
                ExcessEligibility::Indeterminate => ResidualExcessClass::Unclassified,
                ExcessEligibility::Eligible { .. } => ResidualExcessClass::Neutral,
            };
            retain_residual_bookkeeping(state, operand, class);
        }
    }

    /// The checker's `tryMergeUnionOfObjectTypeAndEmptyObject` over a spread
    /// operand's union `arms`: every arm spreading into an empty object
    /// merges to nothing; exactly one other arm, an object surface, merges
    /// to its anonymous partial (every property optional). `None` for any
    /// other union, which distributes.
    fn merge_object_or_empty_union(&self, arms: &[SemanticNodeId]) -> Option<UnionSpreadMerge> {
        let mut object = None;
        for arm in arms.iter().copied() {
            let arm = self.normalize_spread_operand_node(arm);
            let data = self.graph().node_data(arm)?;
            match data.as_ref() {
                data if spreads_into_empty_object(data) => {}
                SemanticNodeData::Object(surface) if object.is_none() => {
                    object = Some(surface.clone());
                }
                _ => return None,
            }
        }
        Some(match object {
            None => UnionSpreadMerge::Empty,
            Some(surface) => UnionSpreadMerge::Partial(surface),
        })
    }

    /// The copied facts of a closed object surface, its bytes reserved
    /// before they are built.
    fn surface_spread_fragment(
        &self,
        surface: &crate::semantic_query::SurfaceView,
    ) -> Result<SpreadFragment, EvalFailure> {
        let facts = surface.positive_members().len() + surface.index_signatures.len();
        self.charge_spread_construction(
            1 + facts,
            facts.saturating_mul(std::mem::size_of::<MemberState>()),
        )?;
        let members = surface
            .positive_members()
            .iter()
            .filter(|member| member.visibility.is_public())
            .filter_map(|member| {
                note_member_visit();
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
            .collect::<Vec<_>>();
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
        Ok(SpreadFragment {
            members: Arc::from(members),
            indices: Arc::from(indices),
            // A closed `Object` surface contributes no residual openness;
            // only a nested program's own residual propagates (see
            // `merge_nested_alternative`).
            residual: false,
        })
    }

    fn merge_spread_fragment(
        &self,
        state: &mut EvalState,
        alternative: &SpreadFragment,
        policy: ExactOptionalPropertyPolicy,
    ) {
        if alternative.residual {
            taint_named_members(state);
            taint_indices(state);
        }
        for member in alternative.members.iter() {
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
                            state.push_member(entry);
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
        for index in alternative.indices.iter() {
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

    /// Whether a direct effect leaves the key domain closed: only a
    /// computed key that resolves to no literal opens it.
    fn direct_effect_is_closed(&self, effect: &ObjectConstructionEffect) -> bool {
        let key = match effect {
            ObjectConstructionEffect::DirectProperty(effect) => &effect.key,
            ObjectConstructionEffect::DirectMethod(effect) => &effect.key,
            ObjectConstructionEffect::DirectGet(effect)
            | ObjectConstructionEffect::DirectSet(effect) => &effect.key,
            _ => return true,
        };
        self.resolve_authored_key(key).is_some()
    }

    pub(super) fn resolve_authored_key(&self, key: &AuthoredPropertyKey) -> Option<PropertyKey> {
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

/// Failures that refuse the construction itself, whichever facts a
/// selector reads.
fn is_construction_refusal(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::CheckerRecovery { .. }
            | QueryError::BudgetExceeded(_)
            | QueryError::Cancelled
            | QueryError::ExecutionAborted(_)
    )
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

fn authored_property_facets(effect: &AuthoredPropertyEffect) -> MemberFacets {
    evaluator_support::member_facets(
        effect.readonly,
        None,
        false,
        effect.visibility,
        effect.spans,
        effect.declaration_origin.clone(),
        effect.declared_in_macro_type_arg,
        effect.merge_role,
        effect.excess_origin,
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

/// What [`ProjectSemanticDispatch::merge_object_or_empty_union`] merged a
/// spread operand's union to.
enum UnionSpreadMerge {
    /// Every arm spreads into an empty object.
    Empty,
    /// The one object arm, read as its anonymous partial.
    Partial(crate::semantic_query::SurfaceView),
}

/// The checker's `isEmptyObjectTypeOrSpreadsIntoEmptyObject`: an empty
/// object type, or a nullish, boolean-, number-, bigint- or string-like
/// type, or `object`.
fn spreads_into_empty_object(data: &SemanticNodeData) -> bool {
    match data {
        SemanticNodeData::Primitive(
            PrimitiveKind::Null
            | PrimitiveKind::Undefined
            | PrimitiveKind::Boolean
            | PrimitiveKind::Number
            | PrimitiveKind::BigInt
            | PrimitiveKind::String
            | PrimitiveKind::Object,
        )
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::TemplateLiteral { .. } => true,
        SemanticNodeData::Object(surface) => {
            surface.positive_members().is_empty()
                && surface.call_signatures.is_empty()
                && surface.construct_signatures.is_empty()
                && surface.index_signatures.is_empty()
        }
        _ => false,
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
