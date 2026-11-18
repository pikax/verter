//! The conditional query's decision procedure (`getConditionalType`), the
//! one owner of how a conditional type is decided: its operands as the
//! checker constructs them, the absorbing lattice rows, distribution over a
//! union check, the permissive and restrictive tests and the relation that
//! select a branch, and the conditional kept whole where none decides it.
//!
//! A conditional read from a query key and one read from syntax take the
//! same procedure; lowering supplies only the branches it demands
//! ([`DecisionBranches`]). A consumer asking which branch a
//! conditional takes (a relation, a closedness walk) reads the query's
//! typed outcome ([`ProjectSemanticDispatch::conditional_outcome`]).

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::{ConditionalBranchSelection, ProjectSemanticDispatch};
use crate::semantic_query::surface_resolution::NonEmptyReasons;
use crate::semantic_query::{
    BranchSelection, ConditionalPendingSubstitution, ExecutionAbort, FactResult, LiteralValue,
    OriginEdgeKind, OriginMeta, PartialReason, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey,
};

/// Whether `data` stores a child id: a payload with none is a leaf of every
/// structural walk (a summary, an `infer` scan) and is read on its own.
pub(super) fn has_children(data: &SemanticNodeData) -> bool {
    let mut found = false;
    let _ = data.for_each_child(|_| found = true);
    found
}

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static OPERAND_WALK_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OPERAND_PREPARATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DISTRIBUTED_MEMBERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static BRANCH_SELECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static IN_PLACE_ANSWERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static MEMBER_QUERIES_FORCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Count one node a conditional's operand classification read
/// ([`ProjectSemanticDispatch::type_is_generic`], the `infer` inventory, the
/// lattice-extreme probe).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn note_operand_walk_visit() {
    OPERAND_WALK_VISITS.with(|count| count.set(count.get() + 1));
}

/// The free type variables below one semantic node, binder-aware: the type
/// parameters written below it that no generic signature within declares
/// over the position they are written in, and whether an `infer` reference
/// or declaration sits below it. A node's summary is a function of its immutable payload
/// and its children's summaries, so it holds for as long as the node id
/// does.
#[derive(Clone, Default)]
pub(super) struct FreeTypeVariables {
    /// The free type parameters in first-reached order, each once; `None`
    /// when there are none, so a leaf allocates nothing.
    params: Option<Arc<[SemanticNodeId]>>,
    /// Whether an `InferRef` sits below, the constraints and defaults of the
    /// type parameters written there included.
    infer_ref: bool,
    /// Infer declaration presence, used only where an operand reads the
    /// declaration as a still-unbound variable rather than a pattern site.
    infer_declaration: bool,
}

/// The dispatcher's summaries of [`FreeTypeVariables`] by node, kept for
/// the dispatcher's lifetime (one request) and dropped with it. Node ids
/// are append-only and the dispatcher asks only about nodes it keeps
/// rooted, whose payloads therefore do not change while it holds them, so
/// a kept summary does not go stale. A node whose last owner releases it
/// reads as the shared placeholder afterwards; its id is never asked about
/// again by a dispatcher that did not root it. The summaries are request
/// state: a semantic task the dispatcher lends its state to inherits them
/// and whatever it composes comes back to the lender with the rest of the
/// state ([`ProjectSemanticDispatch::exchange_request_state`]), so reuse
/// spans the task lifecycle instead of restarting per drive.
pub(super) type FreeTypeVariableSummaries =
    rustc_hash::FxHashMap<SemanticNodeId, FreeTypeVariables>;

/// The ordered union of some summaries' free type parameters. A union
/// that one summary's set already covers shares that set rather than
/// copying it, so a chain of wrappers over one parameter holds one set.
#[derive(Default)]
struct ParamUnion {
    shared: Option<Arc<[SemanticNodeId]>>,
    merged: Option<(Vec<SemanticNodeId>, FxHashSet<SemanticNodeId>)>,
}

impl ParamUnion {
    fn add(&mut self, params: &Option<Arc<[SemanticNodeId]>>) {
        let Some(params) = params else {
            return;
        };
        if let Some((order, members)) = &mut self.merged {
            for param in params.iter() {
                if members.insert(*param) {
                    order.push(*param);
                }
            }
            return;
        }
        let Some(shared) = &self.shared else {
            self.shared = Some(Arc::clone(params));
            return;
        };
        // A short set is checked by scanning; a long one merges by hashing.
        const SCAN_LIMIT: usize = 8;
        if Arc::ptr_eq(shared, params)
            || (shared.len() <= SCAN_LIMIT
                && params.len() <= shared.len()
                && params.iter().all(|param| shared.contains(param)))
        {
            return;
        }
        let mut order = shared.to_vec();
        let mut members: FxHashSet<SemanticNodeId> = order.iter().copied().collect();
        for param in params.iter() {
            if members.insert(*param) {
                order.push(*param);
            }
        }
        self.shared = None;
        self.merged = Some((order, members));
    }

    fn finish(self) -> Option<Arc<[SemanticNodeId]>> {
        match self.merged {
            Some((order, _)) => Some(Arc::from(order)),
            None => self.shared,
        }
    }

    /// The union without the parameters `declared` binds. A short
    /// declaration list is checked by scanning; a long one is indexed, so a
    /// wide generic signature costs work linear in its parameters.
    fn finish_without(
        self,
        declared: impl ExactSizeIterator<Item = SemanticNodeId> + Clone,
    ) -> Option<Arc<[SemanticNodeId]>> {
        const SCAN_LIMIT: usize = 8;
        let params = self.finish()?;
        if declared.len() == 0 {
            return Some(params);
        }
        let indexed: Option<FxHashSet<SemanticNodeId>> =
            (declared.len() > SCAN_LIMIT).then(|| declared.clone().collect());
        let binds = |param: &SemanticNodeId| match &indexed {
            Some(indexed) => indexed.contains(param),
            None => declared.clone().any(|declared| declared == *param),
        };
        if !params.iter().any(binds) {
            return Some(params);
        }
        let free: Vec<SemanticNodeId> = params.iter().copied().filter(|p| !binds(p)).collect();
        (!free.is_empty()).then(|| Arc::from(free))
    }
}

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
///
/// Every arm is a COMPLETE reading: a conditional the checker keeps whole
/// ([`Self::Deferred`]) is an exact answer, not a failure. A conditional
/// the query could not finish is not a reading at all — it is the
/// unavailable arm of the [`ConditionalFact`] the query establishes.
pub(super) enum ConditionalReading {
    /// Not a conditional.
    NotConditional,
    /// A conditional that reduced to this type.
    Reduced(SemanticNodeId),
    /// A conditional the checker keeps.
    Deferred(DeferredConditional),
}

/// The fact a conditional read establishes: a complete
/// [`ConditionalReading`], or the unavailable arm naming why the query
/// could not finish it ([`PartialReason::UndecidedConditional`] plus the
/// read's own classes or the query error's classification). A partial read
/// is never an approximate reading — a branch selection made over an
/// unfinished conditional has no documented usable interpretation. A read
/// that observed cancellation or a superseded view aborts
/// ([`ExecutionAbort`]).
///
/// [`PartialReason::UndecidedConditional`]: crate::semantic_query::PartialReason::UndecidedConditional
pub(super) type ConditionalFact = Result<FactResult<ConditionalReading>, ExecutionAbort>;

/// The fact one conditional QUERY read establishes about the type it
/// reduces to: the complete value, or unavailable naming
/// [`PartialReason::UndecidedConditional`] with the read's own classes, its
/// error's classification, or same-path recursion. A read that observed
/// cancellation or a superseded view aborts. Never approximate: a partial
/// read's value is discarded, not offered as a reading.
pub(super) fn conditional_read_fact(
    read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
) -> Result<FactResult<SemanticNodeId>, ExecutionAbort> {
    let undecided = NonEmptyReasons::of(PartialReason::UndecidedConditional);
    if let QueryResult::Error(error) = &read.value {
        if let Some(abort) = ExecutionAbort::from_query_error(error) {
            return Err(abort);
        }
    }
    if read.result_is_partial() {
        let classes = read.partial_reason_classes();
        return Ok(FactResult::unavailable(undecided.with(classes)));
    }
    Ok(match read.value {
        QueryResult::Value(value) => FactResult::complete(value),
        QueryResult::Error(error) => {
            FactResult::unavailable(undecided.union(NonEmptyReasons::from_query_error(&error)))
        }
        QueryResult::Recursive(_) => FactResult::unavailable(
            undecided.union(NonEmptyReasons::of(PartialReason::SamePathRecursion)),
        ),
    })
}

impl ConditionalReading {
    /// The deferred conditional this operand is, if it is one.
    pub(super) fn into_deferred(self) -> Option<DeferredConditional> {
        match self {
            Self::Deferred(conditional) => Some(conditional),
            Self::NotConditional | Self::Reduced(_) => None,
        }
    }
}

/// The exact reading of a conditional fact: `None` for an unfinished or
/// aborted read.
pub(super) fn exact_reading(fact: ConditionalFact) -> Option<ConditionalReading> {
    fact.ok().and_then(FactResult::into_exact)
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
#[derive(Clone, Copy)]
struct ConditionalOperands {
    /// The check the selected branch binds, as written.
    check: SemanticNodeId,
    /// The type a declared check names, which distribution reads.
    declared_check: SemanticNodeId,
    /// The check the absorbing lattice rows read.
    absorbed_check: SemanticNodeId,
    extends: SemanticNodeId,
    /// Every tuple position was exact or its own typed checker recovery.
    checker_recovery: bool,
}

/// Completed operand preparation. Its observations have already joined the
/// owning frame, so a later relation wait retains both the values and their
/// evidence without executing the preparation again.
#[derive(Clone, Copy)]
struct PreparedConditionalOperands {
    demanded_check: SemanticNodeId,
    operands: ConditionalOperands,
}

/// A completed relation/inference stage. Its observations have joined the
/// owning frame before the selected branch can demand another query.
#[derive(Clone)]
struct ConditionalSelection {
    branch: ConditionalBranchSelection,
    infer: Option<super::relation::RelationInferBindings>,
    false_recovery: bool,
}

/// A distribution owns its completed prefix, including the evidence that
/// distinguishes a positional answer from an incomplete member. Its next
/// position is the length of `per_member`; an unanswered position is never
/// appended or folded into the accumulated output.
struct ConditionalDistribution {
    members: Arc<[SemanticNodeId]>,
    per_member: Vec<SemanticNodeId>,
    /// The final query's canonical operand list, prepared once before its
    /// claim can suspend. Both drivers use the same family identity.
    normalization_members: Option<Arc<[SemanticNodeId]>>,
    output: super::walk::QueryBuildOutput,
    positions_complete: bool,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Both operands keep their generic declaration and arguments until the
    /// relation authority applies variance or demands their structure.
    pub(super) fn conditional_operand_context(
        context: ProjectionReductionContext,
    ) -> ProjectionReductionContext {
        let mode = if context.mode == ProjectionMode::Skeleton {
            ProjectionMode::Skeleton
        } else {
            ProjectionMode::Navigate
        };
        // The check consumes its selected value: an unread indexed carrier
        // cannot prove a reflexive relation merely by node identity.
        context.with_mode(mode)
    }

    /// An extends pattern keeps its operators structural until the relation
    /// demands them, without publishing nested mapped members.
    pub(super) fn conditional_target_context(
        context: ProjectionReductionContext,
    ) -> ProjectionReductionContext {
        let context = Self::conditional_operand_context(context);
        context.into_structural_transit_with_mode(context.mode)
    }

    /// Nodes this thread's conditional decisions read to classify their
    /// operands (generic, `infer` sites); test-only. A distributed member
    /// that re-reads its wide `extends` operand shows here.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conditional_operand_walk_visits_for_tests() -> usize {
        OPERAND_WALK_VISITS.with(std::cell::Cell::get)
    }

    /// Operand preparations executed on this thread, including preparations
    /// that later discover an unanswered dependency.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conditional_operand_preparations_for_tests() -> usize {
        OPERAND_PREPARATIONS.with(std::cell::Cell::get)
    }

    /// Completed distributed members folded on this thread. Count at the
    /// production fold, so retaining memo answers alone cannot hide a replay.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conditional_distributed_members_for_tests() -> usize {
        DISTRIBUTED_MEMBERS.with(std::cell::Cell::get)
    }

    /// Entries to the relation/inference stage, including attempts that
    /// suspend. Completed selections must not repeat on a winner's wait.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conditional_branch_selections_for_tests() -> usize {
        BRANCH_SELECTIONS.with(std::cell::Cell::get)
    }

    /// Reads this thread's conditional decisions answered in place through
    /// an owner that exposes no owned read; test-only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conditional_in_place_answers_for_tests() -> usize {
        IN_PLACE_ANSWERS.with(std::cell::Cell::get)
    }

    construction_ordinal_seam! {
        /// Decide a materialized conditional through the same family
        /// continuation a polled query uses. The decision owns absorption,
        /// distribution, relation selection and winner commitment.
        fn build_conditional(
            &self,
            check: SemanticNodeId,
            extends: SemanticNodeId,
            true_branch: SemanticNodeId,
            false_branch: SemanticNodeId,
            distributive: bool,
            pending: Option<
                verter_session_query::retention::owned::Owned<ConditionalPendingSubstitution>,
            >,
        ) -> super::walk::QueryBuildOutput {
            let key = SemanticQueryKey::Conditional {
                check,
                extends,
                true_branch,
                false_branch,
                distributive,
                pending,
            };
            self.drive_family_in_place(
                self.begin_family_build(&key)
                    .expect("a conditional builds as a continuation"),
            )
        }
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
        let mut decision = self.begin_conditional_decision(
            check,
            extends,
            distributive,
            DecisionBranches::Lowered,
            super::helper_continuation::HelperDrive::InPlace,
        );
        self.drive_conditional_in_place(&mut decision, lower_branch)
    }

    /// A check no lattice row absorbs and no distribution splits, proven
    /// positively: the type it completely names (an alias or an
    /// instantiation read through, `Id<any>` being `any`) is a non-extreme
    /// primitive, a literal, an object, an array or a signature. A
    /// reference to an interface or a class names an object type. Every
    /// other check — a union, an intersection (`1 & Id<any>` is `any`), an
    /// indexed access, a `keyof`, a conditional, a template, a tuple, or a
    /// name the lane cannot read completely — is not proven plain. Boolean
    /// is plain only when the conditional is non-distributive.
    ///
    /// This half reads the check's declaration only; a name it must resolve
    /// is the structural demand its operation holds, and
    /// [`Self::plain_check_is_plain`] classifies what that demand resolved.
    fn plain_check(&self, check: SemanticNodeId) -> PlainCheck {
        match self.graph().node_data(check).as_deref() {
            Some(
                SemanticNodeData::DeclRef { identity }
                | SemanticNodeData::InstantiationRef { base: identity, .. },
            ) if matches!(
                self.prepared_decl_kind(identity),
                Some(
                    verter_session_query::declarations::TypeDeclKind::Interface
                        | verter_session_query::declarations::TypeDeclKind::Class
                )
            ) =>
            {
                PlainCheck::Proven
            }
            _ if self.is_named_conditional_operand(check) => PlainCheck::Resolve,
            _ => PlainCheck::Named(check),
        }
    }

    /// Whether `operand` is written as a name — an alias, a declaration or
    /// application reference, a `typeof`, an import type or an unresolved
    /// declaration placeholder — that the structural demand reads through.
    fn is_named_conditional_operand(&self, operand: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(operand).as_deref(),
            Some(
                SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::TypeOf(_)
                    | SemanticNodeData::BareRef(_)
                    | SemanticNodeData::ImportType(_)
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
            )
        )
    }

    /// The classification half of [`Self::plain_check`]: whether the type
    /// the check names is plain.
    fn plain_check_is_plain(&self, named: SemanticNodeId, distributive: bool) -> bool {
        self.peek_special(named).is_none()
            && match self.graph().node_data(named).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => !distributive,
                Some(
                    SemanticNodeData::Primitive(_)
                    | SemanticNodeData::Literal(_)
                    | SemanticNodeData::EnumLiteral(_)
                    | SemanticNodeData::Object(_)
                    | SemanticNodeData::Array { .. }
                    | SemanticNodeData::Signature { .. },
                ) => true,
                _ => false,
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
        pending: Option<
            verter_session_query::retention::owned::Owned<ConditionalPendingSubstitution>,
        >,
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
        crate::semantic_query_memo::record_origin_edge!(
            self.graph(),
            node,
            OriginEdgeKind::ConditionalSelect,
            Arc::from([check, extends]),
            OriginMeta::Branch(BranchSelection::Deferred),
            fence.clone(),
        );
        self.graph().record_conditional_deferred();
        let mut output = super::walk::QueryBuildOutput::from((QueryResult::Value(node), fence))
            .with_observed_self_roots(suspended_roots)
            .with_positional_guarantee();
        // A conditional the checker decides and the lane could not is a
        // typed gap: the shell stands where the checker's answer should be,
        // so it is never published complete or warm-admitted.
        if selection == ConditionalBranchSelection::Undecided {
            output.mark_partial();
            output.cache_suppress = true;
            output.add_partial_reasons(
                crate::semantic_query::PartialReasonSet::UNDECIDED_CONDITIONAL,
            );
        }
        output
    }

    /// The structural demand that resolves a conditional operand: an
    /// indexed operand consumed by the conditional needs its value, rather
    /// than a shallow apparent surface that merges union arms.
    fn resolved_operand_demand(&self, operand: SemanticNodeId) -> FactDemand {
        let context = if matches!(
            self.graph().node_data(operand).as_deref(),
            Some(SemanticNodeData::IndexedAccess { .. })
        ) {
            ProjectionReductionContext::published(ProjectionMode::Expanded)
        } else {
            ProjectionReductionContext::structural_transit()
        };
        FactDemand::Demand(operand, context)
    }

    /// What [`Self::resolved_operand_demand`] resolved, as operand evidence:
    /// exact evidence, or the producer's own checker recovery to propagate
    /// unchanged. A positional approximation cannot establish a lattice
    /// extreme, a selected branch, or an exhaustive union domain.
    fn resolved_operand_of(
        &self,
        outcome: super::evaluate::StructuralFactDemandOutcome,
    ) -> Option<SemanticNodeId> {
        match outcome {
            super::evaluate::StructuralFactDemandOutcome::Complete(node) => Some(node),
            super::evaluate::StructuralFactDemandOutcome::Recovered { node, .. }
                if self.is_conditional_checker_recovery(node) =>
            {
                Some(node)
            }
            _ => None,
        }
    }

    /// Whether a distributed member is decided in place rather than as a
    /// conditional query of its own, given an `extends` operand that is a
    /// union of literals alone ([`Self::is_literal_only_union`]): a literal
    /// member, as syntax-lowered members always are. The choice reads the
    /// operands' shapes only, never what an earlier read prepared, so a
    /// distribution decides its members the same way in every run. Such a
    /// decision reads the two operands alone — the relation is a membership
    /// lookup — and binds the selected branch by structural substitution,
    /// which evaluates nothing (its only query builds a template whose holes
    /// are closed literals), so it reaches no query that could re-enter it.
    /// It runs the same decision with the same reads as the member's query,
    /// charged and refused as that query's cold entry would be
    /// ([`ProjectSemanticDispatch::enter_in_place_query`]); a memo entry for
    /// each member of a wide union
    /// would be read only by another distribution over the same member, which
    /// decides it in place as cheaply.
    fn decides_member_in_place(&self, member: SemanticNodeId) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        if MEMBER_QUERIES_FORCED.get() {
            return false;
        }
        matches!(
            self.graph().node_data(member).as_deref(),
            Some(SemanticNodeData::Literal(_))
        )
    }

    /// Test-only: every distributed member is decided as its own
    /// conditional query on this thread while the guard lives, so a test
    /// can compare the in-place decision with the query it stands for.
    #[cfg(any(test, feature = "test-support"))]
    pub fn force_member_queries_for_tests() -> impl Drop {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                MEMBER_QUERIES_FORCED.set(self.0);
            }
        }
        Restore(MEMBER_QUERIES_FORCED.replace(true))
    }

    /// Whether `node` is a union whose every arm is a literal.
    fn is_literal_only_union(&self, node: SemanticNodeId) -> bool {
        let graph = self.graph();
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Union(arms)) => arms.iter().all(|arm| {
                matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Literal(_))
                )
            }),
            _ => false,
        }
    }

    pub(super) fn is_conditional_checker_recovery(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
        )
    }

    /// A conditional query's read as the output a decision folds in.
    pub(super) fn conditional_read_output(
        &self,
        read: crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
    ) -> super::walk::QueryBuildOutput {
        // Conditional branches and their union normalization already carry
        // a bounded positional representation. Forward the adopted value,
        // including its gaps, rather than discarding its producer guarantee.
        let approximation = read.fact_status.interpretation();
        let mut output = super::walk::QueryBuildOutput::from((read.value, read.dep_signature));
        output.cache_suppress = read.cache_suppress;
        output.merge_completeness(read.fact_status.completeness());
        output.approximation = approximation;
        output
    }

    /// Classify a distribution once. Materialized and lowering-time entries
    /// use the same ordered member walk; only their branch supplier differs.
    fn begin_conditional_distribution(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        (resolved_check, members): (SemanticNodeId, Arc<[SemanticNodeId]>),
    ) -> Box<ConditionalDistribution> {
        let output = super::walk::QueryBuildOutput::from((
            QueryResult::Error(QueryError::Miss),
            self.project_generation_signature(),
        ))
        .with_observed_self_roots(self.observed_self_roots_from_nodes([
            check,
            resolved_check,
            extends,
        ]));
        Box::new(ConditionalDistribution {
            per_member: Vec::with_capacity(members.len()),
            members,
            normalization_members: None,
            output,
            positions_complete: true,
        })
    }

    /// The resolved check and its union members, or `None` when the check
    /// does not distribute; [`DistributionCheck::Resolve`] when the check is
    /// a carrier the structural demand must resolve first, whose complete
    /// answer [`Self::distribution_members_of`] then reads. Retain the
    /// resolved node so its file scope can root the result even when the
    /// input is a global alias.
    ///
    /// Distribution semantics follow the check's INSTANTIATED surface,
    /// not the check node's raw shape: a naked-type-param check whose
    /// binding substituted an `Alias` / `DeclRef` / `InstantiationRef`
    /// CARRIER of a union (the defaulted-union type argument
    /// `T extends SingleOrMultiple = SingleOrMultiple` of reka-ui's
    /// `AccordionRootEmits`) distributes exactly like a raw `Union`
    /// binding. `boolean` contributes its two literals, even inside a union;
    /// unions without `boolean` retain their member slice without allocation. The
    /// carrier shapes resolve through the shared structural-fact demand
    /// (the same `ResolveDecl` / `Instantiate` delegation the relation
    /// engine's own demand-resolve uses, under the relation engine's
    /// `StructuralTransit` context so the union-ness read never reifies
    /// publication member edges), held by the operation that classifies.
    ///
    /// Fail-closed on BOTH rails: a `Partial` demand (budget / cycle /
    /// fault — the demand folds the reasons into the active build
    /// taint frame) and a resolved surface other than a union or `boolean` return `None`,
    /// deferring to the tri-state oracle path. `TypeParam` / `Infer`
    /// shells are STABLE STOPS of the deferred evaluator, so an OPEN
    /// generic check never distributes over its constraint or default —
    /// open conditionals stay deferred shells.
    fn distributive_check(&self, check: SemanticNodeId) -> DistributionCheck {
        if let Some(surface) = self.distribution_members_of(check) {
            return DistributionCheck::Decided(Some(surface));
        }
        // A `keyof` over a type that is no generic object type is the union
        // of its keys (`getIndexType` reduces it eagerly), which the check
        // distributes over: a type holding no type parameter, and a
        // reference to an interface or a class whatever its type arguments
        // — its members are statically known, so `keyof O<S>` over
        // `interface O<S> { id: 1; s?: S }` is `"id" | "s"` while `S` is
        // still open (`Omit<O<S>, "id">` keeps `s`).
        let closed_key_of = match self.graph().node_data(check).as_deref() {
            Some(SemanticNodeData::KeyOf { base })
                if !self.mentions_binder(*base) || self.names_object_declaration(*base) =>
            {
                Some(*base)
            }
            _ => None,
        };
        if let Some(base) = closed_key_of {
            return DistributionCheck::KeySet(base);
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
        // An interface or a class names an object type, never a union:
        // its reference has nothing to distribute over, so it is not
        // resolved here.
        if !is_resolvable_shell || self.names_object_declaration(check) {
            return DistributionCheck::Decided(None);
        }
        DistributionCheck::Resolve
    }

    /// `node` as the members a distribution splits it into: a union's
    /// members, with `boolean` contributing its two literals, or `boolean`
    /// alone. `None` for every other shape.
    fn distribution_members_of(
        &self,
        node: SemanticNodeId,
    ) -> Option<(SemanticNodeId, Arc<[SemanticNodeId]>)> {
        let is_boolean = |node: SemanticNodeId| {
            matches!(
                self.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
            )
        };
        let boolean_members = || {
            [true, false].map(|value| {
                self.graph()
                    .intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(value)))
            })
        };
        self.graph().node_data(node).and_then(|data| match &*data {
            SemanticNodeData::Primitive(PrimitiveKind::Boolean) => {
                Some((node, Arc::from(boolean_members())))
            }
            SemanticNodeData::Union(members) => {
                let Some(first_boolean) = members.iter().position(|&member| is_boolean(member))
                else {
                    return Some((node, members.members_arc()));
                };
                let mut split = Vec::with_capacity(members.len() + 1);
                split.extend(members.iter().take(first_boolean).copied());
                for &member in members.iter().skip(first_boolean) {
                    if is_boolean(member) {
                        split.extend(boolean_members());
                    } else {
                        split.push(member);
                    }
                }
                Some((node, Arc::from(split)))
            }
            _ => None,
        })
    }

    /// Whether `node` is a reference to an interface or a class: an object
    /// type whose members are statically known whatever its type
    /// arguments, never a union.
    fn names_object_declaration(&self, node: SemanticNodeId) -> bool {
        match self.graph().node_data(node).as_deref() {
            Some(
                SemanticNodeData::DeclRef { identity }
                | SemanticNodeData::InstantiationRef { base: identity, .. },
            ) => matches!(
                self.prepared_decl_kind(identity),
                Some(
                    verter_session_query::declarations::TypeDeclKind::Interface
                        | verter_session_query::declarations::TypeDeclKind::Class
                )
            ),
            _ => false,
        }
    }

    /// The type a conditional operand denotes when it is a reference to a
    /// declaration: a type alias is the type it names, so `IsStr<U2>` with
    /// `type U2 = "a" | 1` distributes over `"a"` and `1`, `IsStr<Nv>` with
    /// `type Nv = never` is `never`, and `any extends Un` with `type Un =
    /// unknown` selects its true branch alone (the checker's operand is the
    /// resolved type, never the name). A reference the structural demand
    /// cannot resolve completely, and every other operand, is itself.
    ///
    /// Whether `node` is such a reference to a type alias: the operation
    /// that reads the operand then holds its structural demand
    /// ([`Self::resolved_operand_demand`]) and keeps `node` where the demand
    /// resolves nothing.
    fn declared_operand_resolves(&self, node: SemanticNodeId) -> bool {
        let identity = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => identity.clone(),
            Some(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            })) => crate::semantic_query::DeclIdentity {
                canonical_id: canonical_id.clone(),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: name.clone(),
            },
            _ => return false,
        };
        // An interface or a class names an object type, which no lattice
        // row or distribution reads.
        matches!(
            self.prepared_decl_kind(&identity),
            Some(verter_session_query::declarations::TypeDeclKind::Alias)
        )
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
    /// that relation is the check. So it is when every inference its
    /// constraint admits stays as inferred: an implied constraint (a
    /// template literal hole's `string`) fixes nothing a capture already
    /// satisfies, and the pattern instantiated with the inferences is the
    /// pattern the inferring relation related. Only a pattern that repeats
    /// an `infer`, or an inference its constraint replaced, is related
    /// again.
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
        let (bindings, fixed_to_constraint) = if constraints.is_empty() {
            (bindings, false)
        } else {
            match self.infer_bindings_within_constraints(&bindings, &constraints) {
                Some(fixed) => fixed,
                None => return (ConditionalBranchSelection::Undecided, None),
            }
        };
        if !fixed_to_constraint && !self.pattern_repeats_an_infer(extends) {
            return (
                ConditionalBranchSelection::True,
                Some(super::relation::RelationInferBindings { bindings }),
            );
        }
        let instantiated = bindings.iter().fold(extends, |node, binding| {
            self.substitute_semantic_type_param(node, binding.param, binding.bound)
        });
        match self.execute_relate_pair(check, instantiated) {
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
            verter_session_query::flow::policy::NullabilityPolicy::Strict
        } else {
            verter_session_query::flow::policy::NullabilityPolicy::Erased
        };
        let constructed = self.intern_normalized_union(&members, nullability);
        let same_members = match self.graph().node_data(constructed).as_deref() {
            Some(SemanticNodeData::Union(kept)) => {
                let kept = kept.members_arc();
                kept.len() == members.len() && super::evaluate::holds_every(&members, &kept)
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
            self.execute_relate_pair(check, extends),
        )
    }

    /// `step`, a relation over a permissive instantiation, except that its
    /// failure is undecided where an operand holds an operation over the
    /// wildcard (`Uppercase<W>` and every other builtin utility, `keyof W`, a
    /// template hole, a conditional over it): the checker's instantiation
    /// turns such an operation into the wildcard itself, which relates to
    /// everything, and the lane reads the operation as written. An access of
    /// the wildcard is already the wildcard (substitution folds it).
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
                // A builtin utility is an operation too (`Uppercase<W>`); a
                // runtime nominal (`Promise<W>`) is a reference to the global
                // interface, which the wildcard instantiates like any other.
                SemanticNodeData::InstantiationRef { base, .. } => {
                    base.canonical_id.as_ref() == "__builtin__"
                        && self.runtime_nominal_identity(base).is_none()
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
        self.execute_relate_pair(instantiate(check), instantiate(extends))
    }

    /// Whether `node` holds a type variable free in it: a type parameter no
    /// signature within declares, or a reference to an enclosing `infer`.
    pub(super) fn mentions_generic_operand_variable(
        &self,
        node: SemanticNodeId,
        infer_operand: bool,
    ) -> bool {
        let summary = self.free_type_variables(node);
        summary.params.is_some()
            || summary.infer_ref
            || (infer_operand && summary.infer_declaration)
    }

    /// The free type parameters `roots` hold anywhere below them — not
    /// those a generic signature declares within its own scope
    /// (instantiating a signature maps its own type parameters to fresh
    /// ones) — in first-reached order, each once.
    pub(super) fn type_params_within(&self, roots: &[SemanticNodeId]) -> Vec<SemanticNodeId> {
        let mut union = ParamUnion::default();
        for root in roots {
            union.add(&self.free_type_variables(*root).params);
        }
        union
            .finish()
            .map_or_else(Vec::new, |params| params.to_vec())
    }

    /// The binder-aware summary of the free type variables below `root`
    /// ([`FreeTypeVariables`]). Each node's summary is composed once from
    /// its children's and kept for the dispatcher's lifetime, so a type
    /// that grows by one wrapper per recursive step costs one new summary
    /// per step, never a walk of everything below it.
    fn free_type_variables(&self, root: SemanticNodeId) -> FreeTypeVariables {
        if let Some(found) = self.free_type_variables.borrow().get(&root) {
            return found.clone();
        }
        let graph = self.graph();
        // A node with no children is summarised from its own payload, without
        // the traversal's containers.
        if let Some(data) = graph.node_data(root) {
            if !has_children(&data) {
                let summary = FreeTypeVariables {
                    params: matches!(data.as_ref(), SemanticNodeData::TypeParam { .. })
                        .then(|| Arc::from([root])),
                    infer_ref: matches!(data.as_ref(), SemanticNodeData::InferRef { .. }),
                    infer_declaration: matches!(data.as_ref(), SemanticNodeData::Infer { .. }),
                };
                self.free_type_variables
                    .borrow_mut()
                    .insert(root, summary.clone());
                return summary;
            }
        }
        let mut computed: rustc_hash::FxHashMap<SemanticNodeId, FreeTypeVariables> =
            rustc_hash::FxHashMap::default();
        let mut on_path: FxHashSet<SemanticNodeId> = FxHashSet::default();
        // A node reached again while it is still being composed closes a
        // cycle: what the nodes on it see of each other is incomplete, so
        // nothing composed by this call is kept.
        let mut cyclic = false;
        let mut work = vec![(root, false)];
        let mut children = Vec::new();
        while let Some((node, finish)) = work.pop() {
            if computed.contains_key(&node) {
                continue;
            }
            if !finish {
                if let Some(found) = self.free_type_variables.borrow().get(&node) {
                    computed.insert(node, found.clone());
                    continue;
                }
                if !on_path.insert(node) {
                    cyclic = true;
                    continue;
                }
                work.push((node, true));
                if let Some(data) = graph.node_data(node) {
                    let _ = data.for_each_child(|child| work.push((child, false)));
                }
                continue;
            }
            on_path.remove(&node);
            let summary = match graph.node_data(node) {
                None => FreeTypeVariables::default(),
                Some(data) => {
                    children.clear();
                    let _ = data.for_each_child(|child| children.push(child));
                    let mut infer_ref = matches!(data.as_ref(), SemanticNodeData::InferRef { .. });
                    let mut infer_declaration =
                        matches!(data.as_ref(), SemanticNodeData::Infer { .. });
                    let mut params = ParamUnion::default();
                    for child in &children {
                        if let Some(below) = computed.get(child) {
                            infer_ref |= below.infer_ref;
                            infer_declaration |= below.infer_declaration;
                            params.add(&below.params);
                        }
                    }
                    let params = match data.as_ref() {
                        // A type parameter is free where it is written; its
                        // constraint and default are read where it is
                        // declared.
                        SemanticNodeData::TypeParam { .. } => Some(Arc::from([node])),
                        SemanticNodeData::Signature {
                            type_parameters, ..
                        } => params
                            .finish_without(type_parameters.iter().map(|declared| declared.param)),
                        _ => params.finish(),
                    };
                    FreeTypeVariables {
                        params,
                        infer_ref,
                        infer_declaration,
                    }
                }
            };
            computed.insert(node, summary);
        }
        let summary = computed
            .get(&root)
            .cloned()
            .expect("the traversal composes the root's summary before it ends");
        if !cyclic {
            self.free_type_variables.borrow_mut().extend(computed);
        }
        summary
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
        self.type_is_generic_operand(node, false)
    }

    /// An infer declaration substituted into a conditional's check acts as
    /// an unbound type variable. At its declaration site in `extends` it is
    /// instead a pattern to infer, so genericness must retain that position.
    fn type_is_generic_operand(&self, node: SemanticNodeId, infer_operand: bool) -> bool {
        let mut walk = GenericWalk::new(node, infer_operand);
        let mut step = self.advance_generic_walk(&mut walk, None);
        loop {
            match step {
                GenericStep::Done(generic) => return generic,
                GenericStep::Read(key) => {
                    let mut key = Some(key);
                    step = self.advance_generic_walk(
                        &mut walk,
                        Some(&mut || self.execute_read(key.take().expect("one genericness read"))),
                    );
                }
            }
        }
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
        match self.infer_inventory(extends).as_ref() {
            super::relation::InferInventory::Sites(_) => ConditionalInferRoute::None,
            super::relation::InferInventory::Unsupported => ConditionalInferRoute::OutOfScope,
        }
    }

    /// `node` read through the conditional query: the type a conditional
    /// reduces to, the conditional the checker keeps (a complete deferred
    /// type, related by the rules for one), or a conditional the query could
    /// not finish. Every consumer that asks which branch a conditional
    /// takes reads it here.
    pub(super) fn conditional_outcome(&self, node: SemanticNodeId) -> ConditionalFact {
        let Some(data) = self.graph().node_data(node) else {
            return Ok(FactResult::complete(ConditionalReading::NotConditional));
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
            return Ok(FactResult::complete(ConditionalReading::NotConditional));
        };
        // The procedure keeps a conditional over a naked type parameter
        // whole before it reads anything else (`getConditionalType` defers
        // it first; no operand construction, absorbing row or distribution
        // applies to a type parameter), unless its `extends` type is the
        // error type or permissive wildcard.
        let kept = matches!(
            self.graph().node_data(*check).as_deref(),
            Some(SemanticNodeData::TypeParam { .. })
        ) && !matches!(
            self.peek_special(*extends),
            Some((
                super::absorb::SpecialKind::Error | super::absorb::SpecialKind::Wildcard,
                _
            ))
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
            match conditional_read_fact(self.execute_read(key))? {
                FactResult::Complete(value) => value,
                FactResult::Unavailable { causes } => return Ok(FactResult::unavailable(causes)),
                FactResult::Approximate { causes, .. } => {
                    return Ok(FactResult::unavailable(causes))
                }
            }
        };
        let reading = match self.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            }) => ConditionalReading::Deferred(DeferredConditional {
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
            _ => ConditionalReading::Reduced(value),
        };
        Ok(FactResult::complete(reading))
    }
}

#[cfg(test)]
#[path = "conditional_decision_tests.rs"]
mod conditional_decision_tests;

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    fn alias_application_key(
        &self,
        base: &crate::semantic_query::DeclIdentity,
        args: &Arc<[SemanticNodeId]>,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            self.type_slot_for(
                base.canonical_id.clone(),
                base.owner,
                base.decl_name.clone(),
            ),
            Arc::clone(args),
            self.instantiate_context_for(
                &base.canonical_id,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            ),
        ))
    }
    fn select_non_generic_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> (
        ConditionalBranchSelection,
        Option<super::relation::RelationInferBindings>,
    ) {
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
            // Tuple structure gives `any` no element inference candidates;
            // the relation fixes each site to its default/constraint. Other
            // patterns can expose naked inference sites (for example a
            // union) and retain their existing unavailable outcome here.
            if !matches!(
                self.graph().node_data(extends).as_deref(),
                Some(SemanticNodeData::Tuple { .. })
            ) {
                return (ConditionalBranchSelection::Undecided, None);
            }
            return match self.execute_relate_pair(check, extends) {
                super::dispatch_txn::RelationStep::Assignable { bindings } => {
                    let (selection, infer) = self.select_with_inferences(check, extends, bindings);
                    if selection == ConditionalBranchSelection::True {
                        (ConditionalBranchSelection::Both, infer)
                    } else {
                        (selection, infer)
                    }
                }
                _ => (ConditionalBranchSelection::Undecided, None),
            };
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
}

/// A conditional's decision between two of its reads. Only a suspended
/// native operation is retained in `ops`; a read-free decision never
/// allocates a continuation stack.
pub(super) struct ConditionalDecision {
    check: SemanticNodeId,
    extends: SemanticNodeId,
    distributive: bool,
    branches: DecisionBranches,
    drive: super::helper_continuation::HelperDrive,
    prepared: Option<PreparedConditionalOperands>,
    selection_started: bool,
    distribution: Option<Box<ConditionalDistribution>>,
    selected: Option<ConditionalSelection>,
    ops: Vec<DecisionOp>,
}

pub(super) enum DecisionBranches {
    Materialized {
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        pending:
            Option<verter_session_query::retention::owned::Owned<ConditionalPendingSubstitution>>,
        literal_extends: Option<bool>,
    },
    Lowered,
}

pub(super) enum DecisionStep {
    Done(Box<super::walk::QueryBuildOutput>),
    Read(Box<SemanticQueryKey>),
    Branch(bool),
}

pub(super) enum DecisionDelivery<'d> {
    Read(&'d mut dyn FnMut() -> crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>),
    #[cfg(any(test, feature = "test-support"))]
    Branch(SemanticNodeId),
}

struct DecisionOp {
    operation: DecisionOperation,
    observation: Option<super::flow_return_fact::ObservedRead<()>>,
}

impl DecisionOp {
    fn take(&mut self) -> Self {
        // A suspension moves this native local into the retained chain.
        // The returned call never executes the empty replacement.
        std::mem::replace(
            self,
            Self {
                operation: DecisionOperation::Stage(StageAt::Prepare),
                observation: None,
            },
        )
    }
}

enum DecisionOperation {
    Stage(StageAt),
    Start {
        at: u8,
        keys_of: Option<SemanticNodeId>,
    },
    Preparation {
        at: u8,
        demanded: SemanticNodeId,
        operands: Option<ConditionalOperands>,
        read: Option<super::flow_return_fact::ObservedRead<()>>,
        incomplete: bool,
    },
    OperandRead {
        at: u8,
        check: SemanticNodeId,
    },
    Operands {
        at: u8,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        declared: SemanticNodeId,
        same: bool,
        extreme: Option<SemanticNodeId>,
        tuples: [Option<(SemanticNodeId, bool)>; 2],
        constructed: Option<SemanticNodeId>,
        recovery: bool,
    },
    Extreme {
        operand: SemanticNodeId,
        at: u8,
        arms: Option<Arc<[SemanticNodeId]>>,
        next: usize,
        found: [Option<SemanticNodeId>; 5],
        is_union: bool,
    },
    TupleProbe {
        node: SemanticNodeId,
        next: usize,
        normalized: Vec<crate::semantic_query::TupleElement>,
        recovered: bool,
        value: Option<SemanticNodeId>,
    },
    Deferred {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        read: Option<Box<super::evaluate::DeferredRead>>,
    },
    Fact {
        demand: FactDemand,
        read: Option<Box<super::evaluate::StructuralFactRead>>,
    },
    KeySet {
        base: SemanticNodeId,
        read: Option<Box<super::build::KeySetRead>>,
    },
    Generic {
        root: SemanticNodeId,
        infer_operand: bool,
        walk: Option<Box<GenericWalk>>,
    },
    IsDeferred {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        at: u8,
        elements: [Vec<SemanticNodeId>; 2],
        which: usize,
        next: usize,
    },
    Template {
        operand: SemanticNodeId,
        requested: bool,
    },
    Reduced {
        node: SemanticNodeId,
        requested: bool,
    },
    Indexed {
        node: SemanticNodeId,
    },
    WrittenArm {
        at: u8,
        arms: Option<Arc<[SemanticNodeId]>>,
        next: usize,
    },
    Plain {
        resolving: bool,
    },
    Relation {
        check: SemanticNodeId,
        extends: SemanticNodeId,
    },
    Absorb {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        at: u8,
        branches: [Option<SemanticNodeId>; 2],
        true_value: Option<SemanticNodeId>,
    },
    Selection {
        at: u8,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        answers: Option<Box<ConditionalOperandWalk>>,
    },
    PrepareSelection {
        at: u8,
        errors: [Option<SemanticNodeId>; 2],
        answers: Option<Box<ConditionalOperandWalk>>,
        selected: Option<ConditionalSelection>,
    },
    NamedError {
        operands: [SemanticNodeId; 2],
        next: u8,
    },
    OperandWalk(Option<Box<ConditionalOperandWalk>>),
    Distribution {
        at: u8,
    },
    Member(Box<ConditionalDecision>),
    Branch {
        take_true: bool,
        raw: bool,
        requested: bool,
    },
    Winner {
        at: u8,
        winner: Option<SemanticNodeId>,
        other: Option<SemanticNodeId>,
    },
    Commit {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        winner: SemanticNodeId,
        result: SemanticNodeId,
        selection: ConditionalBranchSelection,
        infer: Option<super::relation::RelationInferBindings>,
        checker_recovery: bool,
        at: u8,
    },
}

/// The structural-fact walk an operation holds: a full demand under its
/// context, or the reduction of a union or intersection over its resolved
/// arms.
#[derive(Clone, Copy)]
enum FactDemand {
    Demand(SemanticNodeId, ProjectionReductionContext),
    Composite(SemanticNodeId),
}

/// A distributive check read before any structural demand: its members
/// decided from its shape, a carrier the demand must resolve first, or a
/// closed `keyof` whose key set the walk over its base must read first.
enum DistributionCheck {
    Decided(Option<(SemanticNodeId, Arc<[SemanticNodeId]>)>),
    Resolve,
    KeySet(SemanticNodeId),
}

/// A check read for the written-arm proof before any structural demand:
/// proven plain by its declaration, a name the demand must resolve, or the
/// node to classify.
enum PlainCheck {
    Proven,
    Resolve,
    Named(SemanticNodeId),
}

#[derive(Clone, Copy)]
enum StageAt {
    Prepare,
    Absorb,
    Distribute,
    Select,
    Winner,
}

// Only a framed decision walks relation operands. Keeping that state behind
// its frame prevents its carrier and answer table from sizing native operations.
struct ConditionalOperandWalk {
    which: usize,
    walk: Option<super::relation::IdentityCarrierRead>,
    errors: [Option<SemanticNodeId>; 2],
    answers: super::query_frames::OperandDemand,
    awaiting: Option<SemanticQueryKey>,
}

struct GenericWalk {
    pending: Vec<(SemanticNodeId, bool)>,
    seen: [crate::semantic_query::small_node_set::SmallNodeSet; 2],
    awaiting: Option<(SemanticNodeId, bool)>,
    root: SemanticNodeId,
    infer_operand: bool,
    started: bool,
}

impl GenericWalk {
    fn new(root: SemanticNodeId, infer_operand: bool) -> Self {
        Self {
            pending: Vec::new(),
            seen: Default::default(),
            awaiting: None,
            root,
            infer_operand,
            started: false,
        }
    }
}

// The native boolean walk keeps its next key inline, just as its direct
// reader does; the read-free path allocates no delivery carrier.
#[allow(clippy::large_enum_variant)]
enum GenericStep {
    Done(bool),
    Read(SemanticQueryKey),
}

// Native results stay inline: a decision with no read must not allocate
// an output box at every operation boundary.
#[allow(clippy::large_enum_variant)]
enum DecisionValue {
    Node(Option<SemanticNodeId>),
    Flag(bool),
    Tuple(Option<(SemanticNodeId, bool)>),
    Operands(ConditionalOperands),
    Prepared(PreparedConditionalOperands),
    Selection(ConditionalSelection),
    Output(super::walk::QueryBuildOutput),
    Deferred(super::evaluate::EvaluateDeferredOutcome),
    Fact(super::evaluate::StructuralFactDemandOutcome),
    Walk(Box<ConditionalOperandWalk>),
    Relation(super::dispatch_txn::RelationStep),
}

struct DecisionResult {
    value: DecisionValue,
    observation: Option<super::flow_return_fact::ObservedRead<()>>,
}

type InPlaceBranches<'a> = Option<&'a mut dyn FnMut(bool) -> SemanticNodeId>;

// Values stay in the caller's native result slot. Only control crosses an
// operation boundary; a read-free decision allocates no result transport.
enum OperationStep {
    Done,
    // A stage completed without an answer; its dispatcher advances in place.
    Next,
    Read(Box<SemanticQueryKey>),
    Branch(bool),
}

/// Synchronous boundaries whose owners have not exposed an owned read yet.
/// Each call is explicit so it can be replaced by its owner's continuation.
enum InPlaceRead {
    IntersectionWidth,
}

impl DecisionBranches {
    fn pending(
        &self,
    ) -> Option<verter_session_query::retention::owned::Owned<ConditionalPendingSubstitution>> {
        match self {
            Self::Materialized { pending, .. } => pending.clone(),
            Self::Lowered => None,
        }
    }
    fn committed<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
        take_true: bool,
        output: &mut super::walk::QueryBuildOutput,
    ) {
        let Self::Materialized {
            true_branch,
            false_branch,
            pending,
            ..
        } = self
        else {
            return;
        };
        let original = if take_true {
            *true_branch
        } else {
            *false_branch
        };
        let frame = pending.as_ref().map(|pending| {
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

impl DecisionResult {
    fn adopt<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<DecisionValue, ExecutionAbort> {
        if let Some(observation) = self.observation {
            observation.adopt(dispatch)?;
        }
        Ok(self.value)
    }
}

// Operation control stays small. Completed values share one native stack
// slot; scalar consumers borrow it instead of returning the whole carrier.
#[must_use]
struct DecisionResultRef<'a>(&'a mut Option<DecisionResult>);

impl DecisionResultRef<'_> {
    fn into_owned(self) -> DecisionResult {
        self.0.take().expect("a completed operation")
    }
    fn adopt_scope<C: crate::resolver_core::ResolverCapabilities>(
        &mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<(), ExecutionAbort> {
        if let Some(observation) = self
            .0
            .as_mut()
            .expect("a completed operation")
            .observation
            .take()
        {
            observation.adopt(dispatch)?;
        }
        Ok(())
    }
    fn adopt<C: crate::resolver_core::ResolverCapabilities>(
        mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<DecisionValue, ExecutionAbort> {
        self.adopt_scope(dispatch)?;
        Ok(self.into_owned().value)
    }
    fn node<C: crate::resolver_core::ResolverCapabilities>(
        mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<Option<SemanticNodeId>, ExecutionAbort> {
        self.adopt_scope(dispatch)?;
        match &self.0.as_ref().expect("a completed operation").value {
            DecisionValue::Node(node) => Ok(*node),
            _ => unreachable!("the child operation answers a node"),
        }
    }
    fn flag<C: crate::resolver_core::ResolverCapabilities>(
        mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<bool, ExecutionAbort> {
        self.adopt_scope(dispatch)?;
        match &self.0.as_ref().expect("a completed operation").value {
            DecisionValue::Flag(flag) => Ok(*flag),
            _ => unreachable!("the child operation answers a predicate"),
        }
    }
    fn fact<C: crate::resolver_core::ResolverCapabilities>(
        mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<super::evaluate::StructuralFactDemandOutcome, ExecutionAbort> {
        self.adopt_scope(dispatch)?;
        match &self.0.as_ref().expect("a completed operation").value {
            DecisionValue::Fact(outcome) => Ok(*outcome),
            _ => unreachable!("the child operation answers a structural fact"),
        }
    }
    fn operands<C: crate::resolver_core::ResolverCapabilities>(
        mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Result<ConditionalOperands, ExecutionAbort> {
        self.adopt_scope(dispatch)?;
        match &self.0.as_ref().expect("a completed operation").value {
            DecisionValue::Operands(operands) => Ok(*operands),
            _ => unreachable!("the child operation answers operands"),
        }
    }
}

impl DecisionOperation {
    fn observes(&self) -> bool {
        matches!(
            self,
            Self::Start { .. }
                | Self::Preparation { .. }
                | Self::OperandRead { .. }
                | Self::Extreme { .. }
                | Self::TupleProbe { .. }
                | Self::Plain { .. }
                | Self::Relation { .. }
                | Self::PrepareSelection { .. }
                | Self::NamedError { .. }
                | Self::Selection { .. }
                | Self::OperandWalk(..)
                | Self::Winner { .. }
                | Self::Distribution { .. }
        )
    }
    fn generic(node: SemanticNodeId, infer_operand: bool) -> Self {
        Self::Generic {
            root: node,
            infer_operand,
            walk: None,
        }
    }
    fn is_deferred(check: SemanticNodeId, extends: SemanticNodeId) -> Self {
        Self::IsDeferred {
            check,
            extends,
            at: 0,
            elements: [Vec::new(), Vec::new()],
            which: 0,
            next: 0,
        }
    }
    fn extreme(operand: SemanticNodeId) -> Self {
        Self::Extreme {
            operand,
            at: 0,
            arms: None,
            next: 0,
            found: [None; 5],
            is_union: false,
        }
    }
    fn fact(demand: FactDemand) -> Self {
        Self::Fact { demand, read: None }
    }
    fn deferred(node: SemanticNodeId, context: ProjectionReductionContext) -> Self {
        Self::Deferred {
            node,
            context,
            read: None,
        }
    }
    fn tuple(node: SemanticNodeId) -> Self {
        Self::TupleProbe {
            node,
            next: 0,
            normalized: Vec::new(),
            recovered: false,
            value: None,
        }
    }
    fn branch(take_true: bool, raw: bool) -> Self {
        Self::Branch {
            take_true,
            raw,
            requested: false,
        }
    }
    fn commit(
        check: SemanticNodeId,
        extends: SemanticNodeId,
        winner: SemanticNodeId,
        selection: ConditionalBranchSelection,
        infer: Option<super::relation::RelationInferBindings>,
        checker_recovery: bool,
    ) -> Self {
        Self::Commit {
            check,
            extends,
            winner,
            result: winner,
            selection,
            infer,
            checker_recovery,
            at: 0,
        }
    }
}

static_assertions::assert_impl_all!(ConditionalDecision: Send);
static_assertions::assert_not_impl_any!(ConditionalDecision: Clone, Copy);
static_assertions::assert_impl_all!(super::build::KeySetRead: Send);
static_assertions::assert_not_impl_any!(super::build::KeySetRead: Clone, Copy);
const _: () = assert!(std::mem::size_of::<DecisionStep>() <= 24);
const _: () = assert!(std::mem::size_of::<OperationStep>() <= 24);

// Each native operation has its own out-of-line function. Only a suspended
// call chain becomes owned frames; read-free decisions keep native locals.
macro_rules! conditional_operation {
    ($d:tt, $name:ident,
     [$this:ident, $decision:ident, $frame:ident, $delivered:ident,
      $remaining:ident, $branches:ident, $result_slot:ident, $observation:ident, $child:ident,
      $read:ident, $label:lifetime],
     $pattern:pat => $body:block) => {
        #[inline(never)]
        fn $name(
            &$this,
            $decision: &mut ConditionalDecision,
            $frame: &mut DecisionOp,
            $delivered: &mut Option<DecisionDelivery<'_>>,
            $remaining: &mut usize,
            $branches: &mut InPlaceBranches<'_>,
            $result_slot: &mut Option<DecisionResult>,
        ) -> Result<OperationStep, ExecutionAbort> {
            let _ = (&$decision, &$delivered, &$remaining, &$branches);
            #[allow(unused_mut)]
            let mut $observation = if $frame.operation.observes() {
                Some(match $frame.observation.take() {
                    Some(held) => held.resume($this),
                    None => $this.begin_read_observation(),
                })
            } else {
                None
            };
            #[allow(unused_macros)]
            macro_rules! $child {
                ($d operation:expr) => {{
                    let operation = if *$remaining > 0 {
                        DecisionOperation::Stage(StageAt::Prepare)
                    } else {
                        $d operation
                    };
                    match $this.run_decision_operation(
                        $decision, operation, $delivered, $remaining, $branches, $result_slot,
                    )? {
                        OperationStep::Done => DecisionResultRef(&mut *$result_slot),
                        pending => {
                            $frame.observation = $observation.map(|scope| scope.finish(()));
                            $decision.ops.push($frame.take());
                            return Ok(pending);
                        }
                    }
                }};
            }
            #[allow(unused_macros)]
            macro_rules! $read {
                ($d key:expr) => {{
                    $frame.observation = $observation.map(|scope| scope.finish(()));
                    $decision.ops.push($frame.take());
                    return Ok(OperationStep::Read(Box::new($d key)));
                }};
            }
            #[allow(unused_labels)]
            let value = $label: loop {
                let $pattern = &mut $frame.operation else {
                    unreachable!("the dispatcher selected this operation")
                };
                $body
            };
            let mut observation = $observation.map(|scope| scope.finish(()));
            let discard = match &$frame.operation {
                DecisionOperation::Extreme { .. } => matches!(value, DecisionValue::Node(None)),
                DecisionOperation::TupleProbe { .. } => {
                    !matches!(value, DecisionValue::Tuple(Some((_, true))))
                }
                DecisionOperation::NamedError { .. } => matches!(value, DecisionValue::Node(None)),
                _ => false,
            };
            if discard {
                if let Some(held) = observation.take() {
                    held.discard()?;
                }
            }
            *$result_slot = Some(DecisionResult { value, observation });
            Ok(OperationStep::Done)
        }
    };
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    fn answer_in_place<T>(&self, _key: InPlaceRead, answer: impl FnOnce() -> T) -> T {
        #[cfg(any(test, feature = "test-support"))]
        IN_PLACE_ANSWERS.with(|count| count.set(count.get() + 1));
        answer()
    }

    fn conditional_operand_key(&self, node: SemanticNodeId) -> Option<SemanticQueryKey> {
        match self.graph().node_data(node).as_deref()? {
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            } => Some(SemanticQueryKey::Conditional {
                check: *check,
                extends: *extends,
                true_branch: *true_branch_ref,
                false_branch: *false_branch_ref,
                distributive: *distributive,
                pending: pending.clone(),
            }),
            _ => None,
        }
    }

    fn drive_conditional_in_place(
        &self,
        decision: &mut ConditionalDecision,
        lower: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> super::walk::QueryBuildOutput {
        match self.drive_decision_operations(
            decision,
            DecisionOperation::Stage(StageAt::Prepare),
            lower,
        ) {
            Ok(DecisionValue::Output(output)) => output,
            Ok(_) => unreachable!("a complete conditional output"),
            Err(abort) => self.aborted_build_output(abort),
        }
    }

    fn fold_decision_member(
        &self,
        decision: &mut ConditionalDecision,
        member_output: super::walk::QueryBuildOutput,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        DISTRIBUTED_MEMBERS.with(|count| count.set(count.get() + 1));
        let distribution = decision.distribution.as_mut().expect("a distribution");
        distribution.positions_complete &= Self::conditional_position_complete(&member_output);
        let output = &mut distribution.output;
        output.cache_suppress |= member_output.cache_suppress;
        output.fold_partial(member_output.result_is_partial());
        output.add_partial_reasons(member_output.partial_reasons());
        output
            .observed_self_roots
            .extend(member_output.observed_self_roots);
        match member_output.result {
            QueryResult::Value(node) => {
                distribution.per_member.push(node);
                true
            }
            _ => {
                self.fold_into_top_build_local_taint_with(true, true, output.partial_reasons());
                self.deposit_operand_self_roots(&output.observed_self_roots);
                decision.distribution = None;
                false
            }
        }
    }

    fn conditional_position_complete(output: &super::walk::QueryBuildOutput) -> bool {
        !output.result_is_partial()
            || matches!(
                output.approximation,
                Some(
                    super::projection_fact::ProjectionInterpretation::Positional
                        | super::projection_fact::ProjectionInterpretation::CheckerRecovery
                )
            )
    }

    fn drive_decision_operations(
        &self,
        decision: &mut ConditionalDecision,
        operation: DecisionOperation,
        lower: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> Result<DecisionValue, ExecutionAbort> {
        let (_connected, _) = self.enter_connected_demand(false);
        let mut lowered = [None; 2];
        let mut answer_branch =
            |branch| *lowered[usize::from(branch)].get_or_insert_with(|| lower(branch));
        let mut in_place_branches: InPlaceBranches<'_> = Some(&mut answer_branch);
        let mut remaining = 0;
        let mut delivered = None;
        let mut result_slot = None;
        let mut step = self.run_decision_operation(
            decision,
            operation,
            &mut delivered,
            &mut remaining,
            &mut in_place_branches,
            &mut result_slot,
        );
        loop {
            match step {
                Ok(OperationStep::Done) => {
                    if remaining > 0 {
                        decision.ops.reverse();
                        self.release_conditional_decision(decision);
                    }
                    return result_slot
                        .take()
                        .expect("the completed decision")
                        .adopt(self);
                }
                Ok(OperationStep::Read(key)) => {
                    #[cfg(any(test, feature = "test-support"))]
                    CONDITIONAL_READ_TRACE.with(|trace| {
                        if let Some(trace) = trace.borrow_mut().as_mut() {
                            trace.push((*key).clone());
                        }
                    });
                    decision.ops.reverse();
                    let counts = Self::decision_reads_for_carrier(decision)
                        && !matches!(*key, SemanticQueryKey::TypeOf { .. });
                    remaining = decision.ops.len();
                    decision.ops.reverse();
                    let mut key = Some(*key);
                    let mut answer = || {
                        let key = key.take().expect("one operation read");
                        if counts {
                            self.record_dispatch_intent_counters(&key);
                        }
                        self.execute_read(key)
                    };
                    let mut delivery = Some(DecisionDelivery::Read(&mut answer));
                    step = self.run_decision_operation(
                        decision,
                        DecisionOperation::Stage(StageAt::Prepare),
                        &mut delivery,
                        &mut remaining,
                        &mut in_place_branches,
                        &mut result_slot,
                    );
                }
                Ok(OperationStep::Next) => {
                    unreachable!("a stage transition stays in its dispatcher")
                }
                Ok(OperationStep::Branch(_)) => {
                    unreachable!("the in-place driver answers branches inside their operations")
                }
                Err(abort) => {
                    self.release_decision_operations(decision);
                    return Err(abort);
                }
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn conditional_absorption_from_steps(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        lower: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> Option<super::walk::QueryBuildOutput> {
        let mut decision = self.begin_conditional_decision(
            check,
            extends,
            distributive,
            DecisionBranches::Lowered,
            super::helper_continuation::HelperDrive::InPlace,
        );
        match self.drive_decision_operations(
            &mut decision,
            DecisionOperation::Absorb {
                check,
                extends,
                at: 0,
                branches: [None; 2],
                true_value: None,
            },
            lower,
        ) {
            Ok(DecisionValue::Output(output)) => Some(output),
            Ok(DecisionValue::Node(None)) => None,
            Ok(_) => unreachable!("an absorption outcome"),
            Err(abort) => Some(self.aborted_build_output(abort)),
        }
    }

    /// Start a decision without making any read or lowering a branch.
    pub(super) fn begin_conditional_decision(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        branches: DecisionBranches,
        drive: super::helper_continuation::HelperDrive,
    ) -> ConditionalDecision {
        ConditionalDecision {
            check,
            extends,
            distributive,
            branches,
            drive,
            prepared: None,
            selection_started: false,
            distribution: None,
            selected: None,
            ops: Vec::new(),
        }
    }

    /// Consume a delivery under the observation of the operation that named
    /// it. Completed prefixes and native locals are never reconstructed.
    pub(super) fn advance_conditional_decision(
        &self,
        decision: &mut ConditionalDecision,
        delivered: Option<DecisionDelivery<'_>>,
    ) -> DecisionStep {
        self.advance_conditional_decision_with_branches(decision, delivered, &mut None)
    }

    fn advance_conditional_decision_with_branches(
        &self,
        decision: &mut ConditionalDecision,
        delivered: Option<DecisionDelivery<'_>>,
        in_place_branches: &mut InPlaceBranches<'_>,
    ) -> DecisionStep {
        if self.cancellation.is_cancelled() {
            self.release_conditional_decision(decision);
            return DecisionStep::Done(Box::new(self.cancelled_build_output()));
        }
        let mut delivered = delivered;
        let mut result_slot = None;
        let mut remaining = decision.ops.len();
        decision.ops.reverse();
        let result = self.run_decision_operation(
            decision,
            DecisionOperation::Stage(StageAt::Prepare),
            &mut delivered,
            &mut remaining,
            in_place_branches,
            &mut result_slot,
        );
        if remaining > 0 {
            verter_debug_assert!(
                self.cancellation.is_cancelled() || result.is_err(),
                "only a terminal stop bypasses retained descendants"
            );
            decision.ops.reverse();
            self.release_conditional_decision(decision);
            remaining = 0;
            delivered = None;
        }
        verter_debug_assert!(
            delivered.is_none(),
            "a delivery is consumed by its standing operation"
        );
        verter_debug_assert!(remaining == 0, "every retained operation was resumed");
        match result {
            Ok(OperationStep::Done) => match result_slot
                .take()
                .expect("the completed decision")
                .adopt(self)
            {
                Ok(DecisionValue::Output(output)) => DecisionStep::Done(Box::new(output)),
                Ok(_) => unreachable!("the root operation produces the conditional"),
                Err(abort) => DecisionStep::Done(Box::new(self.aborted_build_output(abort))),
            },
            Ok(OperationStep::Read(key)) => {
                decision.ops.reverse();
                DecisionStep::Read(key)
            }
            Ok(OperationStep::Next) => unreachable!("a stage transition stays in its dispatcher"),
            Ok(OperationStep::Branch(branch)) => {
                decision.ops.reverse();
                DecisionStep::Branch(branch)
            }
            Err(abort) => {
                self.release_decision_operations(decision);
                DecisionStep::Done(Box::new(self.aborted_build_output(abort)))
            }
        }
    }

    pub(super) fn decision_reads_for_carrier(decision: &ConditionalDecision) -> bool {
        decision.reads_for_carrier()
    }

    pub(super) fn abandon_conditional_decision(&self, mut decision: ConditionalDecision) {
        self.release_decision_operations(&mut decision);
    }

    pub(super) fn release_conditional_decision(&self, decision: &mut ConditionalDecision) {
        self.release_decision_operations(decision);
    }

    fn release_decision_operations(&self, decision: &mut ConditionalDecision) {
        while let Some(op) = decision.ops.pop() {
            match op.operation {
                DecisionOperation::Deferred {
                    read: Some(read), ..
                } => self.abandon_deferred_read(*read),
                DecisionOperation::Fact {
                    read: Some(read), ..
                } => self.abandon_structural_fact_read(*read),
                DecisionOperation::KeySet {
                    read: Some(read), ..
                } => self.abandon_key_set_read(read),
                DecisionOperation::OperandWalk(Some(mut state)) => {
                    if let Some(walk) = state.walk.take() {
                        self.abandon_identity_carrier_read(walk);
                    }
                }
                DecisionOperation::Member(member) => self.abandon_conditional_decision(*member),
                _ => {}
            }
            if let Some(observation) = op.observation {
                let _ = observation.discard();
            }
        }
    }

    #[inline(never)]
    fn run_decision_operation(
        &self,
        decision: &mut ConditionalDecision,
        initial: DecisionOperation,
        delivered: &mut Option<DecisionDelivery<'_>>,
        remaining: &mut usize,
        in_place_branches: &mut InPlaceBranches<'_>,
        result_slot: &mut Option<DecisionResult>,
    ) -> Result<OperationStep, ExecutionAbort> {
        let mut frame = if *remaining > 0 {
            *remaining -= 1;
            decision.ops.pop().expect("a retained native operation")
        } else {
            DecisionOp {
                operation: initial,
                observation: None,
            }
        };
        match &frame.operation {
            DecisionOperation::Stage(..) => self.run_conditional_stage(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Start { .. } => self.run_conditional_start(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Preparation { .. } => self.run_conditional_preparation(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::OperandRead { .. } => self.run_conditional_operand_read(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Operands { .. } => self.run_conditional_operands(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Reduced { .. } => self.run_conditional_reduced(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Template { .. } => self.run_conditional_template(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Deferred { .. } => self.run_conditional_deferred(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Fact { .. } => self.run_conditional_fact(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::KeySet { .. } => self.run_conditional_key_set(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::NamedError { .. } => self.run_conditional_named_error(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Indexed { .. } => self.run_conditional_indexed(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Extreme { .. } => self.run_conditional_extreme(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::TupleProbe { .. } => self.run_conditional_tuple_probe(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::IsDeferred { .. } => self.run_conditional_is_deferred(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Branch { .. } => self.run_conditional_branch(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Plain { .. } => self.run_conditional_plain(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::WrittenArm { .. } => self.run_conditional_written_arm(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Relation { .. } => self.run_conditional_relation(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Absorb { .. } => self.run_conditional_absorb(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Selection { .. } => self.run_conditional_selection(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::PrepareSelection { .. } => self.run_conditional_prepare_selection(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::OperandWalk(..) => self.run_conditional_operand_walk(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Generic { .. } => self.run_conditional_generic(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Member(..) => self.run_conditional_member(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Distribution { .. } => self.run_conditional_distribution(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Winner { .. } => self.run_conditional_winner(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
            DecisionOperation::Commit { .. } => self.run_conditional_commit(
                decision,
                &mut frame,
                delivered,
                remaining,
                in_place_branches,
                result_slot,
            ),
        }
    }

    #[inline(never)]
    fn run_conditional_stage(
        &self,
        decision: &mut ConditionalDecision,
        frame: &mut DecisionOp,
        delivered: &mut Option<DecisionDelivery<'_>>,
        remaining: &mut usize,
        in_place_branches: &mut InPlaceBranches<'_>,
        result_slot: &mut Option<DecisionResult>,
    ) -> Result<OperationStep, ExecutionAbort> {
        loop {
            if self.cancellation.is_cancelled() {
                *result_slot = Some(DecisionResult {
                    value: DecisionValue::Output(self.cancelled_build_output()),
                    observation: None,
                });
                return Ok(OperationStep::Done);
            }
            let step = match &frame.operation {
                DecisionOperation::Stage(StageAt::Prepare) => self.run_conditional_prepare_stage(
                    decision,
                    frame,
                    delivered,
                    remaining,
                    in_place_branches,
                    result_slot,
                ),
                DecisionOperation::Stage(StageAt::Absorb) => self.run_conditional_absorb_stage(
                    decision,
                    frame,
                    delivered,
                    remaining,
                    in_place_branches,
                    result_slot,
                ),
                DecisionOperation::Stage(StageAt::Distribute) => self
                    .run_conditional_distribute_stage(
                        decision,
                        frame,
                        delivered,
                        remaining,
                        in_place_branches,
                        result_slot,
                    ),
                DecisionOperation::Stage(StageAt::Select) => self.run_conditional_select_stage(
                    decision,
                    frame,
                    delivered,
                    remaining,
                    in_place_branches,
                    result_slot,
                ),
                DecisionOperation::Stage(StageAt::Winner) => self.run_conditional_winner_stage(
                    decision,
                    frame,
                    delivered,
                    remaining,
                    in_place_branches,
                    result_slot,
                ),
                _ => unreachable!("a conditional decision stage"),
            };
            match step {
                Ok(OperationStep::Next) => {}
                other => return other,
            }
        }
    }

    conditional_operation! { $, run_conditional_prepare_stage,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Stage(at) => {
            let result = child!(DecisionOperation::Preparation {
                at: 0,
                demanded: decision.check,
                operands: None,
                read: None,
                incomplete: false
            })
            .adopt(self)?;
            match result {
                DecisionValue::Prepared(prepared) => {
                    decision.prepared = Some(prepared)
                }
                DecisionValue::Output(output) => {
                    break DecisionValue::Output(output)
                }
                _ => unreachable!(),
            }
            *at = StageAt::Absorb;
            return Ok(OperationStep::Next);
        }
    }

    conditional_operation! { $, run_conditional_absorb_stage,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Stage(at) => {
            verter_debug_assert!(
                !decision.selection_started,
                "absorption is entered once"
            );
            let result = child!(DecisionOperation::Start {
                at: 0,
                keys_of: None
            })
            .adopt(self)?;
            if let DecisionValue::Output(output) = result {
                break DecisionValue::Output(output);
            }
            decision.selection_started = true;
            *at = StageAt::Distribute;
            return Ok(OperationStep::Next);
        }
    }

    conditional_operation! { $, run_conditional_distribute_stage,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Stage(at) => {
            if decision.distribution.is_some() {
                let result = child!(DecisionOperation::Distribution { at: 0 })
                    .adopt(self)?;
                if let DecisionValue::Output(output) = result {
                    break DecisionValue::Output(output);
                }
            }
            *at = StageAt::Select;
            return Ok(OperationStep::Next);
        }
    }

    conditional_operation! { $, run_conditional_select_stage,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Stage(at) => {
            let result = child!(DecisionOperation::PrepareSelection {
                at: 0,
                errors: [None; 2],
                answers: None,
                selected: None
            })
            .adopt(self)?;
            match result {
                DecisionValue::Selection(selected) => {
                    decision.selected = Some(selected)
                }
                DecisionValue::Output(output) => {
                    break DecisionValue::Output(output)
                }
                _ => unreachable!(),
            }
            *at = StageAt::Winner;
            return Ok(OperationStep::Next);
        }
    }

    conditional_operation! { $, run_conditional_winner_stage,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Stage(_) => {
            break child!(DecisionOperation::Winner {
                at: 0,
                winner: None,
                other: None
            })
            .adopt(self)?;
        }
    }

    conditional_operation! { $, run_conditional_start,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Start { at, keys_of } => {
            let operands = decision.prepared.expect("prepared operands").operands;
            let surface = match *at {
                0 => {
                    let result = child!(DecisionOperation::Absorb {
                        check: operands.absorbed_check,
                        extends: operands.extends,
                        at: 0,
                        branches: [None; 2],
                        true_value: None
                    })
                    .adopt(self)?;
                    if let DecisionValue::Output(output) = result {
                        break DecisionValue::Output(output);
                    }
                    if !decision.distributive {
                        break DecisionValue::Node(None);
                    }
                    match self.distributive_check(operands.declared_check) {
                        DistributionCheck::Decided(surface) => surface,
                        DistributionCheck::Resolve => {
                            *at = 1;
                            continue;
                        }
                        DistributionCheck::KeySet(base) => {
                            *keys_of = Some(base);
                            *at = 2;
                            continue;
                        }
                    }
                }
                2 => child!(DecisionOperation::KeySet {
                    base: keys_of.expect("a closed keyof check names its base"),
                    read: None
                })
                .node(self)?
                .and_then(|keys| self.distribution_members_of(keys)),
                _ => child!(DecisionOperation::fact(FactDemand::Demand(
                    operands.declared_check,
                    ProjectionReductionContext::structural_transit()
                )))
                .fact(self)?
                .into_complete_node()
                .and_then(|resolved| self.distribution_members_of(resolved)),
            };
            decision.distribution = surface.map(|surface| {
                self.begin_conditional_distribution(
                    operands.declared_check,
                    operands.extends,
                    surface,
                )
            });
            break DecisionValue::Node(None);
        }
    }

    conditional_operation! { $, run_conditional_preparation,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Preparation {
            at,
            demanded,
            operands,
            read,
            incomplete,
        } => {
            match *at {
                0 => {
                    let result = child!(DecisionOperation::OperandRead {
                        at: 0,
                        check: decision.check
                    });
                    let result = result.into_owned();
                    let DecisionValue::Prepared(prepared) = result.value else {
                        unreachable!()
                    };
                    let observation = result.observation.expect("the operand read is observed");
                    *incomplete = !observation.observations_are_complete()
                        && !prepared.operands.checker_recovery
                        && !self
                            .is_conditional_checker_recovery(prepared.operands.absorbed_check)
                        && !self.is_conditional_checker_recovery(prepared.operands.extends);
                    *demanded = prepared.demanded_check;
                    *operands = Some(prepared.operands);
                    *read = Some(observation);
                    *at = 1;
                }
                1 => {
                    if *incomplete && *demanded == decision.check {
                        let arm = child!(DecisionOperation::WrittenArm {
                            at: 0,
                            arms: None,
                            next: 0
                        });
                        let arm = arm.into_owned();
                        if matches!(arm.value, DecisionValue::Flag(true)) {
                            let operand_read = read.take().expect("unadopted operands");
                            operand_read.preserve_reuse_evidence(self);
                            operand_read.discard()?;
                            arm.adopt(self)?;
                            if self.cancellation.is_cancelled() {
                                break DecisionValue::Output(self.cancelled_build_output());
                            }
                            *at = 2;
                            continue;
                        }
                        arm.adopt(self)?;
                    }
                    read.take().expect("unadopted operands").adopt(self)?;
                    if *incomplete {
                        *at = 3;
                    } else {
                        break DecisionValue::Prepared(PreparedConditionalOperands {
                            demanded_check: *demanded,
                            operands: operands.expect("constructed operands"),
                        });
                    }
                }
                2 => {
                    let winner = child!(DecisionOperation::branch(true, false))
                        .node(self)?
                        .expect("a branch node");
                    *demanded = winner;
                    *at = 5;
                }
                3 => {
                    *demanded = child!(DecisionOperation::branch(true, true))
                        .node(self)?
                        .expect("true branch");
                    if self.cancellation.is_cancelled() {
                        break DecisionValue::Output(self.cancelled_build_output());
                    }
                    *at = 4;
                }
                4 => {
                    let false_branch = child!(DecisionOperation::branch(false, true))
                        .node(self)?
                        .expect("false branch");
                    break DecisionValue::Output(self.keep_conditional(
                        decision.check,
                        decision.extends,
                        (*demanded, false_branch),
                        decision.distributive,
                        decision.branches.pending(),
                        ConditionalBranchSelection::Undecided,
                    ));
                }
                5 => {
                    let result = child!(DecisionOperation::commit(
                        decision.check,
                        decision.extends,
                        *demanded,
                        ConditionalBranchSelection::True,
                        None,
                        false
                    ))
                    .adopt(self)?;
                    let DecisionValue::Output(mut output) = result else {
                        unreachable!()
                    };
                    decision.branches.committed(self, true, &mut output);
                    break DecisionValue::Output(output);
                }
                _ => unreachable!(),
            };
        }
    }

    conditional_operation! { $, run_conditional_operand_read,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::OperandRead { at, check } => {
            match *at {
                0 => {
                    if matches!(
                        self.graph().node_data(*check).as_deref(),
                        Some(SemanticNodeData::IndexedAccess { .. })
                    ) && !child!(DecisionOperation::is_deferred(*check, decision.extends))
                        .flag(self)?
                    {
                        *at = 1;
                        continue;
                    }
                    *at = 2;
                }
                1 => {
                    let result = child!(DecisionOperation::deferred(
                        *check,
                        ProjectionReductionContext::published(ProjectionMode::Navigate)
                    ))
                    .adopt(self)?;
                    let DecisionValue::Deferred(outcome) = result else {
                        unreachable!()
                    };
                    let evaluated = outcome
                        .into_complete_active_query_build_node(self)
                        .filter(|node| {
                            !matches!(
                                self.graph().node_data(*node).as_deref(),
                                Some(SemanticNodeData::Opaque(error))
                                    if !super::build::is_certified_recovery(error)
                            )
                        })
                        .unwrap_or(*check);
                    *check = self.canonical_argument_carrier(evaluated);
                    *at = 2;
                }
                2 => {
                    let operands = child!(DecisionOperation::Operands {
                        at: 0,
                        check: *check,
                        extends: decision.extends,
                        declared: *check,
                        same: true,
                        extreme: None,
                        tuples: [None; 2],
                        constructed: None,
                        recovery: false
                    })
                    .operands(self)?;
                    break DecisionValue::Prepared(PreparedConditionalOperands {
                        demanded_check: *check,
                        operands,
                    });
                }
                _ => unreachable!(),
            };
        }
    }

    conditional_operation! { $, run_conditional_operands,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Operands {
            at,
            check,
            extends,
            declared,
            same,
            extreme,
            tuples,
            constructed,
            recovery,
        } => {
            match *at {
                0 => {
                    #[cfg(any(test, feature = "test-support"))]
                    OPERAND_PREPARATIONS.with(|count| count.set(count.get() + 1));
                    *at = 1;
                }
                1 => {
                    *check = child!(DecisionOperation::fact(FactDemand::Composite(*check)))
                        .node(self)?
                        .unwrap_or(*check);
                    *check = self.union_as_constructed(*check);
                    *declared = *check;
                    *at = if self.declared_operand_resolves(*check) { 2 } else { 3 };
                }
                2 => {
                    let outcome =
                        child!(DecisionOperation::fact(self.resolved_operand_demand(*check)))
                            .fact(self)?;
                    *declared = self.resolved_operand_of(outcome).unwrap_or(*check);
                    *at = 3;
                }
                3 => {
                    *same = *declared == *check;
                    if !*same {
                        *declared = self.union_as_constructed(*declared);
                    }
                    *at = 4;
                }
                4 => {
                    *extends = child!(DecisionOperation::fact(FactDemand::Composite(*extends)))
                        .node(self)?
                        .unwrap_or(*extends);
                    *at = 5;
                }
                5 => {
                    *check = child!(DecisionOperation::Reduced {
                        node: *check,
                        requested: false
                    })
                    .node(self)?
                    .expect("reduced check");
                    *at = 6;
                }
                6 => {
                    *extends = child!(DecisionOperation::Reduced {
                        node: *extends,
                        requested: false
                    })
                    .node(self)?
                    .expect("reduced extends");
                    *at = 7;
                }
                7 => {
                    *declared = if *same {
                        *check
                    } else {
                        child!(DecisionOperation::Reduced {
                            node: *declared,
                            requested: false
                        })
                        .node(self)?
                        .expect("declared check")
                    };
                    *at = 8;
                }
                8 => {
                    *extreme = child!(DecisionOperation::extreme(*check)).node(self)?;
                    *at = 9;
                }
                9 => {
                    *extends = child!(DecisionOperation::extreme(*extends))
                        .node(self)?
                        .unwrap_or(*extends);
                    *at = 10;
                }
                10 => {
                    *declared = if *same {
                        extreme.unwrap_or(*check)
                    } else {
                        child!(DecisionOperation::extreme(*declared))
                            .node(self)?
                            .unwrap_or(*declared)
                    };
                    *check = extreme.unwrap_or(*check);
                    *at = 11;
                }
                11 => {
                    let result = child!(DecisionOperation::tuple(*check)).adopt(self)?;
                    let DecisionValue::Tuple(tuple) = result else {
                        unreachable!()
                    };
                    tuples[0] = tuple;
                    *at = 12;
                }
                12 => {
                    let result = child!(DecisionOperation::tuple(*extends)).adopt(self)?;
                    let DecisionValue::Tuple(tuple) = result else {
                        unreachable!()
                    };
                    tuples[1] = tuple;
                    let has_recovery = tuples
                        .iter()
                        .any(|tuple| tuple.is_some_and(|(_, recovered)| recovered));
                    *at = if has_recovery { 13 } else { 15 };
                }
                // A checker recovery in either tuple reconstructs both
                // operands: each its recovered tuple, or the operand the
                // structural demand resolves it to. One that resolves to
                // nothing keeps both as they were.
                13 | 14 => {
                    let which = usize::from(*at - 13);
                    let original = if which == 0 { *check } else { *extends };
                    let rebuilt = match tuples[which] {
                        Some((node, _)) => Some(node),
                        None if matches!(
                            self.graph().node_data(original).as_deref(),
                            Some(SemanticNodeData::Tuple { .. })
                        ) =>
                        {
                            None
                        }
                        None => {
                            let outcome = child!(DecisionOperation::fact(
                                self.resolved_operand_demand(original)
                            ))
                            .fact(self)?;
                            self.resolved_operand_of(outcome)
                        }
                    };
                    match (which, rebuilt) {
                        (_, None) => *at = 15,
                        (0, Some(node)) => {
                            *constructed = Some(node);
                            *at = 14;
                        }
                        (_, Some(node)) => {
                            *check = constructed.take().expect("the reconstructed check");
                            *extends = node;
                            *recovery = true;
                            *at = 15;
                        }
                    }
                }
                15 => {
                    let absorbed_check = child!(DecisionOperation::Indexed { node: *declared })
                        .node(self)?
                        .expect("the absorbed check");
                    break DecisionValue::Operands(ConditionalOperands {
                        check: *check,
                        declared_check: *declared,
                        absorbed_check,
                        extends: *extends,
                        checker_recovery: *recovery,
                    });
                }
                _ => unreachable!(),
            };
        }
    }

    conditional_operation! { $, run_conditional_reduced,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Reduced { node, requested } => {
            if !*requested {
                let Some(key) = self.conditional_operand_key(*node) else {
                    break DecisionValue::Node(Some(*node));
                };
                *requested = true;
                read!(key);
            }
            let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                unreachable!("the conditional operand's read is delivered")
            };
            let read = deliver();
            let reduced = match read.value {
                QueryResult::Value(value)
                    if (read.fact_status.is_complete()
                        || self.is_conditional_checker_recovery(value))
                        && !matches!(
                            self.graph().node_data(value).as_deref(),
                            Some(SemanticNodeData::Conditional { .. })
                        ) =>
                {
                    value
                }
                _ => *node,
            };
            break DecisionValue::Node(Some(reduced));
        }
    }

    conditional_operation! { $, run_conditional_template,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Template { operand, requested } => {
            if !*requested {
                let key = match self.graph().node_data(*operand).as_deref() {
                    Some(SemanticNodeData::TemplateLiteral {
                        quasis,
                        expressions,
                    }) => SemanticQueryKey::TemplateLiteralReduce {
                        pattern: quasis.clone(),
                        args: Arc::clone(expressions.as_arc()),
                        context: self.template_literal_reduce_context(),
                    },
                    _ => break DecisionValue::Node(None),
                };
                *requested = true;
                read!(key);
            }
            let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                unreachable!("a template reduction delivery")
            };
            let reduced = match deliver().value {
                QueryResult::Value(node) if self.is_conditional_checker_recovery(node) => {
                    Some(node)
                }
                _ => None,
            };
            break DecisionValue::Node(reduced);
        }
    }

    conditional_operation! { $, run_conditional_deferred,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Deferred {
            node,
            context,
            read: retained,
        } => {
            let step = match retained.take() {
                Some(read) => {
                    let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                        unreachable!("the evaluation's delivery")
                    };
                    self.resume_deferred_read(read, deliver)
                }
                None => self.begin_deferred_read(
                    *node,
                    *context,
                    super::evaluate::DeferredEvaluationDemand::Shell,
                ),
            };
            match step {
                super::evaluate::DeferredStep::Done(outcome) => {
                    break DecisionValue::Deferred(outcome)
                }
                super::evaluate::DeferredStep::Read(key, state) => {
                    *retained = Some(state);
                    read!(key);
                }
            }
        }
    }

    conditional_operation! { $, run_conditional_fact,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Fact {
            demand,
            read: retained,
        } => {
            let step = match retained.take() {
                Some(read) => {
                    let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                        unreachable!("the structural fact's delivery")
                    };
                    self.resume_structural_fact_read(read, deliver)
                }
                None => match *demand {
                    FactDemand::Demand(node, context) => {
                        self.begin_structural_fact_read(node, context, true, true)
                    }
                    FactDemand::Composite(node) => self.begin_structural_composite_read(node),
                },
            };
            match step {
                super::evaluate::FactStep::Done(outcome) => break DecisionValue::Fact(outcome),
                super::evaluate::FactStep::Composite(reduced) => break DecisionValue::Node(reduced),
                super::evaluate::FactStep::Read(key, state) => {
                    *retained = Some(state);
                    read!(key);
                }
            }
        }
    }

    // The literal keys a closed `keyof` check denotes, read by the key-set
    // walk the operation holds: each `keyof` read it names is delivered
    // inside the operation, and the walk continues from the carriers it
    // already settled.
    conditional_operation! { $, run_conditional_key_set,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::KeySet {
            base,
            read: retained,
        } => {
            let step = match retained.take() {
                Some(read) => {
                    let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                        unreachable!("the key set's delivery")
                    };
                    self.resume_key_set_read(read, deliver)
                }
                None => self.begin_key_set_read(*base),
            };
            match step {
                super::build::KeySetStep::Done(keys) => break DecisionValue::Node(keys),
                super::build::KeySetStep::Read(key, state) => {
                    *retained = Some(state);
                    read!(key);
                }
            }
        }
    }

    // The checker's error type a conditional's check or extends operand,
    // written as a name, stands for: the recovery from a diagnostic the
    // name's own instantiation raised is the conditional's answer, as the
    // checker returns its error type when either operand is it. Read after
    // the conditional is related, which has already read the name; an
    // optional probe, kept only when it finds that error.
    conditional_operation! { $, run_conditional_named_error,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::NamedError { operands, next } => {
            let Some(&operand) = operands.get(usize::from(*next)) else {
                break DecisionValue::Node(None);
            };
            if !self.is_named_conditional_operand(operand) {
                *next += 1;
                continue;
            }
            let outcome = child!(DecisionOperation::fact(self.resolved_operand_demand(operand)))
                .fact(self)?;
            *next += 1;
            if let Some(resolved) = self.resolved_operand_of(outcome) {
                if matches!(
                    self.peek_special(resolved),
                    Some((
                        super::absorb::SpecialKind::Error | super::absorb::SpecialKind::Wildcard,
                        _
                    ))
                ) {
                    break DecisionValue::Output(self.absorbed_output(resolved, *operands));
                }
            }
        }
    }

    conditional_operation! { $, run_conditional_indexed,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Indexed { node } => {
            if !matches!(
                self.graph().node_data(*node).as_deref(),
                Some(SemanticNodeData::IndexedAccess { .. })
            ) {
                break DecisionValue::Node(Some(*node));
            }
            let result = child!(DecisionOperation::deferred(
                *node,
                ProjectionReductionContext::structural_transit()
            ))
            .adopt(self)?;
            let DecisionValue::Deferred(outcome) = result else {
                unreachable!()
            };
            let output = outcome.into_query_build_output(self);
            let complete = !output.result_is_partial();
            let reduced = match output.result {
                QueryResult::Value(value)
                    if complete || self.is_conditional_checker_recovery(value) =>
                {
                    value
                }
                _ => *node,
            };
            break DecisionValue::Node(Some(reduced));
        }
    }

    conditional_operation! { $, run_conditional_extreme,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Extreme {
            operand,
            at,
            arms,
            next,
            found,
            is_union,
        } => {
            use super::absorb::SpecialKind;
            if *at == 0 {
                match self.graph().node_data(*operand).as_deref() {
                    Some(SemanticNodeData::Intersection(members)) => {
                        *arms = Some(members.members_arc());
                        *is_union = false;
                    }
                    Some(SemanticNodeData::Union(members)) => {
                        *arms = Some(members.members_arc());
                        *is_union = true;
                    }
                    Some(SemanticNodeData::TemplateLiteral { .. }) => {
                        *at = 2;
                        continue;
                    }
                    _ => break DecisionValue::Node(None),
                }
                if !*is_union {
                    if let Some(recovery) = self
                        .answer_in_place(InPlaceRead::IntersectionWidth, || {
                            self.intersection_too_complex(*operand)
                        })
                    {
                        break DecisionValue::Node(Some(recovery));
                    }
                }
                if *is_union && self.literal_union_index_proves_literal_only(*operand) {
                    break DecisionValue::Node(None);
                }
                *at = 1;
            }
            if *at == 2 {
                break DecisionValue::Node(
                    child!(DecisionOperation::Template {
                        operand: *operand,
                        requested: false
                    })
                    .node(self)?,
                );
            }
            let arms = arms.as_ref().expect("the composite arms");
            let Some(&arm) = arms.get(*next) else {
                let [never, error, any, unknown, wildcard] = *found;
                break DecisionValue::Node(if *is_union {
                    error.or(wildcard).or(any).or(unknown)
                } else {
                    never.or(error).or(wildcard).or(any)
                });
            };
            if *at == 1 {
                #[cfg(any(test, feature = "test-support"))]
                note_operand_walk_visit();
                *at = 3;
            }
            let arm_type = match self.graph().node_data(arm).as_deref() {
                Some(
                    SemanticNodeData::Intersection(_)
                    | SemanticNodeData::Union(_)
                    | SemanticNodeData::TemplateLiteral { .. },
                ) => child!(DecisionOperation::extreme(arm)).node(self)?,
                Some(
                    SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::IndexedAccess { .. }
                    | SemanticNodeData::TypeOf(_)
                    | SemanticNodeData::BareRef(_)
                    | SemanticNodeData::ImportType(_)
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
                ) => {
                    let outcome =
                        child!(DecisionOperation::fact(self.resolved_operand_demand(arm)))
                            .fact(self)?;
                    self.resolved_operand_of(outcome)
                }
                _ => Some(arm),
            };
            if let Some((kind, _)) = arm_type.and_then(|node| self.peek_special(node)) {
                let slot = match kind {
                    SpecialKind::Never => 0,
                    SpecialKind::Error => 1,
                    SpecialKind::Any => 2,
                    SpecialKind::Unknown => 3,
                    SpecialKind::Wildcard => 4,
                };
                found[slot].get_or_insert(arm_type.expect("classified arm"));
            }
            *next += 1;
            *at = 1;
        }
    }

    conditional_operation! { $, run_conditional_tuple_probe,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::TupleProbe {
            node,
            next,
            normalized,
            recovered,
            value,
        } => {
            let Some(data) = self.graph().node_data(*node) else {
                break DecisionValue::Tuple(None);
            };
            let SemanticNodeData::Tuple { elements, readonly } = data.as_ref() else {
                break DecisionValue::Tuple(None);
            };
            if *next == 0 && normalized.capacity() == 0 {
                normalized.reserve_exact(elements.len());
            }
            let Some(element) = elements.get(*next) else {
                if !*recovered {
                    break DecisionValue::Tuple(Some((*node, false)));
                }
                let result = self.graph().intern_preserving_scope(
                    *node,
                    SemanticNodeData::Tuple {
                        elements: crate::semantic_query::PayloadArray::from(
                            std::mem::take(normalized),
                        ),
                        readonly: *readonly,
                    },
                );
                break DecisionValue::Tuple(Some((result, true)));
            };
            if value.is_none() {
                let outcome = child!(DecisionOperation::fact(FactDemand::Demand(
                    element.value,
                    ProjectionReductionContext::structural_transit()
                )))
                .fact(self)?;
                match outcome {
                    super::evaluate::StructuralFactDemandOutcome::Complete(node) => {
                        *value = Some(node)
                    }
                    super::evaluate::StructuralFactDemandOutcome::Recovered {
                        node,
                        ..
                    } if self.is_conditional_checker_recovery(node) => {
                        *recovered = true;
                        *value = Some(node);
                    }
                    _ => break DecisionValue::Tuple(None),
                }
            }
            let original = value.expect("normalized tuple position");
            let constructed = child!(DecisionOperation::extreme(original))
                .node(self)?
                .unwrap_or(original);
            *recovered |= self.is_conditional_checker_recovery(constructed);
            let mut position = element.copy();
            position.value = constructed;
            normalized.push(position);
            *next += 1;
            *value = None;
        }
    }

    conditional_operation! { $, run_conditional_is_deferred,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::IsDeferred {
            check,
            extends,
            at,
            elements,
            which,
            next,
        } => {
            match *at {
                0 => {
                    if child!(DecisionOperation::generic(*check, true)).flag(self)? {
                        break DecisionValue::Flag(true);
                    }
                    *at = 1;
                }
                1 => {
                    if child!(DecisionOperation::generic(*extends, false)).flag(self)? {
                        break DecisionValue::Flag(true);
                    }
                    let tuple = |node| match self.graph().node_data(node).as_deref() {
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
                    match (tuple(*check), tuple(*extends)) {
                        (Some(check), Some(extends)) if check.len() == extends.len() => {
                            *elements = [check, extends];
                        }
                        _ => break DecisionValue::Flag(false),
                    }
                    *at = 2;
                }
                2 => {
                    let Some(&node) = elements[*which].get(*next) else {
                        if *which == 0 {
                            *which = 1;
                            *next = 0;
                            continue;
                        }
                        break DecisionValue::Flag(false);
                    };
                    let infer_operand = *which == 0;
                    if child!(DecisionOperation::generic(node, infer_operand)).flag(self)? {
                        break DecisionValue::Flag(true);
                    }
                    *next += 1;
                }
                _ => unreachable!(),
            };
        }
    }

    conditional_operation! { $, run_conditional_branch,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Branch {
            take_true,
            raw,
            requested,
        } => {
            match &decision.branches {
                DecisionBranches::Materialized {
                    true_branch,
                    false_branch,
                    pending,
                    ..
                } => {
                    let node = if *take_true {
                        *true_branch
                    } else {
                        *false_branch
                    };
                    break DecisionValue::Node(Some(if *raw {
                        node
                    } else {
                        self.apply_conditional_branch_pending(
                            node,
                            pending.as_deref(),
                            *take_true,
                        )
                    }));
                }
                DecisionBranches::Lowered => {
                    if let Some(answer) = in_place_branches.as_mut() {
                        break DecisionValue::Node(Some(answer(*take_true)));
                    }
                    if !*requested {
                        *requested = true;
                        let branch = *take_true;
                        frame.observation = observation.map(|scope| scope.finish(()));
                        decision.ops.push(frame.take());
                        return Ok(OperationStep::Branch(branch));
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    if let Some(DecisionDelivery::Branch(node)) = delivered.take() {
                        break DecisionValue::Node(Some(node));
                    }
                    unreachable!("a lowered branch delivery")
                }
            };
        }
    }

    conditional_operation! { $, run_conditional_plain,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Plain { resolving } => {
            let named = if *resolving {
                let outcome = child!(DecisionOperation::fact(
                    self.resolved_operand_demand(decision.check)
                ))
                .fact(self)?;
                match self.resolved_operand_of(outcome) {
                    Some(named) => named,
                    None => break DecisionValue::Flag(false),
                }
            } else {
                match self.plain_check(decision.check) {
                    PlainCheck::Proven => break DecisionValue::Flag(true),
                    PlainCheck::Named(named) => named,
                    PlainCheck::Resolve => {
                        *resolving = true;
                        continue;
                    }
                }
            };
            break DecisionValue::Flag(self.plain_check_is_plain(named, decision.distributive));
        }
    }

    conditional_operation! { $, run_conditional_written_arm,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::WrittenArm { at, arms, next } => {
            if *at == 0 {
                *arms = match self.graph().node_data(decision.extends).as_deref() {
                    Some(SemanticNodeData::Union(members)) => Some(members.members_arc()),
                    _ => break DecisionValue::Flag(false),
                };
                if !matches!(
                    self.conditional_infer_route(decision.extends),
                    ConditionalInferRoute::None
                ) || !self
                    .type_params_within(&[decision.check, decision.extends])
                    .is_empty()
                {
                    break DecisionValue::Flag(false);
                }
                *at = 1;
            }
            if *at == 1 {
                let plain = child!(DecisionOperation::Plain { resolving: false });
                let plain = plain.into_owned();
                let observation = plain.observation.expect("the plain-check probe");
                if !observation.observations_are_complete()
                    || !matches!(plain.value, DecisionValue::Flag(true))
                {
                    observation.discard()?;
                    break DecisionValue::Flag(false);
                }
                observation.adopt(self)?;
                *at = 2;
            }
            let Some(&arm) = arms.as_ref().expect("written arms").get(*next) else {
                break DecisionValue::Flag(false);
            };
            let related = child!(DecisionOperation::Relation {
                check: decision.check,
                extends: arm
            });
            let related = related.into_owned();
            let observation = related.observation.expect("a written-arm relation probe");
            if observation.observations_are_complete()
                && matches!(
                    related.value,
                    DecisionValue::Relation(
                        super::dispatch_txn::RelationStep::Assignable { .. }
                    )
                )
            {
                *result_slot = Some(DecisionResult {
                    value: DecisionValue::Flag(true),
                    observation: Some(observation),
                });
                return Ok(OperationStep::Done);
            }
            observation.discard()?;
            *next += 1;
        }
    }

    conditional_operation! { $, run_conditional_relation,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Relation { check, extends } => {
            break DecisionValue::Relation(self.execute_relate_pair(*check, *extends));
        }
    }

    conditional_operation! { $, run_conditional_absorb,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Absorb {
            check,
            extends,
            at,
            branches,
            true_value,
        } => {
            use super::absorb::SpecialKind;
            match *at {
                0 => {
                    if let Some((SpecialKind::Error, node)) = self.peek_special(*check) {
                        break DecisionValue::Output(
                            self.absorbed_output(node, [*check, *extends]),
                        );
                    }
                    if let Some((SpecialKind::Error, node)) = self.peek_special(*extends) {
                        break DecisionValue::Output(
                            self.absorbed_output(node, [*check, *extends]),
                        );
                    }
                    for operand in [*check, *extends] {
                        if let Some((SpecialKind::Wildcard, node)) =
                            self.peek_special(operand)
                        {
                            break 'operation DecisionValue::Output(
                                self.absorbed_output(node, [*check, *extends]),
                            );
                        }
                    }
                    match self.peek_special(*check) {
                        Some((SpecialKind::Never, _)) if decision.distributive => {
                            break DecisionValue::Output(self.absorbed_output(
                                self.primitive_node(PrimitiveKind::Never),
                                [*check, *extends],
                            ))
                        }
                        Some((SpecialKind::Any, _))
                            if !self.extends_is_infer_pattern(*extends) =>
                        {
                            *at = 1
                        }
                        _ => break DecisionValue::Node(None),
                    }
                }
                1 => {
                    if child!(DecisionOperation::is_deferred(*check, *extends))
                        .flag(self)?
                    {
                        break DecisionValue::Node(None);
                    }
                    *at = 2;
                }
                2 => {
                    branches[0] =
                        child!(DecisionOperation::branch(true, false)).node(self)?;
                    *at = if self.declared_operand_resolves(*extends) { 7 } else { 6 };
                }
                // The `any` row reads the extends operand as the type its
                // alias names.
                6 | 7 => {
                    let declared = if *at == 7 {
                        let outcome = child!(DecisionOperation::fact(
                            self.resolved_operand_demand(*extends)
                        ))
                        .fact(self)?;
                        self.resolved_operand_of(outcome).unwrap_or(*extends)
                    } else {
                        *extends
                    };
                    if matches!(
                        self.peek_special(declared),
                        Some((SpecialKind::Any | SpecialKind::Unknown, _))
                    ) {
                        let branch = branches[0].expect("true branch");
                        break DecisionValue::Output(
                            self.absorbed_output(branch, [*check, *extends, branch]),
                        );
                    }
                    *at = 3;
                }
                3 => {
                    branches[1] =
                        child!(DecisionOperation::branch(false, false)).node(self)?;
                    *at = 4;
                }
                4 => {
                    *true_value = child!(DecisionOperation::Indexed {
                        node: branches[0].expect("true branch")
                    })
                    .node(self)?;
                    *at = 5;
                }
                5 => {
                    let false_value = child!(DecisionOperation::Indexed {
                        node: branches[1].expect("false branch")
                    })
                    .node(self)?
                    .expect("false branch value");
                    let true_branch = branches[0].expect("true branch");
                    let false_branch = branches[1].expect("false branch");
                    let union = self.intern_normalized_union_or_intersection(
                        &[true_value.expect("true branch value"), false_value],
                        true,
                    );
                    break DecisionValue::Output(self.absorbed_output(
                        union,
                        [*check, *extends, true_branch, false_branch],
                    ));
                }
                _ => unreachable!(),
            }
        }
    }

    conditional_operation! { $, run_conditional_selection,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Selection {
            at,
            check,
            extends,
            answers,
        } => {
            if *at == 0 {
                #[cfg(any(test, feature = "test-support"))]
                BRANCH_SELECTIONS.with(|count| count.set(count.get() + 1));
                if matches!(
                    self.peek_special(*check),
                    Some((super::absorb::SpecialKind::Error, _))
                ) {
                    break DecisionValue::Selection(ConditionalSelection {
                        branch: ConditionalBranchSelection::Deferred,
                        infer: None,
                        false_recovery: false,
                    });
                }
                *at = 1;
            }
            if child!(DecisionOperation::is_deferred(*check, *extends)).flag(self)? {
                break DecisionValue::Selection(ConditionalSelection {
                    branch: ConditionalBranchSelection::Deferred,
                    infer: None,
                    false_recovery: false,
                });
            }
            let (branch, infer) = if let Some(answers) = answers {
                let super::helper_continuation::HelperDrive::Frame { depth } =
                    decision.drive
                else {
                    unreachable!()
                };
                self.with_operand_answers(&mut answers.answers, depth, || {
                    self.select_non_generic_conditional(*check, *extends)
                })
            } else {
                self.select_non_generic_conditional(*check, *extends)
            };
            break DecisionValue::Selection(ConditionalSelection {
                branch,
                infer,
                false_recovery: false,
            });
        }
    }

    conditional_operation! { $, run_conditional_prepare_selection,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::PrepareSelection {
            at,
            errors,
            answers,
            selected: selected_held,
        } => {
            let operands = decision.prepared.expect("prepared operands").operands;
            if *at == 0 {
                if matches!(
                    decision.drive,
                    super::helper_continuation::HelperDrive::Frame { .. }
                ) {
                    let walked = child!(DecisionOperation::OperandWalk(Some(Box::new(ConditionalOperandWalk {
                        which: 0,
                        walk: None,
                        errors: [None; 2],
                        answers: super::query_frames::OperandDemand::empty(),
                        awaiting: None
                    }))));
                    let walked = walked.into_owned();
                    let DecisionValue::Walk(state) = walked.value else {
                        unreachable!()
                    };
                    let observed = walked.observation.expect("observed operand walks");
                    *errors = state.errors;
                    *answers = Some(state);
                    if errors.iter().any(Option::is_some) {
                        observed.adopt(self)?;
                        *at = 1;
                        continue;
                    }
                    observed.discard()?;
                }
                *at = 2;
            }
            if *at == 1 {
                let absorbed = child!(DecisionOperation::Absorb {
                    check: errors[0].unwrap_or(operands.check),
                    extends: errors[1].unwrap_or(operands.extends),
                    at: 0,
                    branches: [None; 2],
                    true_value: None
                })
                .adopt(self)?;
                if let DecisionValue::Output(output) = absorbed {
                    break DecisionValue::Output(output);
                }
                *at = 2;
            }
            if *at == 3 {
                let probed = child!(DecisionOperation::NamedError {
                    operands: [operands.check, operands.extends],
                    next: 0
                })
                .adopt(self)?;
                if let DecisionValue::Output(output) = probed {
                    break DecisionValue::Output(output);
                }
                break DecisionValue::Selection(
                    selected_held.take().expect("the selection its error probe follows"),
                );
            }
            let result = child!(DecisionOperation::Selection {
                at: 0,
                check: operands.check,
                extends: operands.extends,
                answers: answers.take()
            });
            let result = result.into_owned();
            let DecisionValue::Selection(mut selected) = result.value else {
                unreachable!()
            };
            let observation = result.observation.expect("the selection's observation");
            let complete = observation.observations_are_complete();
            let false_recovery = selected.branch == ConditionalBranchSelection::False
                && observation.operation_refusals().iter().any(|refusal| {
                    matches!(
                        refusal,
                        super::walk::ShallowDiagnostic::RelationTooComplex { source, target }
                            if *source == operands.check && *target == operands.extends
                    )
                });
            if selected.branch == ConditionalBranchSelection::Deferred {
                observation.discard()?;
            } else {
                observation.adopt(self)?;
            }
            if !complete && selected.branch != ConditionalBranchSelection::Deferred {
                if false_recovery {
                    selected.false_recovery = true;
                    break DecisionValue::Selection(selected);
                }
                selected.branch = ConditionalBranchSelection::Undecided;
                selected.infer = None;
            }
            *selected_held = Some(selected);
            *at = 3;
            continue;
        }
    }

    conditional_operation! { $, run_conditional_operand_walk,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::OperandWalk(retained) => {
            let state = retained.as_mut().expect("a standing operand walk");
            if state.which == 2 {
                break DecisionValue::Walk(retained.take().expect("the completed operand walk"));
            }
            let ConditionalOperandWalk { which, walk, errors, answers, awaiting } = state.as_mut();
            let operands = decision.prepared.expect("prepared operands").operands;
            let walker = walk.get_or_insert_with(|| {
                super::relation::IdentityCarrierRead::at(
                    super::relation::CarrierWalkPoint {
                        node: [operands.check, operands.extends][*which],
                        depth: 0,
                    },
                )
            });
            if let Some(key) = awaiting.take() {
                let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                    unreachable!("an operand walk's delivery")
                };
                let mut deliver = || self.retain_operand_answer(answers, &key, deliver);
                if let Some(key) = self.deliver_identity_carrier_read(walker, &mut deliver)
                {
                    *awaiting = Some(key.clone());
                    read!(key);
                }
            }
            match self.advance_identity_carrier_read(walker) {
                super::relation::IdentityCarrierStep::Read(key) => {
                    *awaiting = Some(key.clone());
                    read!(key);
                }
                super::relation::IdentityCarrierStep::Done(done) => {
                    if let super::relation::IdentityCarrierUnwrap::Concrete(node) = done {
                        if self.is_conditional_checker_recovery(node) {
                            errors[*which] = Some(node);
                        }
                    }
                    *walk = None;
                    *which += 1;
                }
            }
        }
    }

    #[inline(never)]
    fn advance_generic_walk(
        &self,
        walk: &mut GenericWalk,
        mut delivered: Option<
            &mut dyn FnMut() -> crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
        >,
    ) -> GenericStep {
        if !walk.started {
            if !self.mentions_generic_operand_variable(walk.root, true)
                && self.free_type_variables.borrow().contains_key(&walk.root)
            {
                return GenericStep::Done(false);
            }
            walk.pending = vec![(walk.root, walk.infer_operand)];
            walk.started = true;
        }
        if let Some((node, infer_operand)) = walk.awaiting.take() {
            let Some(deliver) = delivered.take() else {
                unreachable!("an alias application delivery")
            };
            let read = deliver();
            if !read.result_is_partial() {
                if let QueryResult::Value(named) = read.value {
                    if named == node {
                        return GenericStep::Done(true);
                    }
                    walk.pending.push((named, infer_operand));
                }
            }
        }
        let graph = self.graph();
        while let Some((node, infer_operand)) = walk.pending.pop() {
            if !walk.seen[usize::from(infer_operand)].insert(node) {
                continue;
            }
            #[cfg(any(test, feature = "test-support"))]
            note_operand_walk_visit();
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if let SemanticNodeData::InstantiationRef { base, args } = &*data {
                let base = base.clone();
                let args = Arc::clone(args.as_arc());
                drop(data);
                if !args
                    .iter()
                    .any(|arg| self.mentions_generic_operand_variable(*arg, infer_operand))
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
                        walk.pending
                            .extend(args.iter().map(|arg| (*arg, infer_operand)));
                    } else {
                        walk.pending.extend(
                            generic_in
                                .iter()
                                .filter_map(|index| args.get(*index))
                                .map(|arg| (*arg, infer_operand)),
                        );
                    }
                    continue;
                }
                let key = self.alias_application_key(&base, &args);
                walk.awaiting = Some((node, infer_operand));
                return GenericStep::Read(key);
            }
            match &*data {
                SemanticNodeData::Infer { .. } if infer_operand => {
                    return GenericStep::Done(true);
                }
                SemanticNodeData::TypeParam { .. } | SemanticNodeData::InferRef { .. } => {
                    return GenericStep::Done(true);
                }
                // A shell the lane keeps where it is written (`Rec["a"]`,
                // `keyof O`) is generic only over a generic operand; the
                // checker resolved it.
                SemanticNodeData::IndexedAccess { object, index, .. } => {
                    walk.pending.push((*object, infer_operand));
                    if let crate::semantic_query::IndexKey::Computed(index) = index {
                        walk.pending.push((*index, infer_operand));
                    }
                }
                SemanticNodeData::KeyOf { base } => walk.pending.push((*base, infer_operand)),
                SemanticNodeData::Conditional { check, extends, .. } => {
                    walk.pending.extend([(*check, true), (*extends, false)]);
                }
                SemanticNodeData::IntrinsicApplication { args, .. } => {
                    walk.pending
                        .extend(args.iter().map(|arg| (*arg, infer_operand)));
                }
                SemanticNodeData::Alias(inner) => walk.pending.push((*inner, infer_operand)),
                SemanticNodeData::Mapped { source, .. } => {
                    walk.pending.push((*source, infer_operand))
                }
                SemanticNodeData::Union(members) => walk.pending.extend(
                    members
                        .members_arc()
                        .iter()
                        .map(|member| (*member, infer_operand)),
                ),
                SemanticNodeData::Intersection(members) => {
                    walk.pending.extend(
                        members
                            .members_arc()
                            .iter()
                            .map(|member| (*member, infer_operand)),
                    );
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    walk.pending.extend(
                        expressions
                            .iter()
                            .map(|expression| (*expression, infer_operand)),
                    );
                }
                SemanticNodeData::Tuple { elements, .. } => walk.pending.extend(
                    elements
                        .iter()
                        .filter(|element| element.rest)
                        .map(|element| element.value)
                        .filter(|value| {
                            matches!(
                                graph.node_data(*value).as_deref(),
                                Some(SemanticNodeData::TypeParam { .. })
                            )
                        })
                        .map(|value| (value, infer_operand)),
                ),
                _ => {}
            }
        }
        GenericStep::Done(false)
    }

    conditional_operation! { $, run_conditional_generic,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Generic { root, infer_operand, walk: retained } => {
            let mut fresh = None;
            let walk = match retained.as_deref_mut() {
                Some(walk) => walk,
                None => fresh.insert(GenericWalk::new(*root, *infer_operand)),
            };
            let delivery = match delivered.take() {
                Some(DecisionDelivery::Read(deliver)) => Some(deliver),
                None => None,
                #[cfg(any(test, feature = "test-support"))]
                Some(DecisionDelivery::Branch(_)) => unreachable!("a generic walk reads a query"),
            };
            match self.advance_generic_walk(walk, delivery) {
                GenericStep::Done(generic) => break DecisionValue::Flag(generic),
                GenericStep::Read(key) => {
                    if let Some(walk) = fresh.take() {
                        *retained = Some(Box::new(walk));
                    }
                    read!(key);
                },
            }
        }
    }

    conditional_operation! { $, run_conditional_member,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Member(member) => {
            match self.advance_conditional_decision_with_branches(
                member,
                delivered.take(),
                in_place_branches,
            ) {
                DecisionStep::Done(output) => break DecisionValue::Output(*output),
                DecisionStep::Read(key) => {
                    read!(*key);
                }
                DecisionStep::Branch(branch) => {
                    frame.observation = observation.map(|scope| scope.finish(()));
                    decision.ops.push(frame.take());
                    return Ok(OperationStep::Branch(branch));
                }
            }
        }
    }

    conditional_operation! { $, run_conditional_distribution,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Distribution { at } => {
            if *at == 0 {
                let distribution = decision.distribution.as_ref().expect("a distribution");
                if self.cancellation.is_cancelled() {
                    break DecisionValue::Output(self.cancelled_build_output());
                }
                if let Some(&member) =
                    distribution.members.get(distribution.per_member.len())
                {
                    let extends = decision
                        .prepared
                        .expect("prepared operands")
                        .operands
                        .extends;
                    let output = match &mut decision.branches {
                        DecisionBranches::Materialized {
                            true_branch,
                            false_branch,
                            pending,
                            literal_extends,
                        } => {
                            let pending = pending.as_ref().map(|pending| {
                                pending.distributed_over(member).map_or_else(
                                    || pending.clone(),
                                    verter_session_query::retention::owned::Owned::new,
                                )
                            });
                            let literal_extends = *literal_extends
                                .get_or_insert_with(|| self.is_literal_only_union(extends));
                            let key = SemanticQueryKey::Conditional {
                                check: member,
                                extends,
                                true_branch: *true_branch,
                                false_branch: *false_branch,
                                distributive: false,
                                pending: pending.clone(),
                            };
                            if literal_extends && self.decides_member_in_place(member) {
                                // Charged and refused exactly as the member's
                                // own query would be: a refusal is that
                                // member's answer, folded like any other.
                                match self.enter_in_place_query(&key) {
                                    Ok(_entry) => {
                                        let _nested = self.nested_decision_query();
                                        let mut child = self.begin_conditional_decision(
                                            member,
                                            extends,
                                            false,
                                            DecisionBranches::Materialized {
                                                true_branch: *true_branch,
                                                false_branch: *false_branch,
                                                pending,
                                                literal_extends: None,
                                            },
                                            super::helper_continuation::HelperDrive::InPlace,
                                        );
                                        self.drive_conditional_in_place(&mut child, &mut |_| {
                                            unreachable!("materialized branches")
                                        })
                                    }
                                    Err(refusal) => *refusal,
                                }
                            } else {
                                *at = 1;
                                // A member's observation is retained independently of
                                // completed members; each delivery is folded once.
                                read!(key);
                            }
                        }
                        DecisionBranches::Lowered => {
                            if let Some(branches) = in_place_branches.as_deref_mut() {
                                let mut child = self.begin_conditional_decision(
                                    member,
                                    extends,
                                    false,
                                    DecisionBranches::Lowered,
                                    super::helper_continuation::HelperDrive::InPlace,
                                );
                                self.drive_conditional_in_place(&mut child, branches)
                            } else {
                                let result = child!(DecisionOperation::Member(Box::new(
                                    self.begin_conditional_decision(
                                        member,
                                        extends,
                                        false,
                                        DecisionBranches::Lowered,
                                        super::helper_continuation::HelperDrive::InPlace
                                    )
                                )))
                                .adopt(self)?;
                                let DecisionValue::Output(output) = result else {
                                    unreachable!()
                                };
                                output
                            }
                        }
                    };
                    if let Some(scope) = observation.take() {
                        scope.finish(()).adopt(self)?;
                    }
                    if !self.fold_decision_member(decision, output) {
                        break DecisionValue::Node(None);
                    }
                    observation = Some(self.begin_read_observation());
                    continue;
                }
                *at = 2;
            }
            if *at == 1 {
                let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                    unreachable!("a distributed member delivery")
                };
                let output = self.conditional_read_output(deliver());
                if let Some(scope) = observation.take() {
                    scope.finish(()).adopt(self)?;
                }
                if !self.fold_decision_member(decision, output) {
                    break DecisionValue::Node(None);
                }
                observation = Some(self.begin_read_observation());
                *at = 0;
                continue;
            }
            if *at == 2 {
                let distribution = decision.distribution.as_mut().expect("a distribution");
                let members = distribution.normalization_members.get_or_insert_with(|| {
                    super::canonicalize_node_list(
                        self.graph().store(),
                        &distribution.per_member,
                    )
                });
                let key = SemanticQueryKey::ReduceUnion {
                    members: Arc::clone(members),
                    nullability:
                        verter_session_query::flow::policy::NullabilityPolicy::Strict,
                };
                *at = 3;
                read!(key);
            }
            let Some(DecisionDelivery::Read(deliver)) = delivered.take() else {
                unreachable!("the final union delivery")
            };
            let normalized = self.conditional_read_output(deliver());
            let mut distribution = decision
                .distribution
                .take()
                .expect("the completed distribution");
            distribution.positions_complete &=
                Self::conditional_position_complete(&normalized);
            let output = &mut distribution.output;
            output.cache_suppress |= normalized.cache_suppress;
            output.fold_partial(normalized.result_is_partial());
            output.add_partial_reasons(normalized.partial_reasons());
            output.result = normalized.result;
            output.approximation = distribution
                .positions_complete
                .then_some(super::projection_fact::ProjectionInterpretation::Positional);
            break DecisionValue::Output(distribution.output);
        }
    }

    conditional_operation! { $, run_conditional_winner,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Winner { at, winner, other } => {
            let prepared = decision.prepared.expect("prepared operands");
            let operands = prepared.operands;
            let selected = decision
                .selected
                .as_ref()
                .expect("a completed selection")
                .clone();
            if self.cancellation.is_cancelled() {
                break DecisionValue::Output(self.cancelled_build_output());
            }
            let selection = selected.branch;
            let keep = matches!(
                selection,
                ConditionalBranchSelection::Deferred
                    | ConditionalBranchSelection::Undecided
            );
            let both = selection == ConditionalBranchSelection::Both;
            let take_true =
                !selected.false_recovery && selection != ConditionalBranchSelection::False;
            if *at == 0 {
                *winner = child!(DecisionOperation::branch(
                    if keep { true } else { take_true },
                    keep
                ))
                .node(self)?;
                *at = 1;
            }
            if *at == 1 {
                if keep || both {
                    if keep && self.cancellation.is_cancelled() {
                        break DecisionValue::Output(self.cancelled_build_output());
                    }
                    *other = child!(DecisionOperation::branch(false, keep)).node(self)?;
                }
                *at = 2;
            }
            let winner_node = winner.expect("a winning or kept branch");
            if keep {
                break DecisionValue::Output(self.keep_conditional(
                    operands.check,
                    operands.extends,
                    (winner_node, other.expect("a false branch")),
                    decision.distributive,
                    decision.branches.pending(),
                    selection,
                ));
            }
            if selected.false_recovery {
                let mut output = super::walk::QueryBuildOutput::from((
                    QueryResult::Value(winner_node),
                    self.project_generation_signature(),
                ))
                .with_observed_self_roots(
                    self.observed_self_roots_from_nodes([
                        operands.check,
                        operands.extends,
                        winner_node,
                    ]),
                );
                output.approximation =
                    Some(super::projection_fact::ProjectionInterpretation::CheckerRecovery);
                decision.branches.committed(self, false, &mut output);
                break DecisionValue::Output(output);
            }
            if *at == 2 {
                let result = if both {
                    self.intern_normalized_union(
                        &[winner_node, other.expect("false branch")],
                        verter_session_query::flow::policy::NullabilityPolicy::from_strict_null_checks(
                            self.relation_strict_config().strict_null_checks,
                        ),
                    )
                } else {
                    winner_node
                };
                *winner = Some(if decision.check != prepared.demanded_check {
                    self.substitute_semantic_type_param(
                        result,
                        decision.check,
                        prepared.demanded_check,
                    )
                } else {
                    result
                });
                *at = 3;
            }
            let result = child!(DecisionOperation::commit(
                operands.check,
                operands.extends,
                winner.expect("the substituted winner"),
                selection,
                selected.infer.clone(),
                operands.checker_recovery
            ))
            .adopt(self)?;
            let DecisionValue::Output(mut output) = result else {
                unreachable!()
            };
            decision.branches.committed(self, take_true, &mut output);
            if both {
                decision.branches.committed(self, false, &mut output);
            }
            break DecisionValue::Output(output);
        }
    }

    conditional_operation! { $, run_conditional_commit,
        [self, decision, frame, delivered, remaining, in_place_branches, result_slot, observation, child, read, 'operation],
        DecisionOperation::Commit {
            check,
            extends,
            winner,
            result,
            selection,
            infer,
            checker_recovery,
            at,
        } => {
            if self.cancellation.is_cancelled() {
                break DecisionValue::Output(self.cancelled_build_output());
            }
            if *at == 0 {
                if let Some(infer) = infer {
                    for binding in infer.bindings.iter() {
                        *result = self.substitute_semantic_type_param(
                            *result,
                            binding.param,
                            binding.bound,
                        );
                        crate::semantic_query_memo::record_origin_edge!(
                            self.graph(),
                            *result,
                            OriginEdgeKind::InferBind,
                            Arc::from([*check, *extends]),
                            OriginMeta::SubstitutedParam(binding.name.clone()),
                            self.project_generation_signature()
                        );
                    }
                }
                *at = 1;
            }
            let mut selected_output = None;
            if infer.is_some()
                && matches!(
                    self.graph().node_data(*result).as_deref(),
                    Some(SemanticNodeData::Conditional { .. })
                )
            {
                let output = child!(DecisionOperation::deferred(
                    *result,
                    ProjectionReductionContext::published(ProjectionMode::Expanded)
                ))
                .adopt(self)?;
                let DecisionValue::Deferred(outcome) = output else {
                    unreachable!()
                };
                let output = outcome.into_query_build_output(self);
                match output.result {
                    QueryResult::Value(node) => *result = node,
                    _ => break DecisionValue::Output(output),
                }
                selected_output = Some(output);
            }
            if self.cancellation.is_cancelled() {
                break DecisionValue::Output(self.cancelled_build_output());
            }
            let graph = self.graph();
            let fence = self.project_generation_signature();
            if !*checker_recovery {
                let branch = match selection {
                    ConditionalBranchSelection::True => {
                        graph.record_branch_selection_true();
                        Some(BranchSelection::True)
                    }
                    ConditionalBranchSelection::False => {
                        graph.record_branch_selection_false();
                        Some(BranchSelection::False)
                    }
                    ConditionalBranchSelection::Both => None,
                    _ => unreachable!("only decided winners are committed"),
                };
                if let Some(branch) = branch {
                    crate::semantic_query_memo::record_origin_edge!(
                        graph,
                        *result,
                        OriginEdgeKind::ConditionalSelect,
                        Arc::from([*check, *extends]),
                        OriginMeta::Branch(branch),
                        fence.clone()
                    );
                }
                graph.record_conditional_decided();
            }
            let mut output =
                selected_output
                    .unwrap_or_else(|| {
                        super::walk::QueryBuildOutput::from((
                            QueryResult::Value(*result),
                            fence,
                        ))
                    })
                    .with_observed_self_roots(self.observed_self_roots_from_nodes([
                        *check, *extends, *winner, *result,
                    ]));
            if *checker_recovery && !output.completeness.is_partial() {
                output.approximation =
                    Some(super::projection_fact::ProjectionInterpretation::CheckerRecovery);
            }
            break DecisionValue::Output(output);
        }
    }
}

impl ConditionalDecision {
    pub(super) fn reads_for_carrier(&self) -> bool {
        match self.ops.last().map(|op| &op.operation) {
            Some(DecisionOperation::Deferred {
                read: Some(read), ..
            }) => read.reads_for_carrier(),
            Some(DecisionOperation::Fact {
                read: Some(read), ..
            }) => read.reads_for_carrier(),
            Some(DecisionOperation::OperandWalk(Some(state))) => state
                .walk
                .as_ref()
                .is_some_and(|walk| walk.reads_for_carrier()),
            Some(DecisionOperation::Member(member)) => member.reads_for_carrier(),
            Some(DecisionOperation::Reduced { .. }) => true,
            _ => false,
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub struct ConditionalReadForTests {
    decision: Option<ConditionalDecision>,
}

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static CONDITIONAL_READ_TRACE: std::cell::RefCell<Option<Vec<SemanticQueryKey>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
pub enum ConditionalProgressForTests {
    Done(Box<super::walk::QueryBuildOutput>),
    Read(SemanticQueryKey),
    Branch(bool),
}

#[cfg(any(test, feature = "test-support"))]
impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    pub fn with_conditional_read_for_tests<R>(
        &self,
        key: &SemanticQueryKey,
        lowered: bool,
        framed: bool,
        body: impl FnOnce(&mut ConditionalReadForTests) -> R,
    ) -> R {
        struct Held<'s, 'a, C: crate::resolver_core::ResolverCapabilities> {
            dispatch: &'s ProjectSemanticDispatch<'a, C>,
            read: ConditionalReadForTests,
        }
        impl<C: crate::resolver_core::ResolverCapabilities> Drop for Held<'_, '_, C> {
            fn drop(&mut self) {
                if let Some(decision) = self.read.decision.take() {
                    self.dispatch.abandon_conditional_decision(decision);
                }
            }
        }
        let (_connected, _) = self.enter_connected_demand(false);
        let SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } = key
        else {
            panic!("a conditional fixture key")
        };
        let branches = if lowered {
            DecisionBranches::Lowered
        } else {
            DecisionBranches::Materialized {
                true_branch: *true_branch,
                false_branch: *false_branch,
                pending: pending.clone(),
                literal_extends: None,
            }
        };
        let drive = if framed {
            super::helper_continuation::HelperDrive::Frame {
                depth: u32::try_from(self.dispatch_txn.borrow().reentry().depth() + 1)
                    .expect("a fixture frame has bounded depth"),
            }
        } else {
            super::helper_continuation::HelperDrive::InPlace
        };
        let decision =
            self.begin_conditional_decision(*check, *extends, *distributive, branches, drive);
        let mut held = Held {
            dispatch: self,
            read: ConditionalReadForTests {
                decision: Some(decision),
            },
        };
        body(&mut held.read)
    }

    pub fn advance_conditional_read_for_tests(
        &self,
        read: &mut ConditionalReadForTests,
        delivered: Option<
            &mut dyn FnMut() -> crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
        >,
        branch: Option<SemanticNodeId>,
    ) -> ConditionalProgressForTests {
        let delivery = match (delivered, branch) {
            (Some(deliver), None) => Some(DecisionDelivery::Read(deliver)),
            (None, Some(node)) => Some(DecisionDelivery::Branch(node)),
            (None, None) => None,
            _ => panic!("one conditional delivery"),
        };
        match self.advance_conditional_decision(
            read.decision.as_mut().expect("an unfinished decision"),
            delivery,
        ) {
            DecisionStep::Done(output) => {
                read.decision = None;
                ConditionalProgressForTests::Done(output)
            }
            DecisionStep::Read(key) => ConditionalProgressForTests::Read(*key),
            DecisionStep::Branch(branch) => ConditionalProgressForTests::Branch(branch),
        }
    }

    pub fn abandon_conditional_read_for_tests(&self, read: &mut ConditionalReadForTests) {
        if let Some(decision) = read.decision.take() {
            self.abandon_conditional_decision(decision);
        }
    }

    pub fn conditional_observation_depth_for_tests(&self) -> usize {
        self.build_local_taint.borrow().len()
    }

    /// Decide the materialized conditional `key` through the in-place
    /// decision driver, the driver a literal member decided in place takes,
    /// rather than through its family continuation.
    pub fn decide_materialized_conditional_in_place_for_tests(
        &self,
        key: &SemanticQueryKey,
    ) -> super::walk::QueryBuildOutput {
        let SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } = key
        else {
            panic!("a conditional key: {key:?}");
        };
        let mut decision = self.begin_conditional_decision(
            *check,
            *extends,
            *distributive,
            DecisionBranches::Materialized {
                true_branch: *true_branch,
                false_branch: *false_branch,
                pending: pending.clone(),
                literal_extends: None,
            },
            super::helper_continuation::HelperDrive::InPlace,
        );
        self.drive_conditional_in_place(&mut decision, &mut |_| {
            unreachable!("materialized branches")
        })
    }

    pub fn conditional_build_from_lowering_for_tests(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        branches: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> super::walk::QueryBuildOutput {
        self.build_conditional_from_lowering(check, extends, distributive, branches)
    }

    pub fn trace_conditional_from_lowering_for_tests(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        distributive: bool,
        branches: &mut dyn FnMut(bool) -> SemanticNodeId,
    ) -> (super::walk::QueryBuildOutput, Vec<SemanticQueryKey>) {
        struct Restore(Option<Vec<SemanticQueryKey>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CONDITIONAL_READ_TRACE.with(|trace| *trace.borrow_mut() = self.0.take());
            }
        }
        let outer = CONDITIONAL_READ_TRACE.with(|trace| trace.replace(Some(Vec::new())));
        let _restore = Restore(outer);
        let output = self.build_conditional_from_lowering(check, extends, distributive, branches);
        let trace =
            CONDITIONAL_READ_TRACE.with(|trace| trace.take().expect("an active decision trace"));
        (output, trace)
    }

    pub fn conditional_standing_operation_for_tests(
        &self,
        read: &ConditionalReadForTests,
    ) -> &'static str {
        fn standing(decision: &ConditionalDecision) -> &'static str {
            match decision.ops.last().map(|op| &op.operation) {
                Some(DecisionOperation::Deferred { .. }) => "evaluation",
                Some(DecisionOperation::Fact { .. }) => "structural fact",
                Some(DecisionOperation::KeySet { .. }) => "key set",
                Some(DecisionOperation::Generic { .. }) => "genericness",
                Some(DecisionOperation::Template { .. }) => "template",
                Some(DecisionOperation::Reduced { .. }) => "conditional operand",
                Some(DecisionOperation::Branch { .. }) => "branch",
                Some(DecisionOperation::Member(member)) => standing(member),
                Some(DecisionOperation::Distribution { .. }) => "distribution",
                Some(DecisionOperation::OperandWalk(..)) => "operand walk",
                _ => "complete",
            }
        }
        read.decision.as_ref().map_or("complete", standing)
    }

    pub fn taint_conditional_probe_for_tests(&self) {
        self.fold_local_partial_completeness(crate::semantic_query::PartialReasonSet::PROPAGATED);
    }
}
