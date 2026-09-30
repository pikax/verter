//! The conditional query's decision procedure (`getConditionalType`), the
//! one owner of how a conditional type is decided: its operands as the
//! checker constructs them, the absorbing lattice rows, distribution over a
//! union check, the permissive and restrictive tests and the relation that
//! select a branch, and the conditional kept whole where none decides it.
//!
//! A conditional read from a query key and one read from syntax take the
//! same procedure; lowering supplies only the branches it demands
//! ([`ConditionalBranches`]). A consumer asking which branch a
//! conditional takes (a relation, a closedness walk) reads the query's
//! typed outcome ([`ProjectSemanticDispatch::conditional_outcome`]).

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::{ConditionalBranchSelection, ProjectSemanticDispatch};
use crate::semantic_query::{
    BranchSelection, ConditionalPendingSubstitution, OriginEdgeKind, OriginMeta, QueryError,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// The infer-routing classification of a conditional's `extends` pattern
/// (see [`ProjectSemanticDispatch::conditional_infer_route`]): bare
/// `infer X`, an in-scope binding pattern, an out-of-scope deep pattern
/// (stays deferred), or no infer at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConditionalInferRoute {
    None,
    Bare,
    InScopePattern,
    OutOfScope,
}

/// A type read through the conditional query: what a consumer of a
/// conditional (a relation, a closedness walk) takes it as.
pub(super) enum ConditionalOutcome {
    /// Not a conditional.
    NotConditional,
    /// A conditional that reduced to this type.
    Reduced(SemanticNodeId),
    /// A conditional the checker keeps.
    Deferred(DeferredConditional),
    /// A conditional the query could not finish.
    Undecided,
}

impl ConditionalOutcome {
    /// The deferred conditional this operand is, if it is one.
    pub(super) fn into_deferred(self) -> Option<DeferredConditional> {
        match self {
            Self::Deferred(conditional) => Some(conditional),
            Self::NotConditional | Self::Reduced(_) | Self::Undecided => None,
        }
    }
}

/// A deferred conditional's operands and branches, the branches read
/// through its pending substitution.
pub(super) struct DeferredConditional {
    pub(super) check: SemanticNodeId,
    pub(super) extends: SemanticNodeId,
    pub(super) true_branch: SemanticNodeId,
    pub(super) false_branch: SemanticNodeId,
    pub(super) distributive: bool,
}

/// Whether a relation from `source` to `target` holds by the permissive
/// instantiation's wildcard type (`wildcardType`), which the definitely-false
/// test reads each type parameter as: it relates to every type, `never`
/// included, and every type relates to it (`isSimpleTypeRelatedTo`). The
/// one statement of that rule; every relation arm reads it here.
pub(super) fn wildcard_relates(source: &SemanticNodeData, target: &SemanticNodeData) -> bool {
    let wildcard = |data: &SemanticNodeData| {
        matches!(
            data,
            SemanticNodeData::Opaque(QueryError::PermissiveWildcard)
        )
    };
    wildcard(source) || wildcard(target)
}

/// A conditional's operands as the decision reads them.
struct ConditionalOperands {
    /// The check the selected branch binds, as written.
    check: SemanticNodeId,
    /// The type a declared check names, which distribution reads.
    declared_check: SemanticNodeId,
    /// The check the absorbing lattice rows read.
    absorbed_check: SemanticNodeId,
    extends: SemanticNodeId,
}

/// How the decision procedure reaches a conditional's branches.
trait ConditionalBranches {
    /// The branch `take_true` names, ready to stand as the answer.
    fn branch(&mut self, dispatch: &ProjectSemanticDispatch, take_true: bool) -> SemanticNodeId;
    /// Both branches, for a conditional kept whole; `None` when the
    /// request was cancelled between them.
    fn both(
        &mut self,
        dispatch: &ProjectSemanticDispatch,
    ) -> Option<(SemanticNodeId, SemanticNodeId)>;
    /// The pending substitution the branches carry.
    fn pending(&self) -> Option<Arc<ConditionalPendingSubstitution>>;
    /// One member of a distributed check, decided without distribution.
    fn member(
        &mut self,
        dispatch: &ProjectSemanticDispatch,
        member: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> super::walk::QueryBuildOutput;
    /// The selected branch's own inputs, recorded on the answer.
    fn committed(
        &self,
        _dispatch: &ProjectSemanticDispatch,
        _take_true: bool,
        _output: &mut super::walk::QueryBuildOutput,
    ) {
    }
}

/// The branch handles of a conditional query key, with their pending
/// substitution.
struct MaterializedBranches {
    true_branch: SemanticNodeId,
    false_branch: SemanticNodeId,
    pending: Option<Arc<ConditionalPendingSubstitution>>,
}

impl ConditionalBranches for MaterializedBranches {
    fn branch(&mut self, dispatch: &ProjectSemanticDispatch, take_true: bool) -> SemanticNodeId {
        dispatch.apply_conditional_branch_pending(
            if take_true {
                self.true_branch
            } else {
                self.false_branch
            },
            self.pending.as_deref(),
            take_true,
        )
    }

    fn both(
        &mut self,
        _dispatch: &ProjectSemanticDispatch,
    ) -> Option<(SemanticNodeId, SemanticNodeId)> {
        Some((self.true_branch, self.false_branch))
    }

    fn pending(&self) -> Option<Arc<ConditionalPendingSubstitution>> {
        self.pending.clone()
    }

    fn member(
        &mut self,
        dispatch: &ProjectSemanticDispatch,
        member: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> super::walk::QueryBuildOutput {
        dispatch.conditional_query_output(SemanticQueryKey::Conditional {
            check: member,
            extends,
            true_branch: self.true_branch,
            false_branch: self.false_branch,
            distributive: false,
            pending: self.pending.as_ref().map(|pending| {
                pending
                    .distributed_over(member)
                    .map_or_else(|| Arc::clone(pending), Arc::new)
            }),
        })
    }

    fn committed(
        &self,
        dispatch: &ProjectSemanticDispatch,
        take_true: bool,
        output: &mut super::walk::QueryBuildOutput,
    ) {
        let original = if take_true {
            self.true_branch
        } else {
            self.false_branch
        };
        let frame = self.pending.as_ref().map(|pending| {
            if take_true {
                pending.true_branch()
            } else {
                pending.false_branch()
            }
        });
        let nodes = std::iter::once(original).chain(
            frame
                .into_iter()
                .flat_map(|frame| frame.pairs().iter().map(|&(_, arg)| arg)),
        );
        for root in dispatch.observed_self_roots_from_nodes(nodes) {
            if !output.observed_self_roots.contains(&root) {
                output.observed_self_roots.push(root);
            }
        }
    }
}

/// The branches of a conditional written in syntax, lowered on demand.
struct LoweredBranches<'a> {
    lower_branch: &'a mut dyn FnMut(bool) -> SemanticNodeId,
}

impl ConditionalBranches for LoweredBranches<'_> {
    fn branch(&mut self, _dispatch: &ProjectSemanticDispatch, take_true: bool) -> SemanticNodeId {
        (self.lower_branch)(take_true)
    }

    fn both(
        &mut self,
        dispatch: &ProjectSemanticDispatch,
    ) -> Option<(SemanticNodeId, SemanticNodeId)> {
        let true_branch = (self.lower_branch)(true);
        if dispatch.ctx.is_cancelled() {
            return None;
        }
        Some((true_branch, (self.lower_branch)(false)))
    }

    fn pending(&self) -> Option<Arc<ConditionalPendingSubstitution>> {
        None
    }

    fn member(
        &mut self,
        dispatch: &ProjectSemanticDispatch,
        member: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> super::walk::QueryBuildOutput {
        dispatch.build_conditional_from_lowering(member, extends, false, self.lower_branch)
    }
}

impl ProjectSemanticDispatch<'_> {
    /// Conditional type (lazy-block evaluation +
    /// distributive-conditional authority), read from the materialized
    /// branch handles of a [`SemanticQueryKey::Conditional`].
    ///
    /// Evaluates `check extends extends ? true_branch : false_branch`
    /// using the shared relation engine and returns one of:
    ///
    /// - **Distributive union check** — when `distributive == true` AND
    ///   `check` resolves to a [`SemanticNodeData::Union`], the builder
    ///   distributes per-member by re-entering the dispatcher with
    ///   `SemanticQueryApi::execute(SemanticQueryKey::Conditional {
    ///   check: member, extends, true_branch, false_branch,
    ///   distributive: false })` for every member, then combines the
    ///   per-member results through
    ///   `SemanticQueryApi::execute(SemanticQueryKey::ReduceUnion {
    ///   members: per_member_results, nullability: Strict })`. Termination is guaranteed by
    ///   the `distributive: false` flag on each sub-query (no re-
    ///   distribution), the family memo's per-member dedup, and the
    ///   dispatch layer's same-path recursion sentinel. Dispatch owns
    ///   distributive distribution.
    /// - **Closed/decidable check** — one of the branch shell references
    ///   directly (no `Conditional` node interned). Emits a
    ///   [`OriginEdgeKind::ConditionalSelect`] edge with
    ///   [`BranchSelection::True`] or [`BranchSelection::False`]. The
    ///   unselected branch is NOT materialised beyond its shell
    ///   reference (it already has one via the key's
    ///   `true_branch` / `false_branch` fields), and it is a dead
    ///   operand for dependency purposes: the result's observed
    ///   self-roots cover `check`, `extends`, and the WINNER only.
    /// - **Open/undecidable check** — a
    ///   [`SemanticNodeData::Conditional`] shell with both branch
    ///   references intact. Emits
    ///   [`OriginEdgeKind::ConditionalSelect`] with
    ///   [`BranchSelection::Deferred`]. Neither branch is recursively
    ///   materialised; path projection into the result drives
    ///   per-subexpression lazy expansion.
    ///
    /// The relation evaluator handles the decidable shapes the shallow
    /// walker reaches directly: primitive identity, primitive-to-top/any,
    /// `never` bottom, exact node identity, and the obvious
    /// non-assignability cases. Object / union / intersection / generic
    /// relations stay deferred — the full solver routing lands via the
    /// `resolve_conditional` dispatch handoff in . Bare-infer
    /// bindings (`T extends infer X`) are handled by the shortcut below;
    /// nested-infer in complex patterns defers to the relation engine.
    pub(super) fn build_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
        pending: Option<Arc<ConditionalPendingSubstitution>>,
    ) -> super::walk::QueryBuildOutput {
        verter_audit::attribute_scope!(ConditionalReduce);
        self.decide_conditional(
            check,
            extends,
            distributive,
            &mut MaterializedBranches {
                true_branch,
                false_branch,
                pending,
            },
        )
    }

    /// Transient lowering ingress. Only selected syntax is lowered; no
    /// callback or syntax enters graph identity. The conditional is decided
    /// by the same procedure as a materialized one: lowering supplies only
    /// the branches it demands.
    pub(super) fn build_conditional_from_lowering(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        lower_branch: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> super::walk::QueryBuildOutput {
        self.decide_conditional(
            check,
            extends,
            distributive,
            &mut LoweredBranches { lower_branch },
        )
    }

    /// The conditional query's one decision procedure (`getConditionalType`):
    /// the operands as the checker constructs them, the lattice rows that
    /// absorb the conditional, distribution over a union check, and the
    /// branch selection; a conditional none of them decides is kept whole.
    /// `branches` supplies only the branches the decision demands.
    fn decide_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        branches: &mut dyn ConditionalBranches,
    ) -> super::walk::QueryBuildOutput {
        if self.ctx.is_cancelled() {
            return self.cancelled_build_output();
        }
        let operands = self.conditional_operands(check, extends);
        let extends = operands.extends;
        if let Some(absorbed) = self.absorb_conditional(
            operands.absorbed_check,
            extends,
            distributive,
            |take_true| branches.branch(self, take_true),
        ) {
            return absorbed;
        }
        if distributive {
            if let Some(output) =
                self.distribute_conditional(operands.declared_check, extends, &mut |member| {
                    branches.member(self, member, extends)
                })
            {
                return output;
            }
        }
        let check = operands.check;
        let (selection, infer) = self.conditional_branch_selection(check, extends);
        if let Some(output) = self.absorb_named_error_operand(check, extends) {
            return output;
        }
        if self.ctx.is_cancelled() {
            return self.cancelled_build_output();
        }
        match selection {
            ConditionalBranchSelection::True | ConditionalBranchSelection::False => {
                let take_true = selection == ConditionalBranchSelection::True;
                let winner = branches.branch(self, take_true);
                let mut output =
                    self.commit_conditional_winner(check, extends, winner, selection, infer);
                branches.committed(self, take_true, &mut output);
                output
            }
            ConditionalBranchSelection::Deferred | ConditionalBranchSelection::Undecided => {
                let Some((true_branch, false_branch)) = branches.both(self) else {
                    return self.cancelled_build_output();
                };
                self.keep_conditional(
                    check,
                    extends,
                    (true_branch, false_branch),
                    distributive,
                    branches.pending(),
                    selection,
                )
            }
        }
    }

    /// A conditional's operands as the checker constructs them before it
    /// relates them: a union or intersection operand is the type its
    /// written names reduce to (`1 & ReturnType<typeof anyf>` is `any`), an
    /// instantiated conditional operand the type it reduces to, and a name
    /// for the checker's error type that error type.
    fn conditional_operands(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> ConditionalOperands {
        let check = self.composite_over_resolved_arms(check).unwrap_or(check);
        let check = self.union_as_constructed(check);
        // The type a declared check names decides absorption and
        // distribution (a reference to `type Nv = never` distributes as
        // `never`), while the selected branch binds the check as written
        // (`O extends infer K ? K : 2` is `O`).
        let declared_check = self.declared_operand_where_written(check);
        let declared_check = self.union_as_constructed(declared_check);
        let extends = self
            .composite_over_resolved_arms(extends)
            .unwrap_or(extends);
        // An operand that is itself an instantiated conditional is the type
        // it reduces to (`unknown extends ThisParameterType<F>` relates to
        // `ThisParameterType<F>`'s branch); one still open stays itself.
        let check = self.reduced_conditional_operand(check);
        let extends = self.reduced_conditional_operand(extends);
        let declared_check = self.reduced_conditional_operand(declared_check);
        let check = self.conditional_operand_extreme(check).unwrap_or(check);
        let extends = self.conditional_operand_extreme(extends).unwrap_or(extends);
        let declared_check = self
            .conditional_operand_extreme(declared_check)
            .unwrap_or(declared_check);
        ConditionalOperands {
            check,
            declared_check,
            absorbed_check: self.indexed_access_where_written(declared_check),
            extends,
        }
    }

    /// The conditional kept whole: the checker's deferred type, or, for a
    /// conditional the checker decides and the lane could not, a typed gap
    /// in its place.
    fn keep_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        (true_branch, false_branch): (SemanticNodeId, SemanticNodeId),
        distributive: bool,
        pending: Option<Arc<ConditionalPendingSubstitution>>,
        selection: ConditionalBranchSelection,
    ) -> super::walk::QueryBuildOutput {
        let suspended_roots = self.observed_self_roots_from_nodes(
            [check, extends, true_branch, false_branch]
                .into_iter()
                .chain(pending.iter().flat_map(|frame| frame.argument_nodes())),
        );
        let node = self.graph().intern_node(SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref: true_branch,
            false_branch_ref: false_branch,
            distributive,
            pending,
        });
        let fence = self.project_generation_signature();
        self.graph().record_origin_edge(
            node,
            OriginEdgeKind::ConditionalSelect,
            Arc::from([check, extends]),
            OriginMeta::Branch(BranchSelection::Deferred),
            fence.clone(),
        );
        self.graph().record_conditional_deferred();
        let mut output = super::walk::QueryBuildOutput::from((QueryResult::Value(node), fence))
            .with_observed_self_roots(suspended_roots);
        // A conditional the checker decides and the lane could not is a
        // typed gap: the shell stands where the checker's answer should be,
        // so it is never published complete or warm-admitted.
        if selection == ConditionalBranchSelection::Undecided {
            output.result_is_partial = true;
            output.cache_suppress = true;
            output.partial_reasons = output
                .partial_reasons
                .union(crate::semantic_query::PartialReasonSet::UNDECIDED_CONDITIONAL);
        }
        output
    }

    /// The lattice extreme a conditional operand is where the checker
    /// constructs it. The checker instantiates a conditional's check and
    /// extends types before it relates them, so:
    /// - a union or an intersection is the extreme its members reduce it to
    ///   (`getUnionType` / `getIntersectionType`): `never`, then the error
    ///   type, then `any` decide an intersection (`1 & any` is `any`, `1 &
    ///   any & never` is `never`); the error type, `any`, then `unknown`
    ///   decide a union;
    /// - an intersection whose cross product the checker refuses is its
    ///   error type.
    ///
    /// The members are read as the types they name, whatever carrier the
    /// composite was built as: an intersection substituted into an
    /// instantiated body is still the intersection the checker constructs.
    /// `None` for any other operand; an operand that is itself a name is
    /// read once the conditional is related
    /// ([`Self::absorb_named_error_operand`]), so the relation, not this
    /// read, decides where its evaluation joins the demand.
    fn conditional_operand_extreme(&self, operand: SemanticNodeId) -> Option<SemanticNodeId> {
        use super::absorb::SpecialKind;
        let graph = self.graph();
        let (arms, is_union) = match graph.node_data(operand).as_deref() {
            Some(
                SemanticNodeData::Alias(_)
                | SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. }
                | SemanticNodeData::TypeOf(_)
                | SemanticNodeData::BareRef(_)
                | SemanticNodeData::ImportType(_)
                | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
            ) => return None,
            Some(SemanticNodeData::Intersection(arms)) => (arms.members_arc(), false),
            Some(SemanticNodeData::Union(arms)) => (arms.members_arc(), true),
            Some(SemanticNodeData::TemplateLiteral { .. }) => {
                return self.template_operand_recovery(operand)
            }
            _ => return None,
        };
        if !is_union {
            if let Some(recovery) = self.intersection_too_complex(operand) {
                return Some(recovery);
            }
        }
        let mut found: [Option<SemanticNodeId>; 4] = [None; 4];
        for arm in arms.iter() {
            let arm_type = match graph.node_data(*arm).as_deref() {
                Some(
                    SemanticNodeData::Intersection(_)
                    | SemanticNodeData::Union(_)
                    | SemanticNodeData::TemplateLiteral { .. },
                ) => self.conditional_operand_extreme(*arm),
                Some(
                    SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::TypeOf(_)
                    | SemanticNodeData::BareRef(_)
                    | SemanticNodeData::ImportType(_)
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
                ) => self.resolved_operand(*arm),
                _ => Some(*arm),
            };
            let Some(arm_type) = arm_type else { continue };
            let slot = match self.peek_special(arm_type) {
                Some((SpecialKind::Never, _)) => 0,
                Some((SpecialKind::Error, _)) => 1,
                Some((SpecialKind::Any, _)) => 2,
                Some((SpecialKind::Unknown, _)) => 3,
                None => continue,
            };
            found[slot].get_or_insert(arm_type);
        }
        let [never, error, any, unknown] = found;
        if is_union {
            error.or(any).or(unknown)
        } else {
            never.or(error).or(any)
        }
    }

    /// The checker's recovery a template literal type `operand` is when
    /// the checker refuses its cross product; `None` otherwise.
    fn template_operand_recovery(&self, operand: SemanticNodeId) -> Option<SemanticNodeId> {
        let (pattern, args) = match self.graph().node_data(operand).as_deref() {
            Some(SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            }) => (Arc::clone(quasis), Arc::clone(expressions)),
            _ => return None,
        };
        let read = self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
            pattern,
            args,
            context: self.template_literal_reduce_context(),
        });
        match read.value {
            QueryResult::Value(reduced) => matches!(
                self.graph().node_data(reduced).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
            )
            .then_some(reduced),
            _ => None,
        }
    }

    /// The type a name operand resolves to, at structural transit; `None`
    /// when its demand does not complete.
    fn resolved_operand(&self, operand: SemanticNodeId) -> Option<SemanticNodeId> {
        self.normalize_node_for_structural_fact_demand(
            operand,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
        .into_complete_node()
    }

    fn conditional_query_output(&self, key: SemanticQueryKey) -> super::walk::QueryBuildOutput {
        let read = self.execute_read(key);
        let mut output = super::walk::QueryBuildOutput::from((read.value, read.dep_signature));
        output.cache_suppress = read.cache_suppress;
        output.result_is_partial = read.result_is_partial;
        output.partial_reasons = read.partial_reasons;
        output
    }

    /// The sole union distributor for materialized and lowering-time
    /// conditionals. Each member disables distribution; materialized operands
    /// re-enter the conditional family, while syntax lowers only its winner.
    fn distribute_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        reduce_member: &mut dyn FnMut(SemanticNodeId) -> super::walk::QueryBuildOutput,
    ) -> Option<super::walk::QueryBuildOutput> {
        let (resolved_check, members) = self.distributive_check_union_members(check)?;
        let mut output = super::walk::QueryBuildOutput::from((
            QueryResult::Error(QueryError::Miss),
            self.project_generation_signature(),
        ))
        .with_observed_self_roots(self.observed_self_roots_from_nodes([
            check,
            resolved_check,
            extends,
        ]));
        let mut per_member = Vec::with_capacity(members.len());
        for &member in members.iter() {
            if self.ctx.is_cancelled() {
                return Some(self.cancelled_build_output());
            }
            let member_output = reduce_member(member);
            output.cache_suppress |= member_output.cache_suppress;
            output.result_is_partial |= member_output.result_is_partial;
            output.partial_reasons = output.partial_reasons.union(member_output.partial_reasons);
            output
                .observed_self_roots
                .extend(member_output.observed_self_roots);
            match member_output.result {
                QueryResult::Value(node) => per_member.push(node),
                _ => {
                    self.fold_into_top_build_local_taint_with(true, true, output.partial_reasons);
                    self.deposit_operand_self_roots(&output.observed_self_roots);
                    return None;
                }
            }
        }
        let normalized = self.conditional_query_output(SemanticQueryKey::ReduceUnion {
            members: Arc::from(per_member),
            nullability: crate::semantic_query::NullabilityPolicy::Strict,
        });
        output.cache_suppress |= normalized.cache_suppress;
        output.result_is_partial |= normalized.result_is_partial;
        output.partial_reasons = output.partial_reasons.union(normalized.partial_reasons);
        output.result = normalized.result;
        Some(output)
    }

    /// Selection side effects have one owner at both forcing sites. Only a
    /// demanded winner reaches inference substitution or origin recording.
    fn commit_conditional_winner(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        winner: SemanticNodeId,
        selection: ConditionalBranchSelection,
        infer: Option<super::relation::RelationInferBindings>,
    ) -> super::walk::QueryBuildOutput {
        if self.ctx.is_cancelled() {
            return self.cancelled_build_output();
        }
        let graph = self.graph();
        let fence = self.project_generation_signature();
        let mut result = winner;
        if let Some(selected) = infer {
            for binding in selected.bindings.iter() {
                result = self.substitute_semantic_type_param(result, binding.param, binding.bound);
                graph.record_origin_edge(
                    result,
                    OriginEdgeKind::InferBind,
                    Arc::from([check, extends]),
                    OriginMeta::SubstitutedParam(Arc::clone(&binding.name)),
                    fence.clone(),
                );
            }
            // Only a conditional re-reduces here. Other carriers retain the
            // consumer's projection context and its existing demand point.
            if matches!(
                graph.node_data(result).as_deref(),
                Some(SemanticNodeData::Conditional { .. })
            ) {
                result = self.evaluate_deferred_semantic_node(result);
            }
        }
        if self.ctx.is_cancelled() {
            return self.cancelled_build_output();
        }
        let branch = match selection {
            ConditionalBranchSelection::True => {
                graph.record_branch_selection_true();
                BranchSelection::True
            }
            ConditionalBranchSelection::False => {
                graph.record_branch_selection_false();
                BranchSelection::False
            }
            ConditionalBranchSelection::Deferred | ConditionalBranchSelection::Undecided => {
                unreachable!("only decided winners are committed")
            }
        };
        graph.record_origin_edge(
            result,
            OriginEdgeKind::ConditionalSelect,
            Arc::from([check, extends]),
            OriginMeta::Branch(branch),
            fence.clone(),
        );
        graph.record_conditional_decided();
        super::walk::QueryBuildOutput::from((QueryResult::Value(result), fence))
            .with_observed_self_roots(
                self.observed_self_roots_from_nodes([check, extends, winner, result]),
            )
    }

    /// The resolved check and its union members, or `None` when the check
    /// does not distribute. Retain the resolved node so its file scope can
    /// root the result even when the input is a global alias.
    ///
    /// Distribution semantics follow the check's INSTANTIATED surface,
    /// not the check node's raw shape: a naked-type-param check whose
    /// binding substituted an `Alias` / `DeclRef` / `InstantiationRef`
    /// CARRIER of a union (the defaulted-union type argument
    /// `T extends SingleOrMultiple = SingleOrMultiple` of reka-ui's
    /// `AccordionRootEmits`) distributes exactly like a raw `Union`
    /// binding. The raw-`Union` fast path stays allocation-free; the
    /// carrier shapes resolve through the shared structural-fact demand
    /// primitive ([`Self::normalize_node_for_structural_fact_demand`] —
    /// the same `ResolveDecl` / `Instantiate` delegation the relation
    /// engine's own demand-resolve uses, under the relation engine's
    /// `StructuralTransit` context so the union-ness read never reifies
    /// publication member edges).
    ///
    /// Fail-closed on BOTH rails: a `Partial` demand (budget / cycle /
    /// fault — the primitive folds the reasons into the active build
    /// taint frame) and a resolved non-`Union` surface return `None`,
    /// deferring to the tri-state oracle path. `TypeParam` / `Infer`
    /// shells are STABLE STOPS of the deferred evaluator, so an OPEN
    /// generic check never distributes over its constraint or default —
    /// open conditionals stay deferred shells.
    pub(super) fn distributive_check_union_members(
        &self,
        check: SemanticNodeId,
    ) -> Option<(SemanticNodeId, Arc<[SemanticNodeId]>)> {
        let union_members_of = |node: SemanticNodeId| {
            self.graph().node_data(node).and_then(|data| match &*data {
                SemanticNodeData::Union(members) => Some((node, members.members_arc())),
                _ => None,
            })
        };
        if let Some(surface) = union_members_of(check) {
            return Some(surface);
        }
        // A `keyof` over a type holding no type parameter is the union of
        // its keys (`getIndexType` reduces it eagerly), which the check
        // distributes over.
        let closed_key_of = match self.graph().node_data(check).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) if !self.mentions_binder(*base) => Some(*base),
            _ => None,
        };
        if let Some(base) = closed_key_of {
            return union_members_of(self.key_set_of(base)?);
        }
        // Only carrier / deferred-shell shapes can still hide a union;
        // every other shape is already terminal for the union-ness fact.
        let is_resolvable_shell = matches!(
            self.graph().node_data(check).as_deref(),
            Some(
                SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
            )
        );
        if !is_resolvable_shell {
            return None;
        }
        let resolved = self
            .normalize_node_for_structural_fact_demand(
                check,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .into_complete_node()?;
        union_members_of(resolved)
    }

    /// The type a conditional operand denotes when it is a reference to a
    /// declaration: a type alias is the type it names, so `IsStr<U2>` with
    /// `type U2 = "a" | 1` distributes over `"a"` and `1`, `IsStr<Nv>` with
    /// `type Nv = never` is `never`, and `any extends Un` with `type Un =
    /// unknown` selects its true branch alone (the checker's operand is the
    /// resolved type, never the name). A reference the structural demand
    /// cannot resolve completely, and every other operand, is itself.
    pub(super) fn declared_operand_where_written(&self, node: SemanticNodeId) -> SemanticNodeId {
        let identity = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => identity.clone(),
            Some(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            })) => crate::semantic_query::DeclIdentity {
                canonical_id: Arc::clone(canonical_id),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: Arc::clone(name),
            },
            _ => return node,
        };
        // An interface or a class names an object type, which no lattice
        // row or distribution reads.
        if !matches!(
            self.prepared_decl_kind(&identity),
            Some(verter_semantic::analysis::type_eval::TypeDeclKind::Alias)
        ) {
            return node;
        }
        self.normalize_node_for_structural_fact_demand(
            node,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
        .into_complete_node()
        .unwrap_or(node)
    }

    /// The type an operand denotes where it is written. The checker
    /// instantiates a conditional's check and branch types eagerly, so an
    /// indexed access over a type that is not generic IS the property type
    /// it reads (`Rec["a"]` with `a: any` is `any`): the lattice rows of
    /// [`Self::absorb_conditional`] apply to that check type, and the union
    /// of both branches an `any` check selects is built from those branch
    /// types — the same read the relation takes of an indexed-access
    /// operand. Every other operand, and an indexed access the deferred
    /// evaluator cannot read further (`T["k"]` over an open `T`), is itself.
    pub(super) fn indexed_access_where_written(&self, node: SemanticNodeId) -> SemanticNodeId {
        if !matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::IndexedAccess { .. })
        ) {
            return node;
        }
        self.evaluate_deferred_semantic_node_with_context(
            node,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
        .into_active_query_build_node(self)
    }

    /// Conditional branch selection, the last step of the decision
    /// procedure ([`Self::decide_conditional`]); a consumer asking which
    /// branch a conditional takes reads the query's outcome
    /// ([`Self::conditional_outcome`]), never this step alone. Returns the
    /// selection PLUS the relation payload's inference bindings when the
    /// selection came from a binding-producing relation, which the
    /// procedure substitutes into the selected branch.
    ///
    /// `Deferred` is the checker's own deferral and `Undecided` a
    /// conditional the checker decides and the lane cannot. Decision order:
    ///
    /// 1. an `error` check DOMINATES the whole conditional — no branch
    ///    is selected ⇒ `Deferred` (in `build_conditional` this row is
    ///    pre-absorbed by `absorb_conditional`; the guard makes the
    ///    oracle safe for classifier callers, which have no absorber in
    ///    front of them);
    /// 2. a generic operand ⇒ `Deferred` ([`Self::conditional_is_deferred`]);
    /// 3. infer routing: a BARE-infer extends (`C extends infer X`) binds
    ///    `X := check` through the relation for ANY check, so it precedes
    ///    the `any` guard; an out-of-scope deep infer pattern is decided
    ///    only when its permissive instantiation fails
    ///    ([`Self::permissive_conditional_selection`]);
    /// 4. an `any` check against an infer pattern, which the checker reads
    ///    as the union of both branches with the pattern inferred from
    ///    `any` ⇒ `Undecided` (every other `any` check is absorbed in
    ///    build before this oracle);
    /// 5. the SOLE relation authority (`execute(SemanticQueryKey::Relate)`
    ///    via [`Self::execute_relate_pair`]): an in-scope infer pattern
    ///    binds THROUGH the relation (object property, tuple head/tail,
    ///    function inference); a plain pair decides through the same
    ///    authority; a failure whose permissive instantiation still relates
    ///    ⇒ `Deferred`; an undecided step (`Unknown`, a budget, an
    ///    assumption) ⇒ `Undecided`. The O(tag) prefilter lives INSIDE the
    ///    authority — it is never consulted here (never a parallel truth
    ///    source).
    pub(super) fn conditional_branch_selection(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> (
        ConditionalBranchSelection,
        Option<super::relation::RelationInferBindings>,
    ) {
        if matches!(
            self.peek_special(check),
            Some((super::absorb::SpecialKind::Error, _))
        ) {
            return (ConditionalBranchSelection::Deferred, None);
        }
        // The checker defers a conditional whose check or extends type is
        // generic (`getConditionalType`'s `isDeferredType`) before relating
        // or inferring anything: `T extends unknown ? [T] : never` stays a
        // conditional until `T` is known, and distributes over the union it
        // receives; `T extends infer X ? A : B` too.
        if self.conditional_is_deferred(check, extends) {
            return (ConditionalBranchSelection::Deferred, None);
        }
        let route = self.conditional_infer_route(extends);
        if matches!(route, ConditionalInferRoute::OutOfScope) {
            return (self.permissive_conditional_selection(check, extends), None);
        }
        if matches!(route, ConditionalInferRoute::Bare) {
            // `check extends infer X` binds `X := check` through the
            // relation for ANY check (`any` included — the pre-any-guard
            // placement is load-bearing).
            return match self.execute_relate_pair(check, extends) {
                super::dispatch_txn::RelationStep::Assignable { bindings } => {
                    self.select_with_inferences(check, extends, bindings)
                }
                super::dispatch_txn::RelationStep::NotAssignable => {
                    (ConditionalBranchSelection::False, None)
                }
                _ => (ConditionalBranchSelection::Undecided, None),
            };
        }
        if matches!(
            self.peek_special(check),
            Some((super::absorb::SpecialKind::Any, _))
        ) {
            return (ConditionalBranchSelection::Undecided, None);
        }
        // The full relation authority — the SAME `execute(Relate)` path
        // every consumer rides. A binding-producing judgement's returned
        // bindings substitute into the selected (true) branch.
        let related = self.execute_relate_pair(check, extends);
        let selected = |bindings: std::sync::Arc<[crate::semantic_query::InferBinding]>| {
            if !matches!(route, ConditionalInferRoute::InScopePattern) {
                return (ConditionalBranchSelection::True, None);
            }
            self.select_with_inferences(check, extends, bindings)
        };
        let params = self.type_params_within(&[check, extends]);
        if params.is_empty() {
            return match related {
                super::dispatch_txn::RelationStep::Assignable { bindings } => selected(bindings),
                super::dispatch_txn::RelationStep::NotAssignable => {
                    (ConditionalBranchSelection::False, None)
                }
                _ => (ConditionalBranchSelection::Undecided, None),
            };
        }
        // Operands holding a type parameter the checker does not defer on
        // (`{ v: T } extends { v: string }`) are decided by two tests of
        // `getConditionalType`: definitely false when the permissive
        // instantiation (every type parameter the wildcard type) fails,
        // definitely true when the restrictive instantiation (every type
        // parameter without its constraint) holds; deferred otherwise.
        match self.permissive_relation(check, extends, &params) {
            super::dispatch_txn::RelationStep::NotAssignable => {
                return (ConditionalBranchSelection::False, None)
            }
            super::dispatch_txn::RelationStep::Assignable { .. } => {}
            _ => return (ConditionalBranchSelection::Undecided, None),
        }
        match (self.restrictive_relation(check, extends, &params), related) {
            (
                super::dispatch_txn::RelationStep::Assignable { .. },
                super::dispatch_txn::RelationStep::Assignable { bindings },
            ) => selected(bindings),
            (super::dispatch_txn::RelationStep::NotAssignable, _) => {
                (ConditionalBranchSelection::Deferred, None)
            }
            _ => (ConditionalBranchSelection::Undecided, None),
        }
    }

    /// The branch an inferring `extends` pattern selects once its
    /// inferences are fixed. What the pattern infers selects no branch by
    /// itself: an `infer` whose inference its declared or implied constraint
    /// refuses takes the constraint (`getInferredType`), and the checker relates the
    /// check to the pattern instantiated with the fixed types
    /// (`getConditionalType`) — `[1]` against `[infer X extends string]`
    /// fixes `string` and takes the false branch, and `{ a: 1; b: (x:
    /// string) => void }` against `{ a: infer U; b: (x: infer U) => void }`
    /// infers `1` and takes it too. A pattern that reaches each of its
    /// unconstrained `infer` declarations once infers each from the one
    /// position it occupies, where the inferring relation already related it:
    /// that relation is the check.
    fn select_with_inferences(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        bindings: std::sync::Arc<[crate::semantic_query::InferBinding]>,
    ) -> (
        ConditionalBranchSelection,
        Option<super::relation::RelationInferBindings>,
    ) {
        let constraints = self.infer_constraints_in(extends);
        let constrained = !constraints.is_empty();
        let bindings = if constrained {
            match self.infer_bindings_within_constraints(&bindings, &constraints) {
                Some(bindings) => bindings,
                None => return (ConditionalBranchSelection::Undecided, None),
            }
        } else {
            bindings
        };
        if !constrained && !self.pattern_repeats_an_infer(extends) {
            return (
                ConditionalBranchSelection::True,
                Some(super::relation::RelationInferBindings { bindings }),
            );
        }
        let instantiated = bindings.iter().fold(extends, |node, binding| {
            self.substitute_semantic_type_param(node, binding.param, binding.bound)
        });
        match self.relate_outside_inference(check, instantiated) {
            super::dispatch_txn::RelationStep::Assignable { .. } => (
                ConditionalBranchSelection::True,
                Some(super::relation::RelationInferBindings { bindings }),
            ),
            super::dispatch_txn::RelationStep::NotAssignable => {
                (ConditionalBranchSelection::False, None)
            }
            _ => (ConditionalBranchSelection::Undecided, None),
        }
    }

    /// Whether `pattern` reaches one of its `infer` declarations along more
    /// than one path (`{ a: infer U; b: (x: infer U) => void }`), or declares
    /// one binder at two nodes; a homomorphic mapped type over one is one
    /// position of it. Two passes over the pattern, each node read
    /// once: which nodes hold an `infer`, then whether a node holding one is
    /// reached twice.
    fn pattern_repeats_an_infer(&self, pattern: SemanticNodeId) -> bool {
        let graph = self.graph();
        let mut holds: rustc_hash::FxHashMap<SemanticNodeId, bool> =
            rustc_hash::FxHashMap::default();
        let mut stack: Vec<(SemanticNodeId, bool)> = vec![(pattern, false)];
        while let Some((node, children_done)) = stack.pop() {
            let Some(data) = graph.node_data(node) else {
                holds.insert(node, false);
                continue;
            };
            if children_done {
                let mut held = matches!(data.as_ref(), SemanticNodeData::Infer { .. });
                let _ = data.for_each_child(|child| {
                    held |= holds.get(&child).copied().unwrap_or(false);
                });
                holds.insert(node, held);
                continue;
            }
            if holds.contains_key(&node) {
                continue;
            }
            stack.push((node, true));
            let _ = data.for_each_child(|child| {
                if !holds.contains_key(&child) {
                    stack.push((child, false));
                }
            });
        }
        let mut reached: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut binders: FxHashSet<crate::semantic_query::InferBinderId> = FxHashSet::default();
        let mut stack = vec![pattern];
        while let Some(node) = stack.pop() {
            if !holds.get(&node).copied().unwrap_or(false) {
                continue;
            }
            if !reached.insert(node) {
                return true;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::Infer { binder, .. } => {
                    if !binders.insert(binder.clone()) {
                        return true;
                    }
                }
                // A homomorphic mapped type over an `infer` declaration
                // (`{ [K in keyof infer T]: X }`) is one position of it: its
                // key space and template read it where the reverse mapping
                // infers it.
                SemanticNodeData::Mapped { source, .. } => {
                    if let Some(SemanticNodeData::Infer { binder, .. }) =
                        graph.node_data(*source).as_deref()
                    {
                        if !binders.insert(binder.clone()) {
                            return true;
                        }
                        continue;
                    }
                }
                _ => {}
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        false
    }

    /// A written union as the checker constructs it (`getUnionType`): an
    /// `any` or `unknown` member is the whole union, a literal beside its
    /// primitive and a duplicate go, and without `strictNullChecks` so do
    /// `null` and `undefined` beside another member. A type argument keeps
    /// the union as written (`D<string | unknown>`); the conditional it
    /// reaches distributes over the union it constructs, and relates it.
    fn union_as_constructed(&self, node: SemanticNodeId) -> SemanticNodeId {
        let members = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.members_arc(),
            _ => return node,
        };
        let nullability = if self.relation_strict_config().strict_null_checks {
            crate::semantic_query::NullabilityPolicy::Strict
        } else {
            crate::semantic_query::NullabilityPolicy::Erased
        };
        let constructed = self.intern_normalized_union(&members, nullability);
        let same_members = match self.graph().node_data(constructed).as_deref() {
            Some(SemanticNodeData::Union(kept)) => {
                let kept = kept.members_arc();
                kept.len() == members.len() && kept.iter().all(|member| members.contains(member))
            }
            _ => false,
        };
        if same_members {
            return node;
        }
        // The written union's files are what the reduced one was read from.
        self.deposit_operand_self_roots(&self.observed_self_roots_from_nodes([node]));
        constructed
    }

    /// The checker's definitely-false test (`getPermissiveInstantiation`):
    /// `check` related to `extends` with each of `params` read as the
    /// wildcard type, which relates both ways like `any` and turns an
    /// operation over it into itself.
    pub(super) fn permissive_relation(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        params: &[SemanticNodeId],
    ) -> super::dispatch_txn::RelationStep {
        let wildcard = self
            .graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::PermissiveWildcard));
        let permissive = |node: SemanticNodeId| {
            params.iter().fold(node, |node, param| {
                self.substitute_semantic_type_param(node, *param, wildcard)
            })
        };
        let (check, extends) = (permissive(check), permissive(extends));
        self.wildcard_failure_undecided(
            [check, extends],
            wildcard,
            self.relate_outside_inference(check, extends),
        )
    }

    /// `step`, a relation over a permissive instantiation, except that its
    /// failure is undecided where an operand holds an operation over the
    /// wildcard (`Uppercase<W>` and every other builtin utility, `W["k"]`, a
    /// template hole, a conditional over it): the checker's instantiation turns such an operation into
    /// the wildcard itself, which relates to everything, and the lane reads
    /// the operation as written.
    fn wildcard_failure_undecided(
        &self,
        operands: [SemanticNodeId; 2],
        wildcard: SemanticNodeId,
        step: super::dispatch_txn::RelationStep,
    ) -> super::dispatch_txn::RelationStep {
        if !matches!(step, super::dispatch_txn::RelationStep::NotAssignable) {
            return step;
        }
        let graph = self.graph();
        let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut stack = operands.to_vec();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            let operation = match data.as_ref() {
                SemanticNodeData::IntrinsicApplication { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::KeyOf { .. }
                | SemanticNodeData::TemplateLiteral { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::Mapped { .. } => true,
                // A builtin utility is an operation too (`Uppercase<W>`).
                SemanticNodeData::InstantiationRef { base, .. } => {
                    base.canonical_id.as_ref() == "__builtin__"
                }
                _ => false,
            };
            let mut over_wildcard = false;
            let _ = data.for_each_child(|child| {
                over_wildcard |= child == wildcard;
                stack.push(child);
            });
            if operation && over_wildcard {
                return super::dispatch_txn::RelationStep::Unknown;
            }
        }
        step
    }

    /// The checker's definitely-true test (`getRestrictiveInstantiation`):
    /// `check` related to `extends` with each of `params` replaced by the
    /// same parameter without its constraint, so a parameter relates only to
    /// itself and to what every type relates to.
    pub(super) fn restrictive_relation(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        params: &[SemanticNodeId],
    ) -> super::dispatch_txn::RelationStep {
        let graph = self.graph();
        let restrictive: Vec<(SemanticNodeId, SemanticNodeId)> = params
            .iter()
            .filter_map(|param| {
                let data = graph.node_data(*param)?;
                let mut unconstrained = data.as_ref().clone();
                drop(data);
                match &mut unconstrained {
                    SemanticNodeData::TypeParam {
                        constraint: constraint @ Some(_),
                        ..
                    } => *constraint = None,
                    _ => return None,
                }
                Some((*param, graph.intern_preserving_scope(*param, unconstrained)))
            })
            .collect();
        let instantiate = |node: SemanticNodeId| {
            restrictive
                .iter()
                .fold(node, |node, (param, unconstrained)| {
                    self.substitute_semantic_type_param(node, *param, *unconstrained)
                })
        };
        self.relate_outside_inference(instantiate(check), instantiate(extends))
    }

    /// `source` related to `target` as `isTypeAssignableTo` relates them:
    /// no inference session of the enclosing call binds a type parameter
    /// either reads, so each is rigid.
    fn relate_outside_inference(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> super::dispatch_txn::RelationStep {
        self.dispatch_txn.borrow_mut().begin_binding_disabled();
        let step = self.execute_relate_pair(source, target);
        self.dispatch_txn.borrow_mut().end_binding_disabled();
        step
    }

    /// The type an alias application names, instantiated as the checker
    /// instantiates it where it is written and not reduced further: the
    /// substituted declared body (a conditional over a type variable stays
    /// the deferred conditional). `None` when the instantiation does not
    /// complete.
    fn alias_application_instantiated(
        &self,
        base: &crate::semantic_query::DeclIdentity,
        args: &Arc<[SemanticNodeId]>,
    ) -> Option<SemanticNodeId> {
        let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            self.type_slot_for(
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            ),
            Arc::clone(args),
            self.instantiate_context_for(
                &base.canonical_id,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            ),
        ));
        let read = self.execute_read(key);
        if read.result_is_partial {
            return None;
        }
        match read.value {
            QueryResult::Value(node) => Some(node),
            _ => None,
        }
    }

    /// Whether `node` holds a type variable free in it: a type parameter no
    /// signature within declares, or a reference to an enclosing `infer`.
    fn mentions_free_type_variable(&self, node: SemanticNodeId) -> bool {
        if !self.type_params_within(&[node]).is_empty() {
            return true;
        }
        let graph = self.graph();
        let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if matches!(data.as_ref(), SemanticNodeData::InferRef { .. }) {
                return true;
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        false
    }

    /// The free type parameters `roots` hold anywhere below them — not
    /// those a generic signature within declares (instantiating a signature
    /// maps its own type parameters to fresh ones) — read from an explicit
    /// stack.
    pub(super) fn type_params_within(&self, roots: &[SemanticNodeId]) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut bound: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut stack: Vec<SemanticNodeId> = roots.to_vec();
        let mut params = Vec::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::TypeParam { .. } => {
                    params.push(node);
                    continue;
                }
                SemanticNodeData::Signature {
                    type_parameters, ..
                } => bound.extend(type_parameters.iter().map(|declared| declared.param)),
                _ => {}
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        params.retain(|param| !bound.contains(param));
        params
    }

    /// Whether the checker defers the conditional `check extends extends`
    /// (`isDeferredType` in `getConditionalType`): either operand is
    /// generic, or both are tuples of one arity whose elements are all
    /// required (`checkTuples`) and an element of either is. An `infer`
    /// declaration of the extends type is no type variable of the check: it
    /// is inferred before the test.
    pub(super) fn conditional_is_deferred(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> bool {
        if self.type_is_generic(check) || self.type_is_generic(extends) {
            return true;
        }
        let graph = self.graph();
        let simple_tuple = |node: SemanticNodeId| match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Tuple { elements, .. })
                if elements
                    .iter()
                    .all(|element| !element.optional && !element.rest) =>
            {
                Some(
                    elements
                        .iter()
                        .map(|element| element.value)
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        };
        match (simple_tuple(check), simple_tuple(extends)) {
            (Some(check), Some(extends)) if check.len() == extends.len() => check
                .into_iter()
                .chain(extends)
                .any(|element| self.type_is_generic(element)),
            _ => false,
        }
    }

    /// The checker's `isGenericType`: a type parameter or `infer`
    /// reference, an indexed access, `keyof`, conditional or intrinsic
    /// application over one, a mapped type over a generic source, a template
    /// literal type with a generic hole, a tuple with a generic variadic
    /// element, or a union or intersection holding one. An alias
    /// application over a type variable is the type it names (the checker
    /// instantiates it where it is written): `MessageBase<T>` over a
    /// conditional alias is generic, `Box<T>` over an object type is not;
    /// a builtin utility is generic in the operands its result is generic
    /// in (`NonNullable<T[K]>` is, `Pick<T, "a">` is not). Read from an
    /// explicit stack.
    pub(super) fn type_is_generic(&self, node: SemanticNodeId) -> bool {
        let graph = self.graph();
        let mut pending = vec![node];
        let mut seen: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        while let Some(node) = pending.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if let SemanticNodeData::InstantiationRef { base, args } = &*data {
                let base = base.clone();
                let args = Arc::clone(args);
                drop(data);
                if !args
                    .iter()
                    .any(|arg| self.mentions_free_type_variable(*arg))
                {
                    continue;
                }
                if base.canonical_id.as_ref() == "__builtin__" {
                    // A builtin utility is generic in the operands its result
                    // is generic in: a mapped utility in its key domain
                    // (`Pick<T, "a">` is an object type, `Partial<T>` maps
                    // `keyof T`), every other one (a conditional, an
                    // intersection, a string mapping) in any operand.
                    let generic_in: &[usize] = match base.decl_name.as_ref() {
                        "Pick" => &[1],
                        "Record" => &[0],
                        "Partial" | "Required" | "Readonly" => &[0],
                        _ => &[],
                    };
                    if generic_in.is_empty() {
                        pending.extend(args.iter().copied());
                    } else {
                        pending.extend(generic_in.iter().filter_map(|index| args.get(*index)));
                    }
                    continue;
                }
                match self.alias_application_instantiated(&base, &args) {
                    Some(named) if named == node => return true,
                    Some(named) => pending.push(named),
                    None => {}
                }
                continue;
            }
            match &*data {
                SemanticNodeData::TypeParam { .. } | SemanticNodeData::InferRef { .. } => {
                    return true;
                }
                // A shell the lane keeps where it is written (`Rec["a"]`,
                // `keyof O`) is generic only over a generic operand; the
                // checker resolved it.
                SemanticNodeData::IndexedAccess { object, index } => {
                    pending.push(*object);
                    if let crate::semantic_query::IndexKey::Computed(index) = index {
                        pending.push(*index);
                    }
                }
                SemanticNodeData::KeyOf { base } => pending.push(*base),
                SemanticNodeData::Conditional { check, extends, .. } => {
                    pending.extend([*check, *extends]);
                }
                SemanticNodeData::IntrinsicApplication { args, .. } => {
                    pending.extend(args.iter());
                }
                SemanticNodeData::Alias(inner) => pending.push(*inner),
                SemanticNodeData::Mapped { source, .. } => pending.push(*source),
                SemanticNodeData::Union(members) => pending.extend(members.members_arc().iter()),
                SemanticNodeData::Intersection(members) => {
                    pending.extend(members.members_arc().iter());
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    pending.extend(expressions.iter());
                }
                SemanticNodeData::Tuple { elements, .. } => pending.extend(
                    elements
                        .iter()
                        .filter(|element| element.rest)
                        .map(|element| element.value)
                        .filter(|value| {
                            matches!(
                                graph.node_data(*value).as_deref(),
                                Some(SemanticNodeData::TypeParam { .. })
                            )
                        }),
                ),
                _ => {}
            }
        }
        false
    }

    /// The selection over an `extends` pattern whose `infer` declarations
    /// sit deeper than the relation binds: the false branch when the check
    /// does not relate to the pattern even with every `infer` read as the
    /// permissive wildcard type — the checker's definitely-false test over
    /// the permissive instantiation (`getConditionalType`), which no
    /// inference can turn true (`string extends { then(cb: (v: infer V) =>
    /// void): void }` is false). The wildcard is no `any`: an operation over
    /// it is the wildcard, so `"AB"` against `Uppercase<infer U>` is not
    /// definitely false. Otherwise the checker infers from the check and relates the
    /// inferred pattern, which the lane does not model here: the
    /// conditional is undecided, as is a pattern holding a nested
    /// conditional or mapped type, whose binders scope their own `infer`
    /// declarations.
    fn permissive_conditional_selection(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> ConditionalBranchSelection {
        let scan = self.infer_scan(extends, true);
        if scan.binder_scope || scan.infers.is_empty() {
            return ConditionalBranchSelection::Undecided;
        }
        let wildcard = self
            .graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::PermissiveWildcard));
        let permissive = scan.infers.iter().fold(extends, |pattern, infer| {
            self.substitute_semantic_type_param(pattern, *infer, wildcard)
        });
        if self.subtree_contains_infer(permissive) {
            return ConditionalBranchSelection::Undecided;
        }
        let params = self.type_params_within(&[check, permissive]);
        let step = if params.is_empty() {
            self.wildcard_failure_undecided(
                [check, permissive],
                wildcard,
                self.execute_relate_pair(check, permissive),
            )
        } else {
            self.permissive_relation(check, permissive, &params)
        };
        match step {
            super::dispatch_txn::RelationStep::NotAssignable => ConditionalBranchSelection::False,
            _ => ConditionalBranchSelection::Undecided,
        }
    }

    /// `node` reduced when it is a conditional the conditional query
    /// decides, else `node` itself.
    fn reduced_conditional_operand(&self, node: SemanticNodeId) -> SemanticNodeId {
        let key = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            }) => SemanticQueryKey::Conditional {
                check: *check,
                extends: *extends,
                true_branch: *true_branch_ref,
                false_branch: *false_branch_ref,
                distributive: *distributive,
                pending: pending.clone(),
            },
            _ => return node,
        };
        match crate::semantic_query::SemanticQueryApi::execute_type_node(self, key) {
            QueryResult::Value(output)
                if !matches!(
                    self.graph().node_data(output.value).as_deref(),
                    Some(SemanticNodeData::Conditional { .. })
                ) =>
            {
                output.value
            }
            _ => node,
        }
    }

    /// The infer-routing of a conditional's `extends` pattern: a bare
    /// `infer X`, a pattern whose declarations the relation infers through
    /// (its inventory, or a root reverse mapping), one holding a
    /// declaration below structure the relation does not infer through (an
    /// explicit capability gap), or none.
    fn conditional_infer_route(&self, extends: SemanticNodeId) -> ConditionalInferRoute {
        if matches!(
            self.graph().node_data(extends).as_deref(),
            Some(SemanticNodeData::Infer { .. })
        ) {
            return ConditionalInferRoute::Bare;
        }
        if self.relation_pattern_info(extends).is_some() {
            return ConditionalInferRoute::InScopePattern;
        }
        match self.infer_inventory(extends) {
            super::relation::InferInventory::Sites(_) => ConditionalInferRoute::None,
            super::relation::InferInventory::Unsupported => ConditionalInferRoute::OutOfScope,
        }
    }

    /// `node` read through the conditional query: the type a conditional
    /// reduces to, the conditional the checker keeps (a complete deferred
    /// type, related by the rules for one), or a conditional the query could
    /// not finish. Every consumer that asks which branch a conditional
    /// takes reads it here.
    pub(super) fn conditional_outcome(&self, node: SemanticNodeId) -> ConditionalOutcome {
        let Some(data) = self.graph().node_data(node) else {
            return ConditionalOutcome::NotConditional;
        };
        let SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
            pending,
        } = data.as_ref()
        else {
            return ConditionalOutcome::NotConditional;
        };
        // The procedure keeps a conditional over a naked type parameter
        // whole before it reads anything else (`getConditionalType` defers
        // it first; no operand construction, absorbing row or distribution
        // applies to a type parameter), unless its `extends` type is the
        // error type.
        let kept = matches!(
            self.graph().node_data(*check).as_deref(),
            Some(SemanticNodeData::TypeParam { .. })
        ) && !matches!(
            self.peek_special(*extends),
            Some((super::absorb::SpecialKind::Error, _))
        );
        let key = SemanticQueryKey::Conditional {
            check: *check,
            extends: *extends,
            true_branch: *true_branch_ref,
            false_branch: *false_branch_ref,
            distributive: *distributive,
            pending: pending.clone(),
        };
        drop(data);
        let value = if kept {
            node
        } else {
            let read = self.execute_read(key);
            if read.result_is_partial {
                return ConditionalOutcome::Undecided;
            }
            let QueryResult::Value(value) = read.value else {
                return ConditionalOutcome::Undecided;
            };
            value
        };
        match self.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            }) => ConditionalOutcome::Deferred(DeferredConditional {
                check: *check,
                extends: *extends,
                true_branch: self.apply_conditional_branch_pending(
                    *true_branch_ref,
                    pending.as_deref(),
                    true,
                ),
                false_branch: self.apply_conditional_branch_pending(
                    *false_branch_ref,
                    pending.as_deref(),
                    false,
                ),
                distributive: *distributive,
            }),
            _ => ConditionalOutcome::Reduced(value),
        }
    }
}
