//! `evaluate_deferred_semantic_node` — deferred-shell evaluation
//! fix-point loop ( Change Split + §2 guard contract row for
//! `evaluate_deferred_semantic_node`).
//!
//! Walks `SemanticNodeData` unwrapping `Alias(target)` hops,
//! substituting `Instantiate` shells, and projecting single-segment
//! `IndexedAccess` shells through dispatch re-entry. Returns the
//! caller's current node on cyclic re-entry (fix-point) per
//! Also hosts `normalized_index_key_node` which belongs to the
//! evaluation surface.

use std::sync::Arc;

use verter_session_query::facts::fact_read_set::FactReadSetFinalise;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    CacheRead, IndexKey, LiteralValue, PartialReasonSet, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, ResolveDeclKey, ResultCompleteness,
    ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// How the checker prints an application of a named declaration — the
/// altitude the declaration-keeping structural-fact demand stops at (see
/// `ProjectSemanticDispatch::printed_declaration`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintedDeclaration {
    /// An interface or class, or a mapped utility application the checker
    /// names (`Partial<Face>`): printed by its name.
    Named,
    /// An alias the checker prints by name when its application settles
    /// on the type the alias constructs.
    AliasNamed,
    /// An alias whose body is an intersection: the checker names only the
    /// intersection or the distributed union its application constructs
    /// (`type NN<T> = T & {}` prints `NN<string | number | null>`), never
    /// a constituent the intersection reduced to (`NN<{ a: string } |
    /// null>` prints `{ a: string; }`, `NN<unknown>` prints `{}`) nor an
    /// argument it returned as it is (`type NU<T> = T & unknown` prints
    /// `NU<string | null>` as `string | null`).
    AliasNamedIntersection,
    /// An alias the checker prints as the application its body writes: a
    /// reference to a non-generic declaration (`type ToFace = Face` prints
    /// `Face`) or a homomorphic mapped application (`type P<T> =
    /// Partial<T>` prints `P<{ a: 1 }>` as `Partial<{ a: 1; }>`).
    AliasThrough,
    /// An alias the checker prints as what it resolves to.
    AliasTransparent,
}

/// One step of the printing walk over a declaration.
enum DeclarationPrinting {
    /// The declaration prints so (`None` when its kind or body cannot be
    /// recovered).
    Printed(Option<PrintedDeclaration>),
    /// The declaration's body references this declaration with these
    /// declared arguments.
    Reference(crate::semantic_query::DeclIdentity, Arc<[SemanticNodeId]>),
}

/// How an alias whose body references a declaration prints.
enum ReferencePrinting {
    /// Printed so.
    Printed(PrintedDeclaration),
    /// As the referenced alias's own declaration prints: named by the
    /// outer alias unless the target prints as what it resolves to.
    AsTarget,
}

/// A builtin mapped utility, by whether its mapping is homomorphic (its
/// keys are `keyof` its source).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuiltinMappedUtility {
    /// `Partial` / `Required` / `Readonly`.
    Homomorphic,
    /// `Pick` / `Record` / `Omit`.
    Keyed,
}

impl BuiltinMappedUtility {
    fn of(name: &str) -> Option<Self> {
        match name {
            "Partial" | "Required" | "Readonly" => Some(Self::Homomorphic),
            "Pick" | "Record" | "Omit" => Some(Self::Keyed),
            _ => None,
        }
    }
}

/// Whether `identity` names a builtin (lib) declaration.
fn is_builtin(identity: &crate::semantic_query::DeclIdentity) -> bool {
    identity.canonical_id.as_ref() == "__builtin__"
}

/// Map a residual-carrier resolution read's `QueryError` onto the demand
/// loop's typed exit classification.
///
/// An honest `Miss` is a STABLE stop (`None`): an unresolved authored name
/// is a valid semantic `Unknown` — a legitimate classification input, not
/// operational partiality (over-partializing it would wrongly refuse every
/// read touching a genuinely-unknown name). Budget exhaustion and the
/// completion-fence unstable state map to their dedicated reasons; every
/// other non-`Miss` fault is a [`PartialReasonSet::SEMANTIC_QUERY_FAULT`].
/// (`QueryResult::Recursive` never reaches this mapping — the caller
/// classifies it as [`PartialReasonSet::SAME_PATH_RECURSION`] directly.)
fn demand_read_fault_reasons(err: &QueryError) -> Option<PartialReasonSet> {
    match err {
        QueryError::Miss => None,
        QueryError::BudgetExceeded(_) => Some(PartialReasonSet::BUDGET_EXCEEDED),
        QueryError::UnstableState { .. } => Some(PartialReasonSet::UNSTABLE_STATE),
        _ => Some(PartialReasonSet::SEMANTIC_QUERY_FAULT),
    }
}

/// Entry-scoped outcome of a deferred-shell evaluation: the resolved
/// `node` PLUS the typed [`ResultCompleteness`] of the evaluation that
/// produced it — a nested read tripped `BudgetExceeded` / recursion / a
/// fatal walker miss (the boolean-bridge fold, lifted as
/// [`PartialReasonSet::PROPAGATED`]), a connected operational limit fired, OR
/// an operand evaluation was itself partial (its exact reasons merge through).
/// The completeness is the entry-scoped admission authority for
/// `evaluate_deferred_memo`: only a `Complete` result is published, so a
/// budget-tainted result is withheld REGARDLESS of whether a
/// `RequestContext` is installed (the request-global suppress sticky is
/// NOT the authority — see [`ProjectSemanticDispatch::evaluate_deferred_outcome`]).
///
/// The `cache_suppress` bit is the OR of every nested read's
/// [`CacheRead::cache_suppress`](crate::semantic_query::CacheRead) observed
/// while producing this outcome — inner-memo non-cacheability that is BENIGN
/// (a torn / unrootable self-root, a `ReturnOnly` cross-owner-reuse
/// admission, a fenced serve) but distinct from a partial
/// result. A `Complete` outcome can still carry `cache_suppress = true`; that
/// signal is NOT reconstructible from the node, so it rides the outcome so
/// [`Self::into_active_query_build_node`] can fold it into the active build
/// frame EVEN on a `Complete` outcome (memo non-admission).
///
/// All fields are PRIVATE to this module: outside `evaluate.rs` the ONLY
/// bare-node escape is [`Self::into_active_query_build_node`], which
/// folds the completeness AND the suppress bit into the active propagation
/// channels first. A caller cannot read `.node` and drop those signals — the
/// compiler's field-privacy boundary is the fail-closed rail. Typed query
/// forwarding uses [`Self::into_query_build_output`] to retain availability.
#[must_use]
#[derive(Clone, Copy)]
pub(super) struct EvaluateDeferredOutcome {
    node: SemanticNodeId,
    completeness: ResultCompleteness,
    availability: crate::semantic_query::FactStatus,
    /// OR of every nested read's `cache_suppress` observed while producing
    /// this outcome. Orthogonal to `completeness`: a `Complete` result may be
    /// non-cacheable. See the struct-level docs.
    cache_suppress: bool,
}

/// Argument preparation retains its evaluation rails until the consuming
/// demand chooses the typed fact path or the active build path.
#[must_use]
pub(super) struct CarrierArgumentsOutcome {
    arguments: Arc<[SemanticNodeId]>,
    completeness: ResultCompleteness,
    availability: crate::semantic_query::FactStatus,
    cache_suppress: bool,
}

impl CarrierArgumentsOutcome {
    pub(super) fn into_active_query_build_arguments<
        C: crate::resolver_core::ResolverCapabilities,
    >(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Arc<[SemanticNodeId]> {
        fold_active_evaluation_observations(dispatch, self.completeness, self.cache_suppress);
        self.arguments
    }
}

impl EvaluateDeferredOutcome {
    /// Forward a completed deferred reduction without losing the adopted
    /// child facts. A stopped evaluation cannot publish its provisional node.
    pub(super) fn into_query_build_output<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> super::walk::QueryBuildOutput {
        let node = self.into_active_query_build_node(dispatch);
        self.forwarded_output(node, dispatch.project_generation_signature())
    }

    fn forwarded_output(
        self,
        node: SemanticNodeId,
        fence: crate::semantic_query::DepSignature,
    ) -> super::walk::QueryBuildOutput {
        let result = match self.availability {
            crate::semantic_query::FactStatus::Complete
            | crate::semantic_query::FactStatus::Approximate(_, _) => QueryResult::Value(node),
            crate::semantic_query::FactStatus::Unavailable(causes) => {
                QueryResult::Error(QueryError::IncompleteSemanticOperand {
                    reasons: causes.get(),
                })
            }
        };
        let mut output = super::walk::QueryBuildOutput::from((result, fence));
        output.merge_completeness(self.completeness);
        output.cache_suppress = self.cache_suppress;
        output.approximation = self.availability.interpretation();
        output
    }

    /// A complete, cacheable, warm-admissible result.
    fn complete(node: SemanticNodeId) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::Complete,
            availability: crate::semantic_query::FactStatus::Complete,
            cache_suppress: false,
        }
    }

    /// A partial (never-published) carrier-stop result carrying `reasons`.
    fn partial(node: SemanticNodeId, reasons: PartialReasonSet) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::partial(reasons),
            availability: crate::semantic_query::FactStatus::unavailable(reasons),
            cache_suppress: false,
        }
    }

    fn aborted(node: SemanticNodeId, abort: crate::semantic_query::ExecutionAbort) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::partial(abort.reason_class()),
            availability: crate::semantic_query::FactStatus::unavailable(abort.reason_class()),
            cache_suppress: true,
        }
    }

    /// The build-scoped escape from the typed outcome: fold the exact
    /// completeness into the ACTIVE query-build propagation channels, then
    /// return the carrier-stop node.
    ///
    /// The fold is dual-channel and runs BEFORE the node is released:
    ///
    /// 1. [`crate::request_context::fold_result_completeness`] — the
    ///    request-scoped sticky suppress + the per-cold-compute completeness
    ///    scope (exact reason set preserved; a `Complete` outcome is a
    ///    no-op there).
    /// 2. On `Partial`: `result_is_partial = true` + `cache_suppress = true`
    ///    into the TOP [`ProjectSemanticDispatch::build_local_taint`] frame —
    ///    the durable admission authority for the enclosing query build
    ///    (the cold-build `BuildLocalTaintGuard` or the relation engine's
    ///    frame). This channel works with NO `RequestContext` installed,
    ///    which is exactly the hole the request sticky cannot cover: the
    ///    recursion-ceiling partial is produced WITHOUT a `CacheRead`, so
    ///    the universal read-boundary fold never sees it.
    ///
    /// The frame requirement bites whenever there is ANYTHING to fold — a
    /// `Partial` outcome OR a `Complete`-with-`cache_suppress`. Either signal
    /// lives ONLY in the active taint frame (a `Complete`-with-suppress is not
    /// reconstructible from the node, and `fold_cache_read_rails` drops a
    /// frameless suppress at the read boundary), so releasing the node with no
    /// active frame would SILENTLY ERASE it — the exact build-scoped escape
    /// hatch this projection exists to close (debug-asserted below). A
    /// genuinely frameless PURE `Complete` (no partial, no suppress) folds
    /// nothing and is permitted (e.g. a build-internal unit test driving a
    /// concrete path directly). Non-build consumers read the typed demand
    /// outcome ([`StructuralFactDemandOutcome`]) instead of this projection.
    pub(super) fn into_active_query_build_node<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> SemanticNodeId {
        fold_active_evaluation_observations(dispatch, self.completeness, self.cache_suppress);
        self.node
    }

    /// The evaluated node of a complete evaluation, released as
    /// [`Self::into_active_query_build_node`] releases it; `None` for a
    /// partial one, whose caller keeps the operand it asked about. The
    /// partial still folds into the enclosing build and the request: the
    /// build that keeps the unevaluated operand is partial and never
    /// admitted, whatever else its caller reads afterwards.
    pub(super) fn into_complete_active_query_build_node<
        C: crate::resolver_core::ResolverCapabilities,
    >(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> Option<SemanticNodeId> {
        #[cfg(any(test, feature = "test-support"))]
        let this = if FORCE_PARTIAL_CLOSED_EVALUATION.get() {
            Self {
                completeness: ResultCompleteness::partial(PartialReasonSet::PROJECTION_WORK_LIMIT),
                ..self
            }
        } else {
            self
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let this = self;
        match this.completeness {
            ResultCompleteness::Complete => Some(this.into_active_query_build_node(dispatch)),
            ResultCompleteness::Partial(reasons) => {
                dispatch.fold_local_partial_completeness(reasons);
                None
            }
        }
    }
}

fn fold_active_evaluation_observations<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    completeness: ResultCompleteness,
    cache_suppress: bool,
) {
    let is_partial = matches!(completeness, ResultCompleteness::Partial(_));
    if is_partial || cache_suppress {
        // The frame requirement bites when there is a partial OR a suppress
        // to fold — the sole moments a dropped signal becomes the escape
        // hatch. A frameless pure-`Complete` (nothing to fold) is permitted.
        verter_debug_assert!(
            !dispatch.build_local_taint.borrow().is_empty(),
            "into_active_query_build_node released a Partial or cache-suppressed node with \
                 no active cold-build/relation taint frame: the completeness / suppress signal \
                 would be silently erased. A build-scoped caller must run inside a frame; a \
                 non-build caller must consume the typed StructuralFactDemandOutcome instead."
        );
    }
    match completeness {
        ResultCompleteness::Partial(reasons) => {
            // Partial folds BOTH channels (`result_is_partial` +
            // `cache_suppress`) into the frame and the request scope; the
            // suppress bit is subsumed.
            dispatch.fold_local_partial_completeness(reasons);
        }
        ResultCompleteness::Complete => {
            if cache_suppress {
                // A benign non-cacheable but COMPLETE evaluation: taint ONLY
                // the frame's `cache_suppress` (enclosing-build memo
                // non-admission), NOT the request partial sticky — a
                // complete-but-non-cacheable result must still warm the
                // component-meta result. Mirrors `fold_cache_read_rails`'s
                // `cache_suppress`-only fold.
                dispatch.fold_into_top_build_local_taint(false, true);
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static FORCE_PARTIAL_CLOSED_EVALUATION: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Test-only: every closed-operator evaluation on this thread ends partial
/// on the work rail, as a trip inside it does, while the guard lives.
#[cfg(any(test, feature = "test-support"))]
pub fn force_partial_closed_operator_evaluation_for_tests() -> impl Drop {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            FORCE_PARTIAL_CLOSED_EVALUATION.set(self.0);
        }
    }
    Restore(FORCE_PARTIAL_CLOSED_EVALUATION.replace(true))
}

/// Shell evaluation follows only operator operands. A selected result also
/// reads value composition, and whole publication reads surface interiors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DeferredEvaluationDemand {
    Shell,
    SelectedValue,
    /// A mapped index consumes the instantiated value, rather than retaining
    /// a named application for a later member demand or call-return display.
    MappedIndexValue,
}

/// Heap-owned continuation for one deferred-operator evaluation entry. A
/// frame advances alias/fix-point hops in place and suspends only when the
/// current operator needs an operand evaluated first. No frame is represented
/// by a Rust call frame.
enum DeferredEvaluationStage {
    EvaluateCurrent,
    AwaitKeyOfBase,
    AwaitIndexedObject {
        index: IndexKey,
    },
    AwaitIndexedIndex {
        object: SemanticNodeId,
    },
    AwaitValueChildren {
        data: Arc<SemanticNodeData>,
        children: Vec<SemanticNodeId>,
        values: Vec<SemanticNodeId>,
    },
}

struct DeferredEvaluationFrame {
    entry_node: SemanticNodeId,
    node: SemanticNodeId,
    context: ProjectionReductionContext,
    demand: DeferredEvaluationDemand,
    memo_checked: bool,
    visited: VisitedNodes,
    completeness: ResultCompleteness,
    availability: crate::semantic_query::FactStatus,
    cache_suppress: bool,
    stage: DeferredEvaluationStage,
    /// The dispatcher's operation-budget epoch when the frame began: a
    /// recovery made under it leaves the frame a resource partial.
    budget_epoch: u64,
    /// Whether the frame's evaluation is recording its cost: from the
    /// memo miss that made it evaluate until it finishes.
    recording: bool,
}

impl DeferredEvaluationFrame {
    fn new(
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        budget_epoch: u64,
        demand: DeferredEvaluationDemand,
    ) -> Self {
        let mut visited = VisitedNodes::default();
        visited.insert(node);
        Self {
            entry_node: node,
            node,
            context,
            demand,
            memo_checked: false,
            visited,
            completeness: ResultCompleteness::Complete,
            availability: crate::semantic_query::FactStatus::Complete,
            cache_suppress: false,
            stage: DeferredEvaluationStage::EvaluateCurrent,
            budget_epoch,
            recording: false,
        }
    }

    fn merge_child(&mut self, child: EvaluateDeferredOutcome) {
        self.completeness = self.completeness.merge(child.completeness);
        self.availability = self.availability.join(child.availability);
        self.cache_suppress |= child.cache_suppress;
    }

    fn merge_read<T>(&mut self, read: &CacheRead<T>) {
        self.completeness = self
            .completeness
            .or_partial_if(read.result_is_partial(), read.partial_reason_classes());
        self.cache_suppress |= read.cache_suppress;
        self.availability = self.availability.join(read.fact_status);
    }

    fn advance_or_finish(&mut self, next: SemanticNodeId) -> DeferredEvaluationAction {
        if next == self.node || !self.visited.insert(next) {
            DeferredEvaluationAction::Finish(self.node)
        } else {
            self.node = next;
            DeferredEvaluationAction::Continue
        }
    }
}

/// The nodes one evaluation frame has advanced through. A frame advances
/// a few steps, so the set is a short list until it grows wide enough to
/// need a table.
#[derive(Default)]
struct VisitedNodes {
    listed: smallvec::SmallVec<[SemanticNodeId; 8]>,
    wide: Option<rustc_hash::FxHashSet<SemanticNodeId>>,
}

impl VisitedNodes {
    /// Up to this many nodes, membership is a linear scan.
    const LINEAR_SCAN_MAX: usize = 16;

    /// Add `node`; `false` when it was already visited.
    fn insert(&mut self, node: SemanticNodeId) -> bool {
        if let Some(wide) = &mut self.wide {
            return wide.insert(node);
        }
        for listed in self.listed.iter() {
            if listed.0 == node.0 {
                return false;
            }
        }
        self.listed.push(node);
        if self.listed.len() > Self::LINEAR_SCAN_MAX {
            self.wide = Some(self.listed.drain(..).collect());
        }
        true
    }
}

enum DeferredEvaluationAction {
    Continue,
    Push {
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    },
    Finish(SemanticNodeId),
    Cached(SemanticNodeId),
}

fn clone_index_key(index: &IndexKey) -> IndexKey {
    match index {
        IndexKey::String(text) => IndexKey::String(Arc::clone(text)),
        IndexKey::Number(number) => IndexKey::Number(*number),
        IndexKey::UniqueSymbol(identity) => IndexKey::UniqueSymbol(identity.clone()),
        IndexKey::Computed(node) => IndexKey::Computed(*node),
    }
}

/// Deliver a completed evaluation frame's facts to the scope that consumes
/// it: the parent frame's tracer, or — for the root — the tracers active
/// around the evaluation. A frame's tracer is a delivering scope, so this
/// is the only way its observations reach them.
fn deliver_evaluation_facts<W: verter_session_query::facts::clocks::WorkspaceClocks + Clone>(
    tracers: &mut [crate::fact_signature_helpers::StepwiseFactTracer<'_, W>],
    finalise: &FactReadSetFinalise,
) {
    let (FactReadSetFinalise::Ok(facts) | FactReadSetFinalise::NonCacheable(facts)) = finalise
    else {
        return;
    };
    let _scope = tracers.last_mut().map(|parent| parent.install());
    crate::fact_tracing::tracing::observe_fan_out_canonical(facts);
}

/// Deliver the facts every unfinished frame of a stopped evaluation
/// observed to the tracers active around it: the evaluation stopped, but
/// what it read was still read.
fn deliver_abandoned_evaluation_facts<
    W: verter_session_query::facts::clocks::WorkspaceClocks + Clone,
>(
    tracers: Vec<crate::fact_signature_helpers::StepwiseFactTracer<'_, W>>,
) {
    for tracer in tracers.into_iter().rev() {
        deliver_evaluation_facts::<W>(&mut [], &tracer.finish());
    }
}

fn aborted_evaluation_outcome(
    frames: &[DeferredEvaluationFrame],
    completed_child: Option<&EvaluateDeferredOutcome>,
    reasons: PartialReasonSet,
) -> EvaluateDeferredOutcome {
    let root = frames
        .first()
        .expect("an evaluator trip requires an active root frame")
        .entry_node;
    let mut completeness = ResultCompleteness::partial(reasons);
    let mut cache_suppress = false;
    for frame in frames {
        completeness = completeness.merge(frame.completeness);
        cache_suppress |= frame.cache_suppress;
    }
    if let Some(child) = completed_child {
        completeness = completeness.merge(child.completeness);
        cache_suppress |= child.cache_suppress;
    }
    EvaluateDeferredOutcome {
        node: root,
        availability: crate::semantic_query::FactStatus::unavailable(completeness.reasons()),
        completeness,
        cache_suppress,
    }
}

/// Typed outcome of a structural-fact demand
/// ([`ProjectSemanticDispatch::normalize_node_for_structural_fact_demand`] /
/// [`ProjectSemanticDispatch::peel_node_for_uninstantiated_carrier_fact_demand`]).
///
/// Node-HIDING by construction: `Partial` carries the reasons ONLY — no
/// `SemanticNodeId`. A consumer cannot obtain a classifiable node without
/// matching `Complete` and thereby seeing (and deciding on) the partial arm,
/// so a truncated / faulted resolution can never flow into a confident
/// structural classification (the type-level fail-closed rail).
///
/// `Complete` covers BOTH a terminal structural body and a STABLE residual
/// carrier-stop (an honest `QueryError::Miss` on an unresolved authored name,
/// a stable no-progress fix-point, the peel's deliberate `InstantiationRef`
/// stop): a stable stop is a valid semantic `Unknown`, not operational
/// partiality. `Partial` is reserved for operational truncation — the step
/// fuse, the evaluator recursion ceiling, a cycle, budget exhaustion, an
/// unstable state, a non-`Miss` query fault, missing arena data, or a
/// partial nested read.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralFactDemandOutcome {
    /// Fully resolved to a terminal structural body OR a stable residual
    /// carrier-stop (no fuse/ceiling/fault fired). The exact-only arm.
    Complete(SemanticNodeId),
    /// A producer-established usable approximation, including positional
    /// gaps and typed checker recoveries. An exact proof consumer treats it
    /// as [`Partial`](Self::Partial); a value consumer reads the node through
    /// [`into_usable_node`](StructuralFactDemandOutcome::into_usable_node).
    Recovered {
        /// The resolved node with its bounded producer interpretation.
        node: SemanticNodeId,
        /// The adopted approximation's causes.
        reasons: PartialReasonSet,
    },
    /// Truncated / faulted. Carries the reasons ONLY — no node.
    Partial(PartialReasonSet),
}

/// One structural-fact demand after its resolution loop: the node it
/// settled on and everything its finish reads, held while the composite
/// reduction of that node runs. The connected-demand guard lives as long
/// as the demand does, as it would across a native call.
struct SettledDemand<'g> {
    _connected_guard: super::connected_demand::ConnectedDemandGuard<'g>,
    context: ProjectionReductionContext,
    resolve_declaration_refs: bool,
    completeness: ResultCompleteness,
    availability: crate::semantic_query::FactStatus,
    n: SemanticNodeId,
    exit_reasons: Option<PartialReasonSet>,
    named_alias_application: Option<(SemanticNodeId, Option<Vec<SemanticNodeId>>)>,
    /// The dispatcher's operation-budget epoch when the demand began.
    budget_epoch: u64,
}

/// A demand's first half: finished outright (a trip on entry), or settled.
enum BegunDemand<'g> {
    Finished(StructuralFactDemandOutcome),
    Settled(SettledDemand<'g>),
}

/// A suspended frame of `ProjectSemanticDispatch::drive_structural_fact_demands`.
enum DemandFrame<'g> {
    /// A settled demand; `composite` is its reduction once started.
    Demand {
        settled: SettledDemand<'g>,
        composite: Option<CompositeReduction>,
        /// The name a reduction asked this demand for; `None` for the root.
        name: Option<SemanticNodeId>,
        /// Whether the demand met a composite already being reduced
        /// further down (a re-entry left it as written): its outcome
        /// depends on where it ran, so it is not reused.
        reentered: bool,
    },
    /// A composite reduction asked for on its own (only ever the root).
    Composite(CompositeReduction),
}

/// What the driver's root frame finished with.
enum DrivenRoot {
    Demand(StructuralFactDemandOutcome),
    Composite(Option<SemanticNodeId>),
}

/// The composite reduction of one union or intersection, reading its arms
/// in order; suspended while a named arm's demand runs.
struct CompositeReduction {
    node: SemanticNodeId,
    arms: Arc<[SemanticNodeId]>,
    is_union: bool,
    next: usize,
    views: Vec<SemanticNodeId>,
    named_unions: Vec<(SemanticNodeId, Vec<SemanticNodeId>)>,
    /// The named arms of an intersection that stand for unions of object
    /// types: the arm stays the view, and the checker's cross product over
    /// the unions is still weighed
    /// ([`ProjectSemanticDispatch::named_intersection_too_complex`]).
    named_object_unions: Vec<SemanticNodeId>,
    /// Whether the result keeps the names the checker prints as a union's
    /// origin (`U0 | 2`): only at the altitude a type is printed at. A
    /// demand that resolves declarations reads the type set itself (`0 |
    /// 1 | 2`), which is what every member a consumer reads must be.
    keep_origin: bool,
    waiting: Option<ArmWait>,
    /// The typed completeness of the demands the reduction read: a name
    /// whose demand did not complete leaves the composite as written, and
    /// its partial reasons with it.
    completeness: ResultCompleteness,
}

/// What a suspended composite reduction waits for.
enum ArmWait {
    /// The demand of this named arm.
    Resolved(SemanticNodeId),
    /// The demand of a name inside the resolved union the named arm `arm`
    /// stands for.
    Members {
        arm: SemanticNodeId,
        walk: MembersWalk,
    },
}

/// What a composite reduction asks for next.
enum CompositeStep {
    /// The demand of this name, at structural transit.
    Demand(SemanticNodeId),
    /// The reduced node, or `None` when the composite stays as written,
    /// and the completeness of the demands it read.
    Done(Option<SemanticNodeId>, ResultCompleteness),
}

/// The walk over the members of the resolved union a named arm stands for,
/// suspended while the demand of a name inside it runs.
struct MembersWalk {
    members: Vec<SemanticNodeId>,
    /// The names on the walk's active path, and those fully read.
    active: rustc_hash::FxHashSet<SemanticNodeId>,
    read: rustc_hash::FxHashSet<SemanticNodeId>,
    pending: Vec<MemberStep>,
    awaiting: Option<SemanticNodeId>,
    /// The partial reasons of a name whose demand did not complete.
    partial: Option<PartialReasonSet>,
}

enum MemberStep {
    Read(SemanticNodeId),
    Leave(SemanticNodeId),
}

/// What a members walk asks for next.
enum MembersStep {
    Demand(SemanticNodeId),
    /// The members, or `None` when a name did not complete or the walk
    /// reached a name on its own active path.
    Done(Option<Vec<SemanticNodeId>>),
}

impl CompositeReduction {
    /// Merge the partial reasons of a finished members walk.
    fn take_walk_partial(&mut self, walk: &MembersWalk) {
        if let Some(reasons) = walk.partial {
            self.completeness = self
                .completeness
                .merge(ResultCompleteness::partial(reasons));
        }
    }
}

impl MembersWalk {
    /// The walk over `union`, the resolved union the named arm `name`
    /// stands for, with `name` on its active path.
    fn new(name: SemanticNodeId, union: SemanticNodeId) -> Self {
        let mut active = rustc_hash::FxHashSet::default();
        active.insert(name);
        Self {
            members: Vec::new(),
            active,
            read: rustc_hash::FxHashSet::default(),
            pending: vec![MemberStep::Leave(name), MemberStep::Read(union)],
            awaiting: None,
            partial: None,
        }
    }
}

impl StructuralFactDemandOutcome {
    /// Fail-closed projection: the resolved node when `Complete`, `None`
    /// when `Partial`. The standard consumer disposition — a partial demand
    /// yields a refusal / conservative fallback, never a classification.
    pub fn into_complete_node(self) -> Option<SemanticNodeId> {
        match self {
            Self::Complete(node) => Some(node),
            Self::Recovered { .. } | Self::Partial(_) => None,
        }
    }

    /// The node the checker would continue with: the resolved node when
    /// `Complete`, and when `Recovered` the node holding the checker's
    /// recovery, which reads and relates as the checker's does. `None` when
    /// `Partial`. The result it feeds stays partial whenever it is
    /// `Recovered`: the demand already folded its reasons.
    pub fn into_usable_node(self) -> Option<SemanticNodeId> {
        match self {
            Self::Complete(node) | Self::Recovered { node, .. } => Some(node),
            Self::Partial(_) => None,
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// A mapped object's values belong to the consuming index. A `typeof`
    /// operand shares the keys-only transit context used by `keyof`, so the
    /// two operands of `T[keyof T]` read one source surface.
    pub(crate) fn indexed_access_operand_context(
        &self,
        object: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> ProjectionReductionContext {
        if matches!(
            self.graph().node_data(object).as_deref(),
            Some(SemanticNodeData::Mapped { .. } | SemanticNodeData::TypeOf(_))
        ) {
            context.into_structural_transit_with_mode(ProjectionMode::Navigate)
        } else {
            super::raise::indexed_access_object_context(context)
        }
    }

    pub(super) fn normalized_index_key_node(&self, node: SemanticNodeId) -> IndexKey {
        self.normalized_index_key_node_outcome(node).0
    }

    /// Outcome variant of [`Self::normalized_index_key_node`] threading the
    /// entry-scoped completeness AND `cache_suppress` of the index-node
    /// evaluation. Resolving the index expression is a nested deferred call, so
    /// a budget-/recursion-truncated index resolution makes the enclosing
    /// `IndexedAccess` reduction partial and a non-cacheable index read makes
    /// it suppress — both merge up to the caller's
    /// [`Self::evaluate_deferred_outcome`] admission gate.
    fn normalized_index_key_node_outcome(
        &self,
        node: SemanticNodeId,
    ) -> (IndexKey, ResultCompleteness, bool) {
        let outcome = self.evaluate_deferred_outcome(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        );
        let completeness = outcome.completeness;
        let cache_suppress = outcome.cache_suppress;
        let resolved = outcome.node;
        let key = self.normalized_index_key_from_evaluated_node(resolved);
        (key, completeness, cache_suppress)
    }

    /// Convert an already-evaluated index node to its canonical key. The
    /// deferred evaluator normally removes aliases; a residual alias can only
    /// be a stable cycle/carrier stop, so it remains a `TypeNode` instead of
    /// recursively re-entering the evaluator.
    fn normalized_index_key_from_evaluated_node(&self, resolved: SemanticNodeId) -> IndexKey {
        match self.graph().node_data(resolved).as_deref() {
            Some(data @ SemanticNodeData::TypeOfNominal(_)) => IndexKey::UniqueSymbol(
                data.typeof_nominal_identity()
                    .expect("nominal typeof has its declaration identity")
                    .clone(),
            ),
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                IndexKey::String(Arc::from(text.as_str()))
            }
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                // Bounded integer-convention fold: `IndexKey::Number`
                // admits ONLY literals whose i64 `Display` IS the
                // canonical `js_number_to_string` spelling (the single
                // shared producer predicate —
                // `build::integer_convention_index_key`). Everything
                // else stays `TypeNode` for the walker's G4.5
                // canonical-needle recovery.
                match super::build::integer_convention_index_key(*number) {
                    Some(integer) => IndexKey::Number(integer),
                    None => IndexKey::Computed(resolved),
                }
            }
            _ => IndexKey::Computed(resolved),
        }
    }

    pub(super) fn evaluate_deferred_semantic_node(&self, node: SemanticNodeId) -> SemanticNodeId {
        // Default to a `Published + Expanded` context. Publication
        // callers (the bounded reducer, mapper value substitution,
        // conditional check evaluation, builtin-utility argument
        // resolution) all need operator dispatches to terminate at
        // their fully-reduced surface. The demand-driven reducer retires the
        // *implicit* Expanded unwrap by exposing
        // [`Self::evaluate_deferred_semantic_node_with_context`] so
        // structural-transit callers (relation engine identity-
        // carrier unwrap and object-vs-record arms) can opt out of
        // publication reduction explicitly.
        //
        // BUILD-SCOPED sugar: this no-context form exists only for cold-build
        // callers, so it routes through the SAME build-scoped projection as
        // every other bare-node escape — the completeness folds into the
        // active taint frame before the node is released, never dropped.
        self.evaluate_deferred_semantic_node_with_context(
            node,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_active_query_build_node(self)
    }

    /// Context-explicit variant of
    /// [`Self::evaluate_deferred_semantic_node`]
    /// (demand-driven reducer). The caller supplies the
    /// [`ProjectionReductionContext`] that flows into every operator
    /// re-dispatch (`KeyOf`, `MappedType`, decl-placeholder
    /// `Instantiate`) so a `StructuralTransit` walk does not reify
    /// per-member edges along its evaluation path.
    ///
    /// Returns the typed [`EvaluateDeferredOutcome`] — node PLUS
    /// completeness. A build-scoped caller that needs the bare node calls
    /// [`EvaluateDeferredOutcome::into_active_query_build_node`], which
    /// folds the completeness into the active taint frame first; there is
    /// no bare-node form that discards the completeness.
    pub(super) fn evaluate_deferred_semantic_node_with_context(
        &self,
        node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        self.evaluate_deferred_outcome(node, reduction_context)
    }

    /// Test observation window for the deferred evaluator: the typed
    /// outcome exposed as a `(node, completeness)` pair. Integration tests
    /// (and the in-crate sibling test modules, which cannot read the
    /// outcome's private fields) reach the evaluator through this shim —
    /// it exposes the completeness alongside the node, never a restored
    /// bare-node API, and performs NO propagation folds (tests run with no
    /// build frame installed).
    ///
    /// STRICTLY test-scoped: gated `#[cfg(any(test, feature = "test-support"))]`,
    /// NOT `debug_assertions`. A `(node, completeness)` pair is a `.0` bare-node
    /// escape that must not exist in an ordinary debug build (e.g. the debug
    /// LSP / `pnpm dev-extension`): `test-support` is off in `default`, yet the
    /// `[dev-dependencies]` self-edge turns it on for `verter_session`'s own
    /// test / integration targets, so this shim compiles for genuine test code
    /// in BOTH the unit (`cfg(test)`) and the integration build and is
    /// COMPILE-ABSENT in every production profile.
    #[cfg(any(test, feature = "test-support"))]
    pub fn evaluate_deferred_semantic_node_with_context_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness) {
        let (node, completeness, _cache_suppress) =
            self.evaluate_deferred_outcome_for_tests(node, context);
        (node, completeness)
    }

    /// `cache_suppress`-exposing sibling of
    /// [`Self::evaluate_deferred_semantic_node_with_context_for_tests`]: the
    /// typed outcome as a `(node, completeness, cache_suppress)` triple.
    ///
    /// The third field is the OR of every nested read's `cache_suppress`
    /// observed while producing this outcome (a fenced / torn-self-root /
    /// `ReturnOnly` benign-non-cacheability). It is the
    /// admission signal the `evaluate_deferred_memo` publish gate consults
    /// alongside completeness, and it is NOT reconstructible from the node —
    /// tests that assert the suppress-aggregation contract (e.g. a
    /// carrier-subject arm threading a nested read's `cache_suppress`) reach it
    /// ONLY through this shim. Same strict `#[cfg(any(test, feature =
    /// "test-support"))]` gate + no propagation folds as the 2-tuple form.
    #[cfg(any(test, feature = "test-support"))]
    pub fn evaluate_deferred_outcome_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness, bool) {
        let outcome = self.evaluate_deferred_outcome(node, context);
        (outcome.node, outcome.completeness, outcome.cache_suppress)
    }

    /// Demand-point structural-fact normalizer for node-domain fact readers
    /// (e.g. [`CallableNodeView`](crate::project_semantic_dispatch::callable_view::CallableNodeView)).
    ///
    /// Resolves a node to its concrete STRUCTURAL BODY at a GENUINE fact demand:
    /// first evaluate deferred shells
    /// ([`Self::evaluate_deferred_semantic_node_with_context`] — which unwraps
    /// `Alias` / `KeyOf` / `IndexedAccess` / `Mapped` / `Conditional` /
    /// `TemplateLiteral` / decl-placeholder / bare-import carriers), then resolve
    /// a RESIDUAL `DeclRef` via the shared `ResolveDecl` query and a residual
    /// `InstantiationRef` via the shared `Instantiate` query — the two carriers
    /// the deferred-shell evaluator deliberately leaves carrier-shaped so an
    /// intermediate indexed-access hop stays symbolic (see the `_ => break node`
    /// arm of [`Self::evaluate_deferred_outcome`] and the matching `relation.rs`
    /// demand note) — then RE-EVALUATE the materialised body. The loop is bounded
    /// by exact-identity cycle detection plus the connected work envelope and is FAIL-CLOSED:
    /// on a cycle, no progress, depth exhaustion, or a `Recursive`/`Error` query
    /// result it returns the current node unchanged (which may still be a
    /// carrier — the caller fails closed, never fabricating a fact).
    ///
    /// This GENERALIZES the relation-oracle demand-resolve pattern (the
    /// `InstantiationRef` materialisation in `relation::record_target_shape`) to
    /// BOTH residual carriers. It is NOT a second resolver: every resolution step
    /// delegates to the existing shared `ResolveDecl` / `Instantiate` queries
    /// (the callable realizer [`realize_callable_member`](crate::project_semantic_dispatch::callable_view::realize_callable_member)
    /// classifies over exactly this primitive), records their dep-signature facts into the active tracer, and
    /// folds their partial / suppress signals — so a node-domain reader's
    /// cache-validity signature observes exactly the facts the resolution
    /// depended on. It NEVER lowers through `TypeExpr` and NEVER walks structure
    /// beyond recognising these carrier shells (shallow-by-default: a child node
    /// is normalised only when a reader reaches its OWN concrete fact demand —
    /// this primitive does not enumerate object surfaces, walk members, or expand
    /// keyspaces).
    ///
    /// Returns the typed [`StructuralFactDemandOutcome`]: `Complete(node)` on a
    /// terminal structural body or a STABLE carrier-stop (an honest miss, a
    /// no-progress fix-point); `Partial(reasons)` — with NO node — when the
    /// resolution was operationally truncated or faulted (connected work or
    /// query-depth exhaustion, cycle, unstable state, non-`Miss` fault, missing
    /// arena data, or a partial nested read). A consumer classifies ONLY a
    /// `Complete` node.
    ///
    /// MUST NOT be used by carrier-PRESERVING readers (e.g.
    /// `first_param_object_surface`): resolving a `DeclRef` subject there would
    /// break the symbolic indexed-access preservation policy (`AppProps['avatar']`).
    /// The semantic demand identity is the caller's `context`; the primitive only
    /// uses the helper contexts `ResolveDecl` / `Instantiate` themselves require.
    pub fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> StructuralFactDemandOutcome {
        // Full structural-fact demand: resolve BOTH residual carriers
        // (`DeclRef` via `ResolveDecl`, `InstantiationRef` via `Instantiate`).
        self.resolve_structural_fact_demand(node, context, true, true)
    }

    /// Carrier-PRESERVING sibling of
    /// [`Self::normalize_node_for_structural_fact_demand`] for readers that must
    /// reach an UNINSTANTIATED `InstantiationRef` carrier (e.g. the validated
    /// Svelte-snippet positional reader
    /// [`CallableNodeView::validated_snippet_positional_params`](crate::project_semantic_dispatch::callable_view::CallableNodeView),
    /// and the Vue slot-binding `Pick` source-root read).
    ///
    /// It is [`Self::normalize_node_for_structural_fact_demand`] MINUS the
    /// `InstantiationRef`-instantiate arm: it evaluates deferred shells and
    /// resolves a residual `DeclRef` through the shared `ResolveDecl` query (the
    /// SAME `ScopeId { canonical_id, local_scope: None }` shape), re-evaluating
    /// each hop, BUT it STOPS at an `InstantiationRef` root and NEVER calls
    /// `Instantiate` on it — it returns the un-instantiated `InstantiationRef`
    /// node so the caller can read its `args` (a positional generic-contract
    /// read legitimate ONLY under a validated boundary such as the Svelte
    /// `Snippet<Params>` contract or the Vue `Pick<Root, K>` DTO policy). The
    /// `Instantiate`-first demand primitive would CONSUME the `args` tuple
    /// (`Snippet<[T]>` → the Snippet interface `Object`, losing the carrier
    /// args), so a carrier-reading reader must peel through THIS primitive first.
    ///
    /// Bounded (exact-identity cycle detection + connected work envelope) and fail-closed:
    /// returns the typed [`StructuralFactDemandOutcome`] — the peel's
    /// deliberate `InstantiationRef` stop and an honest miss / stable
    /// no-progress are `Complete`, while a cycle, connected operational limit,
    /// fault, or partial nested read is `Partial(reasons)` with NO node. It is
    /// NOT a second resolver: the `DeclRef` step delegates to the shared
    /// `ResolveDecl` query and records the same dep-signature / suppress facts as
    /// the demand primitive it derives from.
    pub fn peel_node_for_uninstantiated_carrier_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> StructuralFactDemandOutcome {
        // Carrier-preserving peel: resolve `DeclRef` shells but STOP at an
        // `InstantiationRef` (do NOT instantiate — leave the args readable).
        self.resolve_structural_fact_demand(node, context, false, true)
    }

    /// Declaration-KEEPING sibling of
    /// [`Self::normalize_node_for_structural_fact_demand`]: resolves
    /// residual carriers through the shared `ResolveDecl` / `Instantiate`
    /// queries exactly as it does, but STOPS where the checker prints a
    /// NAME instead of resolving it to the declaration's body.
    ///
    /// That is the altitude a type is PRINTED at: a utility or conditional
    /// application (`InstanceType<…>`, `ReturnType<…>`, `Awaited<…>`) is
    /// reduced, while a named interface is shown by its name, and so is an
    /// alias application the checker names by its alias — each with its
    /// omitted defaulted arguments filled (see
    /// `ProjectSemanticDispatch::printed_declaration`). The checker's
    /// printed answer to `InstanceType<typeof CtorA & typeof CtorB>` is `B`,
    /// the interface — not `{ b: 2 }`, its body — and to a `WithDefault`
    /// reference it is `WithDefault<string>`, so an evidence lane that
    /// compares a live answer against a recorded print needs this stop, and
    /// the fully resolving demand overshoots it by exactly one step.
    ///
    /// Same loop, same resolver, same bounds and typed `Partial` outcome as
    /// its siblings; STRICTLY test-scoped because its only consumer is the
    /// in-crate signature-corpus driver.
    #[cfg(any(test, feature = "test-support"))]
    pub fn normalize_node_keeping_declaration_refs_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> StructuralFactDemandOutcome {
        self.resolve_structural_fact_demand(node, context, true, false)
    }

    /// The named declaration an indexed access reads, when its terminal is
    /// one: the object evaluated, the member read at navigate altitude, and
    /// the read kept only when it is a declaration or class reference.
    /// `None` for any other access (a computed index, a partial read, a
    /// terminal that is not a named reference).
    fn named_indexed_access(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<EvaluateDeferredOutcome> {
        self.probe_optional(|| self.probe_named_indexed_access(node, context))
    }

    fn probe_named_indexed_access(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<EvaluateDeferredOutcome> {
        let (object, index) = match self.graph().node_data(node)?.as_ref() {
            SemanticNodeData::IndexedAccess { object, index }
                if !matches!(index, IndexKey::Computed(_)) =>
            {
                (*object, clone_index_key(index))
            }
            _ => return None,
        };
        let base =
            self.evaluate_deferred_outcome(object, context.with_mode(ProjectionMode::Navigate));
        if !matches!(base.completeness, ResultCompleteness::Complete) {
            return None;
        }
        let read = self.execute_read(SemanticQueryKey::IndexedAccess {
            base: base.node,
            index,
            mode: ProjectionMode::Navigate,
        });
        if read.result_is_partial() {
            return None;
        }
        let QueryResult::Value(value) = read.value else {
            return None;
        };
        self.is_named_reference(value)
            .then(|| EvaluateDeferredOutcome::complete(value))
    }

    /// Whether `node` is a type the checker prints by name: a declaration
    /// or class reference, or a union or intersection of them.
    fn is_named_reference(&self, node: SemanticNodeId) -> bool {
        match self.graph().node_data(node).as_deref() {
            Some(
                SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. }
                | SemanticNodeData::ClassExpressionInstance { .. },
            ) => true,
            Some(SemanticNodeData::Union(arms)) => {
                arms.iter().any(|arm| self.is_named_reference(*arm))
                    && arms.iter().all(|arm| {
                        self.is_named_reference(*arm)
                            || matches!(
                                self.graph().node_data(*arm).as_deref(),
                                Some(SemanticNodeData::Primitive(
                                    PrimitiveKind::Null | PrimitiveKind::Undefined
                                ))
                            )
                    })
            }
            Some(SemanticNodeData::Intersection(arms)) => {
                arms.iter().all(|arm| self.is_named_reference(*arm))
            }
            _ => false,
        }
    }

    /// Whether reading `index` off the evaluated `object` at navigate
    /// altitude ends at a named declaration or class reference.
    pub(super) fn indexed_access_reads_named_declaration(
        &self,
        object: SemanticNodeId,
        index: &IndexKey,
    ) -> bool {
        self.probe_optional(|| {
            let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                base: object,
                index: clone_index_key(index),
                mode: ProjectionMode::Navigate,
            });
            if read.result_is_partial() {
                return None;
            }
            let QueryResult::Value(value) = read.value else {
                return None;
            };
            self.is_named_reference(value).then_some(true)
        })
        .unwrap_or(false)
    }

    /// Shared residual-carrier resolution loop backing
    /// [`Self::normalize_node_for_structural_fact_demand`] (both residual arms
    /// resolve), [`Self::peel_node_for_uninstantiated_carrier_fact_demand`]
    /// (`instantiate_instantiation_refs = false`) and, under test,
    /// `normalize_node_keeping_declaration_refs_for_tests`
    /// (`resolve_declaration_refs = false`). ONE loop, one resolver — the
    /// entry points differ ONLY in which residual arm is allowed to resolve
    /// (there is no divergent second implementation).
    ///
    /// A composite the demand settles on reads its named arms' own demands
    /// (see [`Self::composite_over_resolved_arms`]), and those arms' demands
    /// read theirs in turn: that nesting runs from an explicit stack of
    /// suspended demands ([`Self::drive_structural_fact_demands`]), so a
    /// finite chain of names (`type U1 = U0 | 2; type U2 = U1 | 3; …`)
    /// spends no native stack per link.
    fn resolve_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        instantiate_instantiation_refs: bool,
        resolve_declaration_refs: bool,
    ) -> StructuralFactDemandOutcome {
        match self.begin_structural_fact_demand(
            node,
            context,
            instantiate_instantiation_refs,
            resolve_declaration_refs,
        ) {
            BegunDemand::Finished(outcome) => outcome,
            BegunDemand::Settled(settled) => {
                match self.drive_structural_fact_demands(DemandFrame::Demand {
                    settled,
                    composite: None,
                    name: None,
                    reentered: false,
                }) {
                    DrivenRoot::Demand(outcome) => outcome,
                    DrivenRoot::Composite(_) => {
                        unreachable!("a demand root finishes as a demand")
                    }
                }
            }
        }
    }

    /// The first half of one structural-fact demand: enter the connected
    /// demand, evaluate deferred shells and resolve residual carriers until
    /// the chain settles. What remains — the composite reduction of the
    /// settled node, then [`Self::finish_structural_fact_demand`] — is run
    /// by [`Self::drive_structural_fact_demands`].
    fn begin_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        instantiate_instantiation_refs: bool,
        resolve_declaration_refs: bool,
    ) -> BegunDemand<'_> {
        if let Some(abort) = crate::request_context::current_execution_abort() {
            return BegunDemand::Finished(StructuralFactDemandOutcome::Partial(
                abort.reason_class(),
            ));
        }
        let (connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            self.fold_local_partial_completeness(reasons);
            return BegunDemand::Finished(StructuralFactDemandOutcome::Partial(reasons));
        }
        let budget_epoch = self.operation_budget_epoch.get();
        // Step 1: evaluate deferred shells (Alias / KeyOf / IndexedAccess /
        // Mapped / Conditional / TemplateLiteral / DeclPlaceholder / bare-import),
        // merging the evaluation's typed completeness into the demand outcome.
        // The declaration-keeping mode prints an indexed access that reads
        // a named declaration by that name, as the checker does (`W['d']`
        // over `d: Decl` is `Decl`, a class constructor's `prototype` its
        // class): the read stops at the reference its terminal holds.
        let first = match (!resolve_declaration_refs)
            .then(|| self.named_indexed_access(node, context))
            .flatten()
        {
            Some(named) => named,
            None => self.evaluate_deferred_outcome(node, context),
        };
        let mut completeness = first.completeness;
        let mut availability = first.availability;
        let mut n = first.node;
        // Step 2: resolve residual DeclRef / InstantiationRef carriers the
        // deferred evaluator deliberately leaves shaped, then re-evaluate the
        // materialised body. Bounded, and every exit is TYPED: a stable stop
        // (terminal body / honest miss / no-progress / the peel's deliberate
        // `InstantiationRef` stop) contributes `Complete`; an operational
        // truncation or fault contributes the matching `PartialReasonSet` bit.
        //
        // Each residual-carrier `execute_read` below pairs
        // `observe_component_meta_read_suppress` + `emit_dispatch_dep_signature_facts`,
        // exactly as the canonical resolver does at its own `ResolveDecl` /
        // `Instantiate` sites — so a partial / suppressed sub-resolution taints
        // the caller's request / cold-compute warm gate identically. On TOP of
        // that request-scoped propagation, the read's `result_is_partial` bool
        // folds into THIS demand's typed outcome (the boolean bridge lifts as
        // `PROPAGATED`), so the completeness survives even with NO
        // `RequestContext` installed.
        let mut visited = rustc_hash::FxHashSet::default();
        // The declaration-keeping mode's OUTERMOST alias application the
        // checker names, recorded as the chain resolves through it and
        // printed when the chain settles on a type that alias names.
        // An intersection-bodied alias also keeps its application's
        // arguments: an argument the construction returns as it is was not
        // constructed by the alias.
        let mut named_alias_application: Option<(SemanticNodeId, Option<Vec<SemanticNodeId>>)> =
            None;
        // The loop's own exit classification: `None` = a stable (Complete)
        // stop; `Some(reasons)` = an operational truncation/fault.
        let exit_reasons: Option<PartialReasonSet> = loop {
            let Some(data) = self.graph().node_data(n) else {
                // Missing arena data: the demand cannot classify what it
                // cannot read.
                break Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA);
            };
            // The declaration-keeping mode prints a named declaration the
            // way the checker does: an interface or class by its name, an
            // alias application by its alias when the alias names the type
            // it resolves to — each with its omitted defaulted arguments
            // filled — and every other alias by what it resolves to.
            let printed = if resolve_declaration_refs {
                None
            } else {
                match data.as_ref() {
                    SemanticNodeData::DeclRef { identity } => Some((identity.clone(), None)),
                    SemanticNodeData::InstantiationRef { base, args } => {
                        Some((base.clone(), Some(Arc::clone(args.as_arc()))))
                    }
                    _ => None,
                }
            };
            let printed = printed.map(|(identity, args)| {
                let kind = if is_builtin(&identity) {
                    self.printed_builtin_application(
                        &identity,
                        args.as_deref().unwrap_or_default(),
                        context,
                    )
                } else {
                    self.printed_declaration(&identity)
                };
                (identity, args, kind)
            });
            // TERMINAL-BEFORE-FUSE: classify whether `n` is a residual
            // resolvable carrier BEFORE consulting the fuse, so a result that
            // reached its terminal structural body on exactly the last
            // permitted step is a stable stop, never a false partial. The
            // non-residual break also covers the peel's deliberate
            // un-instantiated `InstantiationRef` stop, and the
            // declaration-keeping mode's stop at a declaration printed by
            // its name or at a class-expression instance.
            let is_residual = match (data.as_ref(), &printed) {
                (_, Some((_, _, Some(PrintedDeclaration::Named)))) => false,
                (_, Some((_, _, Some(_)))) => true,
                (
                    SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::ClassExpressionInstance { .. },
                    _,
                ) => resolve_declaration_refs,
                (SemanticNodeData::InstantiationRef { .. }, _) => instantiate_instantiation_refs,
                _ => false,
            };
            drop(data);
            if let Some((identity, args, Some(kind))) = &printed {
                let args = args.as_deref().unwrap_or_default();
                match kind {
                    // A builtin application is its own printed name.
                    PrintedDeclaration::Named if is_builtin(identity) => {}
                    PrintedDeclaration::Named => {
                        n = self.declared_application(n, identity, args, context);
                    }
                    PrintedDeclaration::AliasNamed | PrintedDeclaration::AliasNamedIntersection
                        if named_alias_application.is_none() =>
                    {
                        let intersection_arguments =
                            (*kind == PrintedDeclaration::AliasNamedIntersection).then(|| {
                                args.iter()
                                    .flat_map(|arg| {
                                        [*arg, self.evaluate_deferred_outcome(*arg, context).node]
                                    })
                                    .collect()
                            });
                        named_alias_application = Some((
                            self.declared_application(n, identity, args, context),
                            intersection_arguments,
                        ));
                    }
                    // The application the alias's body writes, substituted
                    // and still unreduced — classified in turn.
                    PrintedDeclaration::AliasThrough => {
                        let read = self.execute_read(SemanticQueryKey::Instantiate(
                            crate::semantic_query::InstantiateKey::new(
                                self.type_slot_for(
                                    Arc::clone(&identity.canonical_id),
                                    identity.owner,
                                    Arc::clone(&identity.decl_name),
                                ),
                                Arc::from(args.to_vec().into_boxed_slice()),
                                self.instantiate_context_for(
                                    &identity.canonical_id,
                                    ProjectionReductionContext::structural_transit_with_mode(
                                        ProjectionMode::Navigate,
                                    ),
                                ),
                            ),
                        ));
                        crate::request_context::observe_component_meta_read_suppress(&read);
                        crate::meta_resolve::emit_dispatch_dep_signature_facts(
                            self,
                            &read.dep_signature,
                        );
                        completeness = completeness
                            .or_partial_if(read.result_is_partial(), read.partial_reason_classes());
                        if let QueryResult::Value(target) = read.value {
                            if target != n {
                                if !visited.insert(n) {
                                    break Some(PartialReasonSet::SAME_PATH_RECURSION);
                                }
                                if let Err(reasons) = self.charge_connected_work() {
                                    break Some(reasons);
                                }
                                n = target;
                                continue;
                            }
                        }
                    }
                    PrintedDeclaration::AliasNamed
                    | PrintedDeclaration::AliasNamedIntersection
                    | PrintedDeclaration::AliasTransparent => {}
                }
            }
            if !is_residual {
                break None;
            }
            let Some(data) = self.graph().node_data(n) else {
                break Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA);
            };
            if !visited.insert(n) {
                // Residual-carrier cycle (`type MutA = MutB; type MutB = MutA`):
                // the chain can never settle.
                break Some(PartialReasonSet::SAME_PATH_RECURSION);
            }
            if let Err(reasons) = self.charge_connected_work() {
                // Identity continued to change until the connected demand's
                // total work envelope was exhausted. Exact cycles are checked
                // first above and retain their distinct sentinel semantics.
                break Some(reasons);
            }
            let resolved = match data.as_ref() {
                // A class expression's instance resolves to the instance surface
                // it carries — the body a `DeclRef` resolves to, already in hand.
                SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                    let surface = *surface;
                    drop(data);
                    self.class_expression_read_surface(n).unwrap_or(surface)
                }
                // Residual DeclRef → the canonical shallow `ResolveDecl` query
                // (the `ScopeId { canonical_id, local_scope: None }` shape the
                // canonical resolver issues).
                SemanticNodeData::DeclRef { identity } => {
                    let identity = identity.clone();
                    drop(data);
                    let probe = self.observe_read(|| {
                        self.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                            scope: ScopeId {
                                canonical_id: Arc::clone(&identity.canonical_id),
                                owner: identity.owner,
                                local_scope: None,
                                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                                    identity.owner,
                                ),
                            },
                            name: Arc::clone(&identity.decl_name),
                        }))
                    });
                    // A negative lookup does not replace this authored carrier.
                    // Its unavailable value is declined; operationally
                    // incomplete misses have a distinct typed error.
                    if matches!(
                        &probe.value().value,
                        QueryResult::Error(QueryError::Miss | QueryError::RaiseMiss)
                    ) {
                        break match probe.discard() {
                            Ok(_) => None,
                            Err(abort) => Some(abort.reason_class()),
                        };
                    }
                    let read = match probe.adopt(self) {
                        Ok(read) => read,
                        Err(abort) => break Some(abort.reason_class()),
                    };
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self,
                        &read.dep_signature,
                    );
                    completeness = completeness
                        .or_partial_if(read.result_is_partial(), read.partial_reason_classes());
                    availability = availability.join(read.fact_status);
                    match read.value {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(_) => {
                            break Some(PartialReasonSet::SAME_PATH_RECURSION)
                        }
                        QueryResult::Error(err) => break demand_read_fault_reasons(&err),
                    }
                }
                // Residual InstantiationRef → the shared `Instantiate` query
                // with the authored argument carriers intact. The body owns
                // argument demand: evaluating all arguments here would force
                // unused mapped values and losing conditional branches. The
                // slot is the base decl's type slot, and the instantiate
                // context derives from the caller's context.
                //
                // The carrier-preserving peel (`instantiate_instantiation_refs
                // == false`) never reaches this arm — `is_residual` classified
                // the un-instantiated `InstantiationRef` as its deliberate
                // stable stop above.
                SemanticNodeData::InstantiationRef { base, args } => {
                    let slot = self.type_slot_for(
                        Arc::clone(&base.canonical_id),
                        base.owner,
                        Arc::clone(&base.decl_name),
                    );
                    let owner_canonical = Arc::clone(&base.canonical_id);
                    let arguments = self.evaluate_carrier_arguments(base, args, context);
                    completeness = completeness.merge(arguments.completeness);
                    availability = availability.join(arguments.availability);
                    // A standalone typed demand has no enclosing memo to
                    // suppress; a build-enclosed demand retains that rail.
                    self.fold_into_top_build_local_taint(false, arguments.cache_suppress);
                    let args = arguments.arguments;
                    drop(data);
                    let probe = self.observe_read(|| {
                        self.execute_read(SemanticQueryKey::Instantiate(
                            crate::semantic_query::InstantiateKey::new(
                                slot,
                                args,
                                self.instantiate_context_for(&owner_canonical, context),
                            ),
                        ))
                    });
                    // A negative lookup does not replace this authored carrier.
                    // Its unavailable value is declined; operationally
                    // incomplete misses have a distinct typed error.
                    if matches!(
                        &probe.value().value,
                        QueryResult::Error(QueryError::Miss | QueryError::RaiseMiss)
                    ) {
                        break match probe.discard() {
                            Ok(_) => None,
                            Err(abort) => Some(abort.reason_class()),
                        };
                    }
                    let read = match probe.adopt(self) {
                        Ok(read) => read,
                        Err(abort) => break Some(abort.reason_class()),
                    };
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self,
                        &read.dep_signature,
                    );
                    completeness = completeness
                        .or_partial_if(read.result_is_partial(), read.partial_reason_classes());
                    availability = availability.join(read.fact_status);
                    match read.value {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(_) => {
                            break Some(PartialReasonSet::SAME_PATH_RECURSION)
                        }
                        QueryResult::Error(err) => break demand_read_fault_reasons(&err),
                    }
                }
                // `is_residual` above already classified every other shape as
                // a stable stop.
                _ => unreachable!("non-residual shapes break before the resolve step"),
            };
            // Re-evaluate the materialised body (it may itself be a deferred
            // shell or chain into a further residual carrier), merging its
            // typed completeness.
            let next = self.evaluate_deferred_outcome(resolved, context);
            completeness = completeness.merge(next.completeness);
            availability = availability.join(next.availability);
            if next.node == n {
                // No progress — a stable fix-point carrier-stop.
                break None;
            }
            n = next.node;
        };
        BegunDemand::Settled(SettledDemand {
            _connected_guard: connected_guard,
            context,
            resolve_declaration_refs,
            completeness,
            availability,
            n,
            exit_reasons,
            named_alias_application,
            budget_epoch,
        })
    }

    /// The second half of one structural-fact demand, once the composite
    /// reduction of the node it settled on is known (`composite`, `None`
    /// when nothing reduced or the chain did not settle): the key-arm
    /// reading, the alias naming, and the typed outcome.
    fn finish_structural_fact_demand(
        &self,
        settled: SettledDemand<'_>,
        composite: Option<SemanticNodeId>,
        composite_completeness: ResultCompleteness,
    ) -> StructuralFactDemandOutcome {
        let SettledDemand {
            _connected_guard,
            context,
            resolve_declaration_refs,
            mut completeness,
            availability,
            mut n,
            exit_reasons,
            named_alias_application,
            budget_epoch,
        } = settled;
        if let Some(reasons) = exit_reasons {
            completeness = completeness.merge(ResultCompleteness::partial(reasons));
        } else {
            // A post-resolution reduction that read an incomplete demand
            // leaves its node as written; its partial reasons are this
            // demand's, so the unresolved node is never answered Complete.
            completeness = completeness.merge(composite_completeness);
            if let Some(reduced) = composite {
                n = reduced;
            }
            let (keys, key_completeness) = self.union_over_evaluated_key_arms(n, context);
            completeness = completeness.merge(key_completeness);
            if let Some(reduced) = keys {
                n = reduced;
            }
        }
        // The alias names the type its application settled on only while
        // that type is one the alias itself constructs; a union that
        // collapsed to one member (`type U<T> = T | string` at `string`) or
        // an intersection reduced to `never` is printed as itself.
        if let Some((named, intersection_arguments)) = named_alias_application {
            let named_by_alias = if let Some(arguments) = intersection_arguments {
                matches!(
                    self.graph().node_data(n).as_deref(),
                    Some(SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))
                ) && !arguments.contains(&n)
            } else {
                self.alias_names_settled_type(n) && !self.mapped_alias_over_array(named, n)
            };
            if named_by_alias {
                n = named;
            }
        }
        // A recovery made anywhere under the demand — a relation, a nested
        // build, a canonical construction — is this demand's too, even
        // where no read carried its reason here.
        if self.operation_budget_epoch.get() != budget_epoch {
            completeness = completeness.merge(ResultCompleteness::partial(
                PartialReasonSet::OPERATION_BUDGET,
            ));
        }
        // The declaration-keeping mode prints an application's arguments as
        // the checker prints them.
        if !resolve_declaration_refs && !completeness.is_partial() {
            n = self.application_over_printed_arguments(n, context);
        }
        match completeness {
            ResultCompleteness::Complete => StructuralFactDemandOutcome::Complete(n),
            ResultCompleteness::Partial(reasons) => {
                // No-poison fold (BEST-EFFORT, NO mandatory-frame assert). A
                // demand that SELF-detected an operational truncation (step
                // fuse / recursion ceiling / residual-carrier cycle / missing
                // arena data) following only `Complete` residual `ResolveDecl`
                // / `Instantiate` reads produces NO `CacheRead` carrying that
                // partial — the universal read-boundary fold
                // (`fold_cache_read_rails`) never fires for it, so the
                // enclosing cold build would otherwise stay `Complete` and
                // WARM-ADMIT the consumer's incomplete fallback. Fold the
                // reasons through the SAME central rail the evaluator-caller
                // escape uses ([`Self::fold_local_partial_completeness`]): the
                // request / cold-compute completeness scope AND the active
                // cold-build / relation taint frame (`result_is_partial = true`
                // + `cache_suppress = true`). A build-ENCLOSED demand thereby
                // refuses warm admission of the incomplete result.
                //
                // BEST-EFFORT — no frame assert here (unlike
                // `into_active_query_build_node`): a structural-fact demand
                // consumer can legitimately run STANDALONE / frameless (e.g.
                // the transitive `svelte_exec` snippet path). A frameless
                // standalone has no warm cache to poison, so the build-local
                // fold soundly no-ops on the empty stack and the request-scope
                // fold no-ops with no `RequestContext` installed. Both
                // demand primitives (`normalize_node_for_structural_fact_demand`
                // + `peel_node_for_uninstantiated_carrier_fact_demand`) route
                // through this one exit, so both fail closed identically.
                self.fold_local_partial_completeness(reasons);
                // Stopped only by an operation's own allowance: the node
                // holds the checker's recovery, usable but incomplete.
                if (matches!(
                    availability,
                    crate::semantic_query::FactStatus::Approximate(_, _)
                ) && availability.completeness().reasons() == reasons)
                    || matches!(
                        self.graph().node_data(n).as_deref(),
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
                    )
                {
                    StructuralFactDemandOutcome::Recovered { node: n, reasons }
                } else {
                    StructuralFactDemandOutcome::Partial(reasons)
                }
            }
        }
    }

    /// How the checker prints an application of the named declaration
    /// `identity` — measured on the pinned TypeScript 7.0.2 through the
    /// corpus's two-step wrapper:
    ///
    /// - an interface or class application is printed by its name
    ///   (`GI` with `interface GI<T = number>` prints `GI<number>`);
    /// - an alias whose declared body is a type the alias itself
    ///   constructs — an object, function, mapped, array or tuple type, a
    ///   union or an intersection — is printed by the alias (`Tup<number>`,
    ///   `Fn<number>`), except a tuple with a variadic element, which is
    ///   normalized on instantiation and printed as the tuple (`type
    ///   Push<T extends unknown[], U> = [...T, U]` prints `Push<[1, 2], 3>`
    ///   as `[1, 2, 3]`);
    /// - an alias whose body references another declaration prints as
    ///   [`Self::printed_alias_reference`] decides;
    /// - every other alias is not named by the alias: a conditional
    ///   (`Cond` prints its selected branch), a bare parameter
    ///   (`type Lit<T> = T`) or a primitive.
    ///
    /// `None` when the declaration's kind or body cannot be recovered.
    ///
    /// An alias chain is walked in a loop, one alias per step, so a chain
    /// of any length is read in constant native stack.
    fn printed_declaration(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> Option<PrintedDeclaration> {
        let mut visited = rustc_hash::FxHashSet::default();
        let mut current = identity.clone();
        // Whether the printing is the one of an alias further down the
        // chain, reached through generic alias applications.
        let mut through_alias = false;
        let terminal = loop {
            let (target, args) = match self.printed_declaration_body(&current, &mut visited) {
                DeclarationPrinting::Printed(printed) => break printed,
                DeclarationPrinting::Reference(target, args) => (target, args),
            };
            match self.printed_alias_reference(&target, &args) {
                ReferencePrinting::Printed(printed) => break Some(printed),
                ReferencePrinting::AsTarget => {
                    through_alias = true;
                    current = target;
                }
            }
        };
        if !through_alias {
            return terminal;
        }
        // An application of a generic alias is named by the outer alias
        // unless the target prints as what it resolves to — at every step
        // alike, so every step above the terminal prints as the one below.
        Some(match terminal {
            Some(PrintedDeclaration::AliasTransparent) | None => {
                PrintedDeclaration::AliasTransparent
            }
            Some(_) => PrintedDeclaration::AliasNamed,
        })
    }

    /// One step of [`Self::printed_declaration`]: how `identity` prints,
    /// or the declaration its body references, whose printing decides.
    fn printed_declaration_body(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        visited: &mut rustc_hash::FxHashSet<crate::semantic_query::DeclIdentity>,
    ) -> DeclarationPrinting {
        use verter_session_query::declarations::TypeDeclKind;
        if !visited.insert(identity.clone()) {
            // An alias cycle names nothing.
            return DeclarationPrinting::Printed(Some(PrintedDeclaration::AliasTransparent));
        }
        match self.prepared_decl_kind(identity) {
            None => return DeclarationPrinting::Printed(None),
            Some(TypeDeclKind::Interface | TypeDeclKind::Class) => {
                return DeclarationPrinting::Printed(Some(PrintedDeclaration::Named))
            }
            Some(TypeDeclKind::Alias) => {}
        }
        let body = match self.declared_alias_body(identity) {
            None => return DeclarationPrinting::Printed(None),
            Some(None) => {
                return DeclarationPrinting::Printed(Some(PrintedDeclaration::AliasTransparent))
            }
            Some(Some(body)) => body,
        };
        let graph = self.graph();
        DeclarationPrinting::Printed(Some(match graph.node_data(body).as_deref() {
            // A tuple with a variadic element (`...T` over anything but an
            // array type) is normalized when it is instantiated, and the
            // normalized tuple carries no alias (the checker defers only a
            // tuple type node without one).
            Some(SemanticNodeData::Tuple { elements, .. })
                if elements.iter().any(|element| {
                    element.rest
                        && !matches!(
                            graph.node_data(element.value).as_deref(),
                            Some(SemanticNodeData::Array {
                                readonly: false,
                                ..
                            })
                        )
                }) =>
            {
                PrintedDeclaration::AliasTransparent
            }
            Some(
                SemanticNodeData::Object(_)
                | SemanticNodeData::Signature { .. }
                | SemanticNodeData::Mapped { .. }
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Union(_),
            ) => PrintedDeclaration::AliasNamed,
            Some(SemanticNodeData::Intersection(_)) => PrintedDeclaration::AliasNamedIntersection,
            Some(SemanticNodeData::DeclRef { identity: target }) => {
                return DeclarationPrinting::Reference(target.clone(), Arc::from([]));
            }
            Some(SemanticNodeData::InstantiationRef { base: target, args }) => {
                return DeclarationPrinting::Reference(target.clone(), Arc::clone(args.as_arc()));
            }
            _ => PrintedDeclaration::AliasTransparent,
        }))
    }

    /// How the checker prints an alias whose declared body references
    /// `target` with the declared arguments `args`, measured on TypeScript
    /// 7.0.2:
    ///
    /// - a reference to a NON-generic declaration is that declaration's
    ///   type, printed as it prints (`type ToFace = Face` prints `Face`,
    ///   `type ToObj = Obj` prints `Obj`, `type ToToFace = ToFace` prints
    ///   `Face`);
    /// - a generic interface or class application is named by the alias
    ///   (`type ToGFace = GFace<string>` prints `ToGFace`,
    ///   `type ToGFaceDefault = GFace` prints `ToGFaceDefault`,
    ///   `type PromAlias<T> = Promise<T>` prints `PromAlias<number>`);
    /// - a homomorphic mapped application — `Partial` / `Required` /
    ///   `Readonly`, or an alias declaring one — keeps the MAPPED name
    ///   unless its declared source is a union (`type P<T> = Partial<T>`
    ///   prints `P<{ a: 1 }>` as `Partial<{ a: 1; }>`, `type PP<T> = P<T>`
    ///   too, `type MpA<T> = Mp<T>` over `type Mp<T> = { [K in keyof T]:
    ///   T[K] }` prints `Mp<{ a: 1; }>`, `type PFace = Partial<Face>` prints
    ///   `Partial<Face>`, while `type PU = Partial<Face | Obj>` prints `PU`);
    /// - a keyed mapped application is named by the alias (`type PickA<T> =
    ///   Pick<T, 'a'>` prints `PickA<{ a: 1; b: 2; }>`, `type Rec<K> =
    ///   Record<K, number>` prints `Rec<"x">`, `type PickAB = Pick<…>`
    ///   prints `PickAB`);
    /// - a generic alias application is named by the outer alias unless the
    ///   target itself prints as what it resolves to (`type G<U> = F<U[]>`
    ///   prints `G<boolean>`, `type OuterCond<T> = Cond<T>` prints the
    ///   selected branch);
    /// - any other builtin (a conditional utility) is what it resolves to.
    ///
    /// [`ReferencePrinting::AsTarget`] for a generic alias application,
    /// which the target's own printing decides.
    fn printed_alias_reference(
        &self,
        target: &crate::semantic_query::DeclIdentity,
        args: &[SemanticNodeId],
    ) -> ReferencePrinting {
        use verter_session_query::declarations::TypeDeclKind;
        if is_builtin(target) {
            return ReferencePrinting::Printed(match BuiltinMappedUtility::of(&target.decl_name) {
                Some(BuiltinMappedUtility::Homomorphic) if !self.declared_union(args.first()) => {
                    PrintedDeclaration::AliasThrough
                }
                Some(_) => PrintedDeclaration::AliasNamed,
                None if self.runtime_nominal_identity(target).is_some() => {
                    PrintedDeclaration::AliasNamed
                }
                None => PrintedDeclaration::AliasTransparent,
            });
        }
        let (Some(kind), Some(generic)) = (
            self.prepared_decl_kind(target),
            self.prepared_decl_is_generic(target),
        ) else {
            return ReferencePrinting::Printed(PrintedDeclaration::AliasTransparent);
        };
        ReferencePrinting::Printed(match kind {
            _ if !generic => PrintedDeclaration::AliasThrough,
            TypeDeclKind::Interface | TypeDeclKind::Class => PrintedDeclaration::AliasNamed,
            TypeDeclKind::Alias if self.alias_declares_homomorphic_mapping(target) => {
                PrintedDeclaration::AliasThrough
            }
            TypeDeclKind::Alias => return ReferencePrinting::AsTarget,
        })
    }

    /// Whether the alias `identity` declares a homomorphic mapped type — a
    /// mapping over `keyof` one of its type parameters, a `Partial` /
    /// `Required` / `Readonly` application over a source that is not a
    /// union, or an application of another alias that does. The checker
    /// gives such a declaration the mapped type's own name, so an alias of
    /// it is printed by that name.
    ///
    /// Every alias the walk passes answers as the one it starts from — the
    /// walk from each continues the same way — so each answer is kept for
    /// the request, and a chain is walked once however many of its aliases
    /// are asked about.
    fn alias_declares_homomorphic_mapping(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> bool {
        if let Some(&known) = self.homomorphic_aliases.borrow().get(identity) {
            return known;
        }
        let mut walked = Vec::new();
        let answer = self.walk_homomorphic_mapping(identity, &mut walked);
        let mut known = self.homomorphic_aliases.borrow_mut();
        for alias in walked {
            known.insert(alias, answer);
        }
        answer
    }

    /// [`Self::alias_declares_homomorphic_mapping`]'s walk from `identity`,
    /// recording in `walked` every alias it passes.
    fn walk_homomorphic_mapping(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        walked: &mut Vec<crate::semantic_query::DeclIdentity>,
    ) -> bool {
        use verter_session_query::declarations::TypeDeclKind;
        let mut visited = rustc_hash::FxHashSet::default();
        let mut current = identity.clone();
        loop {
            if let Some(&known) = self.homomorphic_aliases.borrow().get(&current) {
                return known;
            }
            if !visited.insert(current.clone()) {
                return false;
            }
            walked.push(current.clone());
            let Some(Some(body)) = self.declared_alias_body(&current) else {
                return false;
            };
            let graph = self.graph();
            let next = match graph.node_data(body).as_deref() {
                Some(SemanticNodeData::Mapped { mapper, .. }) => {
                    return match graph.node_data(mapper.key_space).as_deref() {
                        Some(SemanticNodeData::KeyOf { base }) => matches!(
                            graph.node_data(*base).as_deref(),
                            Some(SemanticNodeData::TypeParam { .. })
                        ),
                        _ => false,
                    };
                }
                Some(SemanticNodeData::InstantiationRef { base, args }) if is_builtin(base) => {
                    return BuiltinMappedUtility::of(&base.decl_name)
                        == Some(BuiltinMappedUtility::Homomorphic)
                        && !self.declared_union(args.first());
                }
                Some(SemanticNodeData::InstantiationRef { base, .. }) => base.clone(),
                _ => return false,
            };
            if self.prepared_decl_kind(&next) != Some(TypeDeclKind::Alias) {
                return false;
            }
            current = next;
        }
    }

    /// Whether a declared (unsubstituted) type argument is a union.
    fn declared_union(&self, argument: Option<&SemanticNodeId>) -> bool {
        argument.is_some_and(|argument| {
            matches!(
                self.graph().node_data(*argument).as_deref(),
                Some(SemanticNodeData::Union(_))
            )
        })
    }

    /// How the checker prints a builtin application reached directly: a
    /// mapped utility is printed by its name (`Partial<{ a: 1 }>` prints
    /// `Partial<{ a: 1; }>`, `Pick<…, 'a'>` prints `Pick<{ a: 1; b: 2; },
    /// "a">`, `Partial<Face | Obj>` prints `Partial<Face | Obj>`) except a
    /// homomorphic one over a primitive, array or tuple, which the checker
    /// maps into that type (`Partial<string>` prints `string`). `None` for
    /// every other builtin, which resolves as before.
    fn printed_builtin_application(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        args: &[SemanticNodeId],
        context: ProjectionReductionContext,
    ) -> Option<PrintedDeclaration> {
        match BuiltinMappedUtility::of(&identity.decl_name)? {
            BuiltinMappedUtility::Keyed => Some(PrintedDeclaration::Named),
            BuiltinMappedUtility::Homomorphic => {
                let source = self
                    .normalize_node_for_structural_fact_demand(*args.first()?, context)
                    .into_usable_node()?;
                (!matches!(
                    self.graph().node_data(source).as_deref(),
                    Some(
                        SemanticNodeData::Primitive(_)
                            | SemanticNodeData::Literal(_)
                            | SemanticNodeData::TemplateLiteral { .. }
                            | SemanticNodeData::Array { .. }
                            | SemanticNodeData::Tuple { .. }
                    )
                ))
                .then_some(PrintedDeclaration::Named)
            }
        }
    }

    /// An alias's declared body shape with `Alias` wrappers peeled: `None`
    /// when the body cannot be recovered, `Some(None)` for an alias cycle.
    pub(super) fn declared_alias_body(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> Option<Option<SemanticNodeId>> {
        let mut body = self.declared_body_shape(identity)?;
        let mut aliases = rustc_hash::FxHashSet::default();
        while let Some(SemanticNodeData::Alias(target)) = self.graph().node_data(body).as_deref() {
            if !aliases.insert(body) {
                return Some(None);
            }
            body = *target;
        }
        Some(Some(body))
    }

    /// The declaration's own unsubstituted body shape, through the
    /// memoized locator provider every declaration body lowers through.
    fn declared_body_shape(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> Option<SemanticNodeId> {
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
        };
        match self.lower_locator(AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::clone(&identity.canonical_id),
                owner: identity.owner,
                symbol: Arc::clone(&identity.decl_name),
                space: LocatorSymbolSpace::Type,
            },
            path: Arc::from(Vec::new().into_boxed_slice()),
        })) {
            QueryResult::Value(body) => Some(body),
            QueryResult::Recursive(_) | QueryResult::Error(_) => None,
        }
    }

    /// Whether the alias application `named` declares a mapped type and
    /// settled on an array or tuple: the checker maps a homomorphic mapped
    /// type over an array or tuple to an array or tuple type
    /// (`instantiateMappedType`), which no alias names (`type Boxed<T> = {
    /// [K in keyof T]: { v: T[K] } }` prints `Boxed<[1, 2]>` as `[{ v: 1; },
    /// { v: 2; }]`).
    fn mapped_alias_over_array(&self, named: SemanticNodeId, settled: SemanticNodeId) -> bool {
        let graph = self.graph();
        if !matches!(
            graph.node_data(settled).as_deref(),
            Some(SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. })
        ) {
            return false;
        }
        let identity = match graph.node_data(named).as_deref() {
            Some(
                SemanticNodeData::InstantiationRef { base: identity, .. }
                | SemanticNodeData::DeclRef { identity },
            ) => identity.clone(),
            _ => return false,
        };
        matches!(
            self.declared_alias_body(&identity),
            Some(Some(body)) if matches!(
                graph.node_data(body).as_deref(),
                Some(SemanticNodeData::Mapped { .. })
            )
        )
    }

    /// Whether a named alias application still names the type it settled
    /// on: one of the constructed types the checker attaches an alias to,
    /// a lib interface application (`Promise<number>`) included.
    fn alias_names_settled_type(&self, settled: SemanticNodeId) -> bool {
        match self.graph().node_data(settled).as_deref() {
            Some(
                SemanticNodeData::Object(_)
                | SemanticNodeData::Signature { .. }
                | SemanticNodeData::Mapped { .. }
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Union(_)
                | SemanticNodeData::Intersection(_),
            ) => true,
            Some(SemanticNodeData::InstantiationRef { base, .. }) => {
                self.runtime_nominal_identity(base).is_some()
            }
            _ => false,
        }
    }

    /// The application `application` with each argument read as the checker
    /// prints it ([`Self::resolve_structural_fact_demand`] keeping
    /// declaration names): the checker instantiates an application's
    /// arguments as types, resolving the operators they write
    /// (`Promise<Awaited<T>>` at `T = string` prints `Promise<string>`). An
    /// argument that does not settle stays as written.
    fn application_over_printed_arguments(
        &self,
        application: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let (base, args) = match self.graph().node_data(application).as_deref() {
            Some(SemanticNodeData::InstantiationRef { base, args }) => {
                (base.clone(), Arc::clone(args.as_arc()))
            }
            _ => return application,
        };
        let printed: Vec<SemanticNodeId> = args
            .iter()
            .map(
                |&arg| match self.resolve_structural_fact_demand(arg, context, true, false) {
                    StructuralFactDemandOutcome::Complete(node) => node,
                    _ => arg,
                },
            )
            .collect();
        if printed.as_slice() == args.as_ref() {
            return application;
        }
        self.graph().intern_preserving_scope(
            application,
            SemanticNodeData::InstantiationRef {
                base,
                args: crate::semantic_query::PayloadArray::from(printed.into_boxed_slice()),
            },
        )
    }

    /// The application of `identity` the checker prints for `carrier`: its
    /// supplied arguments followed by every omitted defaulted one
    /// ([`Self::declared_application_arguments`]). The carrier itself when
    /// nothing is omitted or the defaults cannot be recovered.
    fn declared_application(
        &self,
        carrier: SemanticNodeId,
        identity: &crate::semantic_query::DeclIdentity,
        args: &[SemanticNodeId],
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        match self.declared_application_arguments(identity, args, context) {
            Some(complete) if complete.len() > args.len() => self.graph().intern_preserving_scope(
                carrier,
                SemanticNodeData::InstantiationRef {
                    base: identity.clone(),
                    args: crate::semantic_query::PayloadArray::from(complete),
                },
            ),
            _ => carrier,
        }
    }

    /// The keys a `keyof` carrier over a declaration, an application or a
    /// mapped type settles to under a demand that reduces operators
    /// ([`Self::key_of_through_carrier`]); `None` when `keys` is no such
    /// carrier or its keys stay the carrier.
    fn settled_key_of_carrier(
        &self,
        keys: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        if !crate::semantic_query::may_reduce_operator(context) {
            return None;
        }
        let base = match self.graph().node_data(keys).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) => *base,
            _ => return None,
        };
        if !matches!(
            self.graph().node_data(base).as_deref(),
            Some(
                SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::Mapped { .. }
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
            )
        ) {
            return None;
        }
        self.key_of_through_carrier(base, context)
    }

    /// Entry-scoped workhorse for the deferred-shell evaluator. Returns the
    /// resolved node PLUS the typed completeness of THIS evaluation (see
    /// [`EvaluateDeferredOutcome`]).
    ///
    /// The publish gate is ENTRY-scoped: it admits into the shared
    /// `evaluate_deferred_memo` ONLY when the evaluated entry is itself
    /// `Complete` and cacheable. The completeness
    /// accumulator merges every nested `execute_read`'s `result_is_partial`
    /// (the boolean bridge, lifted as [`PartialReasonSet::PROPAGATED`]) and
    /// every operand evaluation's typed completeness (exact reasons
    /// preserved), so a
    /// budget-/recursion-/fatal-tainted result is withheld REGARDLESS of
    /// whether a `RequestContext` is installed — closing the
    /// no-`RequestContext` (`audit Noop`) hole where the request-global
    /// suppress sticky reads `false`. The request sticky
    /// (`current_request_result_is_partial`) is NOT the admission
    /// authority here; `observe_component_meta_read_suppress` is retained
    /// PURELY to propagate the same partiality to the request /
    /// cold-compute scope (the component-meta / materialize warm gates).
    fn deferred_read_action(
        &self,
        frame: &mut DeferredEvaluationFrame,
        read: CacheRead<QueryResult<SemanticNodeId>>,
        miss_fallback: impl FnOnce() -> SemanticNodeId,
    ) -> DeferredEvaluationAction {
        frame.merge_read(&read);
        crate::request_context::observe_component_meta_read_suppress(&read);
        match read.value {
            QueryResult::Value(next) => frame.advance_or_finish(next),
            _ => DeferredEvaluationAction::Finish(miss_fallback()),
        }
    }

    fn finish_deferred_evaluation_frame(
        &self,
        mut frame: DeferredEvaluationFrame,
        result: SemanticNodeId,
        finalise: FactReadSetFinalise,
    ) -> EvaluateDeferredOutcome {
        let mutation_unstable = matches!(&finalise, FactReadSetFinalise::MutationUnstable);
        let evidence = match finalise {
            FactReadSetFinalise::Ok(facts) => {
                use verter_session_query::facts::fact_cache::{signature_entries, FactVersionRef};

                let mut roots = self.observed_self_roots_from_nodes([frame.entry_node, result]);
                // This memo has no single file owner. Every consumed
                // declaration and augmentation contributor is a strict root,
                // including whole-file facts retained on evidence pages.
                crate::fact_signature_helpers::shared_completed_carrier_rooting_traced_files(
                    &roots, &facts,
                )
                .map(Ok)
                .unwrap_or_else(|| {
                    roots.extend(signature_entries(&facts).filter_map(|fact| match fact {
                        FactVersionRef::FileWholeHash { canonical_id, hash } => {
                            Some((Arc::from(canonical_id.as_str()), *hash))
                        }
                        _ => None,
                    }));
                    self.ctx.complete_graph_signature(&roots, facts.to_vec())
                })
                .ok()
            }
            FactReadSetFinalise::NonCacheable(_) => None,
            FactReadSetFinalise::MutationUnstable => {
                crate::request_context::observe_execution_abort(
                    crate::semantic_query::ExecutionAbort::Superseded,
                );
                None
            }
        };
        frame.cache_suppress |= evidence.is_none();
        if let Some(abort) = crate::request_context::current_execution_abort()
            .or(mutation_unstable.then_some(crate::semantic_query::ExecutionAbort::Superseded))
        {
            if frame.recording {
                self.connected_demand.abandon_cost_scope();
            }
            return EvaluateDeferredOutcome::aborted(
                self.opaque(QueryError::ExecutionAborted(abort)),
                abort,
            );
        }
        // A recovery made under the evaluation — an inline relation, a
        // canonical construction — makes it a resource partial, never a
        // published memo entry, even where no read carried its reason here.
        if self.operation_budget_epoch.get() != frame.budget_epoch {
            frame.completeness = frame.completeness.merge(ResultCompleteness::partial(
                PartialReasonSet::OPERATION_BUDGET,
            ));
        }
        if let Some(reasons) = self.connected_demand_trip() {
            if frame.recording {
                self.connected_demand.abandon_cost_scope();
            }
            return EvaluateDeferredOutcome {
                node: frame.entry_node,
                availability: crate::semantic_query::FactStatus::unavailable(
                    frame.completeness.reasons().union(reasons),
                ),
                completeness: frame
                    .completeness
                    .merge(ResultCompleteness::partial(reasons)),
                cache_suppress: frame.cache_suppress,
            };
        }
        let complete = !frame.completeness.is_partial();
        let receipt = if frame.recording {
            if complete {
                let request = crate::request_context::current_request_budget();
                self.connected_demand.seal_cost_scope(request.as_deref())
            } else {
                self.connected_demand.abandon_cost_scope();
                None
            }
        } else {
            None
        };
        if let Some(receipt) = &receipt {
            self.connected_demand
                .record_prerequisite(receipt, super::cost_receipt::Nesting::InPlace);
        }
        if let (true, false, Some(receipt), Some(evidence)) =
            (complete, frame.cache_suppress, receipt, evidence)
        {
            self.graph().evaluate_deferred_memo_publish(
                frame.entry_node,
                frame.context,
                frame.demand,
                result,
                receipt,
                evidence,
            );
        }
        let availability = if !frame.completeness.is_partial() {
            crate::semantic_query::FactStatus::Complete
        } else if matches!(
            frame.availability,
            crate::semantic_query::FactStatus::Approximate(_, _)
        ) && frame.availability.completeness().reasons() == frame.completeness.reasons()
        {
            frame.availability
        } else if matches!(
            self.graph().node_data(result).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
        ) {
            crate::semantic_query::FactStatus::approximate(frame.completeness.reasons())
                .with_interpretation(Some(
                    crate::semantic_query::FactInterpretation::CheckerRecovery,
                ))
        } else {
            crate::semantic_query::FactStatus::unavailable(frame.completeness.reasons())
        };
        EvaluateDeferredOutcome {
            node: result,
            availability,
            completeness: frame.completeness,
            cache_suppress: frame.cache_suppress,
        }
    }

    fn evaluate_deferred_outcome(
        &self,
        node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        self.evaluate_deferred_demand(node, reduction_context, DeferredEvaluationDemand::Shell)
    }

    /// Prepare the arguments a carrier's owner consumes, using the same
    /// eager-alias/deferred-declaration classification as projection. The
    /// source reader keeps nominal result carriers; argument operator demand
    /// stays with this shared evaluator and its ordinary memo/proof pipeline.
    pub(super) fn evaluate_carrier_arguments(
        &self,
        base: &crate::semantic_query::DeclIdentity,
        args: &[SemanticNodeId],
        context: ProjectionReductionContext,
    ) -> CarrierArgumentsOutcome {
        self.evaluate_carrier_arguments_deferring(self.defers_type_arguments(base), args, context)
    }

    /// [`Self::evaluate_carrier_arguments`] for an owner whose
    /// [`Self::defers_type_arguments`] answer the caller already read.
    pub(super) fn evaluate_carrier_arguments_deferring(
        &self,
        defers_type_arguments: bool,
        args: &[SemanticNodeId],
        context: ProjectionReductionContext,
    ) -> CarrierArgumentsOutcome {
        let argument_context = Self::argument_context_for(defers_type_arguments, context);
        let mut completeness = ResultCompleteness::Complete;
        let mut availability = crate::semantic_query::FactStatus::Complete;
        let mut cache_suppress = false;
        let args = args
            .iter()
            .map(|&arg| {
                let context = self.argument_operand_context(arg, argument_context);
                let indexed = matches!(
                    self.graph().node_data(arg).as_deref(),
                    Some(SemanticNodeData::IndexedAccess { .. })
                );
                let outcome = if indexed && !defers_type_arguments {
                    let probe = self.observe_read(|| {
                        self.evaluate_deferred_outcome(
                            arg,
                            argument_context.with_mode(ProjectionMode::Navigate),
                        )
                    });
                    // Only a selected named value replaces the authored
                    // argument. A declined probe contributes no operand fact;
                    // the application body owns that argument's later demand.
                    let named = matches!(
                        self.graph().node_data(probe.value().node).as_deref(),
                        Some(
                            SemanticNodeData::DeclRef { .. }
                                | SemanticNodeData::InstantiationRef { .. }
                        )
                    );
                    let selected = if named {
                        probe.adopt(self)
                    } else {
                        probe.discard()
                    };
                    match selected {
                        Ok(outcome) if named => outcome,
                        Ok(_) => EvaluateDeferredOutcome::complete(arg),
                        Err(abort) => EvaluateDeferredOutcome::partial(arg, abort.reason_class()),
                    }
                } else if crate::semantic_query::may_reduce_operator(context) {
                    self.evaluate_deferred_outcome(arg, context)
                } else {
                    EvaluateDeferredOutcome::complete(arg)
                };
                completeness = completeness.merge(outcome.completeness);
                availability = availability.join(outcome.availability);
                cache_suppress |= outcome.cache_suppress;
                let canonical = self.canonical_argument_carrier(outcome.node);
                if canonical != arg {
                    self.graph().record_origin_edge(
                        canonical,
                        crate::semantic_query::OriginEdgeKind::Normalize,
                        Arc::from([arg]),
                        crate::semantic_query::OriginMeta::None,
                        self.project_generation_signature(),
                    );
                }
                canonical
            })
            .collect();
        CarrierArgumentsOutcome {
            arguments: args,
            completeness,
            availability,
            cache_suppress,
        }
    }

    pub(super) fn evaluate_selected_value_with_context(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.evaluate_selected_value_outcome(node, context)
            .into_complete_active_query_build_node(self)
            .unwrap_or(node)
    }

    pub(super) fn evaluate_selected_value_outcome(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        self.evaluate_deferred_demand(node, context, DeferredEvaluationDemand::SelectedValue)
    }

    pub(super) fn evaluate_mapped_index_value_outcome(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        self.evaluate_deferred_demand(node, context, DeferredEvaluationDemand::MappedIndexValue)
    }

    /// A mapped member binds its helper application through the same
    /// Instantiate query as whole-surface publication. The resulting body's
    /// operators reduce, while its object interiors remain addressable.
    pub(super) fn evaluate_mapped_member_value_outcome(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        let evaluated = self.evaluate_selected_value_outcome(node, context);
        if evaluated.completeness.is_partial() {
            return evaluated;
        }
        let Some(SemanticNodeData::InstantiationRef { base, args }) = self
            .graph()
            .node_data(evaluated.node)
            .as_deref()
            .map(crate::semantic_query::SemanticNodeData::clone)
        else {
            return evaluated;
        };
        let slot = self.type_slot_for(
            Arc::clone(&base.canonical_id),
            base.owner,
            Arc::clone(&base.decl_name),
        );
        let read = self.execute_read(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                args.as_arc().clone(),
                self.instantiate_context_for(&base.canonical_id, context),
            ),
        ));
        let evaluated = EvaluateDeferredOutcome {
            completeness: evaluated
                .completeness
                .or_partial_if(read.result_is_partial(), read.partial_reason_classes()),
            availability: evaluated.availability.join(read.fact_status),
            cache_suppress: evaluated.cache_suppress | read.cache_suppress,
            ..evaluated
        };
        if evaluated.completeness.is_partial() {
            return evaluated;
        }
        let QueryResult::Value(instantiated) = read.value else {
            return evaluated;
        };
        let body = self.evaluate_deferred_outcome(instantiated, context);
        EvaluateDeferredOutcome {
            node: if matches!(
                self.graph().node_data(body.node).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ) {
                evaluated.node
            } else {
                body.node
            },
            completeness: evaluated.completeness.merge(body.completeness),
            availability: evaluated.availability.join(body.availability),
            cache_suppress: evaluated.cache_suppress | body.cache_suppress,
        }
    }

    fn evaluate_deferred_demand(
        &self,
        node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
    ) -> EvaluateDeferredOutcome {
        if let Some(abort) = crate::request_context::current_execution_abort() {
            return EvaluateDeferredOutcome::aborted(
                self.opaque(QueryError::ExecutionAborted(abort)),
                abort,
            );
        }
        // Continuations live on the heap. Authored structural depth has no
        // cap here; only connected operational limits can stop evaluation.
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            return EvaluateDeferredOutcome::partial(node, reasons);
        }
        if self.evaluates_to_itself(node, reduction_context, demand)
            && self.admit_self_evaluation(node, reduction_context, demand)
        {
            return EvaluateDeferredOutcome::complete(node);
        }
        // A completed cacheable evaluation needs no connected work beyond
        // what its receipt charges. Limited outcomes can never reach this
        // memo; one this demand cannot pay for is evaluated here.
        if let Some(served) =
            self.graph()
                .evaluate_deferred_memo_get(node, reduction_context, demand, self.ctx)
        {
            if self.admits_served(&served.receipt) {
                return EvaluateDeferredOutcome::complete(served.value);
            }
        }
        // The root evaluates, and records from its first step: every step
        // it takes is its own cost, so the receipt a later read replays
        // charges exactly what computing it did. (A nested frame pays its
        // first step to its parent whether it is served or computed.)
        let mut root = DeferredEvaluationFrame::new(
            node,
            reduction_context,
            self.operation_budget_epoch.get(),
            demand,
        );
        self.connected_demand.open_cost_scope(
            super::cost_receipt::CostIdentity::of_key((node, reduction_context, demand))
                .with_roots(
                    self.graph()
                        .root_existing_nodes(smallvec::smallvec![node])
                        .expect("evaluation operand is live"),
                ),
        );
        root.memo_checked = true;
        root.recording = true;
        let mut frames = vec![root];
        // Tracers live beside the frames so a step can mutate its frame.
        // Only the current frame is installed; a completed child replays
        // its proof into its parent. Deep operands do not install or copy
        // an entire stack of tracers on every step.
        let mut tracers = vec![crate::fact_signature_helpers::StepwiseFactTracer::new(
            crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx),
        )];
        let mut completed_child: Option<EvaluateDeferredOutcome> = None;

        loop {
            if let Err(reasons) = self.charge_connected_work() {
                for frame in frames.iter().rev() {
                    if frame.recording {
                        self.connected_demand.abandon_cost_scope();
                    }
                }
                deliver_abandoned_evaluation_facts(tracers);
                return aborted_evaluation_outcome(&frames, completed_child.as_ref(), reasons);
            }

            let scope = tracers
                .last_mut()
                .expect("active evaluation tracer")
                .install();
            let action = if let Some(child) = completed_child.take() {
                let frame = frames
                    .last_mut()
                    .expect("a completed operand requires a suspended parent");
                frame.merge_child(child);
                match std::mem::replace(&mut frame.stage, DeferredEvaluationStage::EvaluateCurrent)
                {
                    DeferredEvaluationStage::AwaitKeyOfBase => {
                        let mut read = self.execute_read(SemanticQueryKey::KeyOf {
                            base: child.node,
                            context: frame.context,
                        });
                        // A `keyof` the builder kept as a carrier over a
                        // declaration, an application or a mapped type is
                        // evaluated here, at a demand for its value: its
                        // keys where they settle, as the checker prints
                        // them.
                        if frame.demand == DeferredEvaluationDemand::Shell {
                            if let QueryResult::Value(keys) = read.value {
                                if let Some(settled) =
                                    self.settled_key_of_carrier(keys, frame.context)
                                {
                                    read.value = QueryResult::Value(settled);
                                }
                            }
                        }
                        self.deferred_read_action(frame, read, || self.opaque(QueryError::Miss))
                    }
                    DeferredEvaluationStage::AwaitIndexedObject { index } => match index {
                        IndexKey::Computed(index_node) => {
                            frame.stage =
                                DeferredEvaluationStage::AwaitIndexedIndex { object: child.node };
                            DeferredEvaluationAction::Push {
                                node: index_node,
                                context: ProjectionReductionContext::published(
                                    ProjectionMode::Expanded,
                                )
                                .with_orthogonal_axes_from(frame.context),
                            }
                        }
                        index
                            if self.indexed_access_proves_absence(
                                child.node,
                                &index,
                                frame.context,
                            ) =>
                        {
                            // A completed member domain owns optional absence.
                            DeferredEvaluationAction::Finish(self.opaque(QueryError::Miss))
                        }
                        index => {
                            let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                                base: child.node,
                                index,
                                mode: frame.context.mode,
                            });
                            self.deferred_read_action(frame, read, || self.opaque(QueryError::Miss))
                        }
                    },
                    DeferredEvaluationStage::AwaitIndexedIndex { object } => {
                        let index = self.normalized_index_key_from_evaluated_node(child.node);
                        let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                            base: object,
                            index,
                            mode: frame.context.mode,
                        });
                        self.deferred_read_action(frame, read, || self.opaque(QueryError::Miss))
                    }
                    DeferredEvaluationStage::AwaitValueChildren {
                        data,
                        children,
                        mut values,
                    } => {
                        values.push(child.node);
                        if let Some(&next) = children.get(values.len()) {
                            frame.stage = DeferredEvaluationStage::AwaitValueChildren {
                                data,
                                children,
                                values,
                            };
                            DeferredEvaluationAction::Push {
                                node: next,
                                context: frame.context,
                            }
                        } else {
                            let rebuilt =
                                self.rebuild_evaluated_value(frame.node, &data, &children, &values);
                            DeferredEvaluationAction::Finish(rebuilt)
                        }
                    }
                    DeferredEvaluationStage::EvaluateCurrent => {
                        unreachable!("only a suspended frame can receive an operand")
                    }
                }
            } else {
                let (entry_node, context, demand, check_memo) = {
                    let frame = frames
                        .last_mut()
                        .expect("the evaluator retains a root frame");
                    let check_memo = !frame.memo_checked;
                    frame.memo_checked = true;
                    (frame.entry_node, frame.context, frame.demand, check_memo)
                };
                let memo_hit = if check_memo {
                    self.graph()
                        .evaluate_deferred_memo_get(entry_node, context, demand, self.ctx)
                        .filter(|served| self.admits_served(&served.receipt))
                        .map(|served| served.value)
                } else {
                    None
                };
                if check_memo && memo_hit.is_none() {
                    // The frame evaluates: its charges are its own.
                    self.connected_demand.open_cost_scope(
                        super::cost_receipt::CostIdentity::of_key((entry_node, context, demand))
                            .with_roots(
                                self.graph()
                                    .root_existing_nodes(smallvec::smallvec![entry_node])
                                    .expect("evaluation operand is live"),
                            ),
                    );
                    frames
                        .last_mut()
                        .expect("the evaluator retains a root frame")
                        .recording = true;
                }
                if let Some(cached) = memo_hit {
                    DeferredEvaluationAction::Cached(cached)
                } else {
                    let current = frames
                        .last()
                        .expect("the evaluator retains a root frame")
                        .node;
                    if let Some(data) = self.graph().node_data(current) {
                        match data.as_ref() {
                            _ if demand != DeferredEvaluationDemand::Shell
                                && self
                                    .selected_value_children(data.as_ref(), context)
                                    .is_some() =>
                            {
                                let children = self
                                    .selected_value_children(data.as_ref(), context)
                                    .expect("selected child topology");
                                if let Some(&first) = children.first() {
                                    let frame = frames.last_mut().expect("active evaluator frame");
                                    frame.stage = DeferredEvaluationStage::AwaitValueChildren {
                                        data: Arc::clone(&data),
                                        children,
                                        values: Vec::new(),
                                    };
                                    DeferredEvaluationAction::Push {
                                        node: first,
                                        context,
                                    }
                                } else {
                                    DeferredEvaluationAction::Finish(current)
                                }
                            }
                            SemanticNodeData::Alias(target) => frames
                                .last_mut()
                                .expect("active evaluator frame")
                                .advance_or_finish(*target),
                            SemanticNodeData::KeyOf { base } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.stage = DeferredEvaluationStage::AwaitKeyOfBase;
                                DeferredEvaluationAction::Push {
                                    node: *base,
                                    context: frame.context.into_structural_transit_with_mode(
                                        ProjectionMode::Navigate,
                                    ),
                                }
                            }
                            SemanticNodeData::IndexedAccess { object, index } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.stage = DeferredEvaluationStage::AwaitIndexedObject {
                                    index: clone_index_key(index),
                                };
                                DeferredEvaluationAction::Push {
                                    node: *object,
                                    context: self
                                        .indexed_access_operand_context(*object, frame.context),
                                }
                            }
                            SemanticNodeData::Mapped { source, mapper } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                let read = self.execute_read(SemanticQueryKey::MappedType {
                                    source: *source,
                                    mapper: mapper.clone(),
                                    context: frame.context,
                                });
                                self.deferred_read_action(frame, read, || {
                                    self.opaque(QueryError::Miss)
                                })
                            }
                            // A NOMINAL (`unique symbol`) carrier is
                            // TERMINAL: resolving its head would project the
                            // annotation down to the shared `symbol`
                            // primitive and erase the declaring identity that
                            // IS the type. Finish on the carrier.
                            SemanticNodeData::TypeOfNominal(_) => frames
                                .last_mut()
                                .expect("active evaluator frame")
                                .advance_or_finish(current),
                            SemanticNodeData::TypeOf(_) => {
                                let (value_root, path) =
                                    data.typeof_head().expect("TypeOf carrier head");
                                let value_root = value_root.clone();
                                let path = Arc::clone(path);
                                let type_args: Vec<SemanticNodeId> =
                                    data.carrier_type_args().to_vec();
                                let context =
                                    frames.last().expect("active evaluator frame").context;
                                let read = self.execute_read(
                                    self.typeof_key_with_path(value_root, path, context),
                                );
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.merge_read(&read);
                                crate::request_context::observe_component_meta_read_suppress(&read);
                                match read.value {
                                    QueryResult::Value(projected) => {
                                        let next = if type_args.is_empty() {
                                            projected
                                        } else {
                                            self.apply_typeof_instantiation_args(
                                                projected, &type_args,
                                            )
                                        };
                                        frames
                                            .last_mut()
                                            .expect("active evaluator frame")
                                            .advance_or_finish(next)
                                    }
                                    _ => DeferredEvaluationAction::Finish(
                                        self.opaque(QueryError::Miss),
                                    ),
                                }
                            }
                            SemanticNodeData::Conditional {
                                check,
                                extends,
                                true_branch_ref,
                                false_branch_ref,
                                distributive,
                                pending,
                            } => {
                                let read = self.execute_read(SemanticQueryKey::Conditional {
                                    check: *check,
                                    extends: *extends,
                                    true_branch: *true_branch_ref,
                                    false_branch: *false_branch_ref,
                                    distributive: *distributive,
                                    pending: pending.clone(),
                                });
                                let frame = frames.last_mut().expect("active evaluator frame");
                                self.deferred_read_action(frame, read, || {
                                    self.opaque(QueryError::Miss)
                                })
                            }
                            SemanticNodeData::TemplateLiteral {
                                quasis,
                                expressions,
                            } => {
                                let read =
                                    self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
                                        pattern: Arc::clone(quasis),
                                        args: Arc::clone(expressions.as_arc()),
                                        context: self.template_literal_reduce_context(),
                                    });
                                let frame = frames.last_mut().expect("active evaluator frame");
                                self.deferred_read_action(frame, read, || current)
                            }
                            SemanticNodeData::InstantiationRef { base, args }
                                if demand == DeferredEvaluationDemand::MappedIndexValue
                                    && !matches!(
                                        self.prepared_decl_kind(base),
                                        Some(
                                            verter_session_query::declarations::TypeDeclKind::Interface
                                                | verter_session_query::declarations::TypeDeclKind::Class
                                        )
                                    ) =>
                            {
                                let slot = self.type_slot_for(
                                    Arc::clone(&base.canonical_id),
                                    base.owner,
                                    Arc::clone(&base.decl_name),
                                );
                                let read = self.execute_read(SemanticQueryKey::Instantiate(
                                    crate::semantic_query::InstantiateKey::new(
                                        slot,
                                        Arc::clone(args.as_arc()),
                                        self.instantiate_context_for(&base.canonical_id, context),
                                    ),
                                ));
                                let frame = frames.last_mut().expect("active evaluator frame");
                                self.deferred_read_action(frame, read, || current)
                            }
                            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                                canonical_id,
                                owner,
                                name,
                                whole_hash: _,
                            }) => {
                                let base = self.type_slot_for(
                                    Arc::clone(canonical_id),
                                    *owner,
                                    Arc::clone(name),
                                );
                                let context =
                                    frames.last().expect("active evaluator frame").context;
                                let read = self.execute_read(SemanticQueryKey::Instantiate(
                                    crate::semantic_query::InstantiateKey::new(
                                        base,
                                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                                        self.instantiate_context_for(canonical_id, context),
                                    ),
                                ));
                                let frame = frames.last_mut().expect("active evaluator frame");
                                self.deferred_read_action(frame, read, || {
                                    self.opaque(QueryError::Miss)
                                })
                            }
                            SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {
                                let context = frames
                                    .last()
                                    .expect("active evaluator frame")
                                    .context
                                    .into_structural_transit_with_mode(ProjectionMode::Navigate);
                                let (resolved, observed, carrier_completeness) = self
                                    .resolve_carrier_subject_node_capturing_suppress(
                                        current, context,
                                    );
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.completeness = frame.completeness.merge(carrier_completeness);
                                frame.completeness = frame.completeness.or_partial_if(
                                    observed.result_is_partial
                                        && !carrier_completeness.is_partial(),
                                    observed.partial_reasons,
                                );
                                frame.cache_suppress |= observed.cache_suppress;
                                frame.availability = frame.availability.join(resolved.status);
                                frame.advance_or_finish(resolved.node)
                            }
                            _ => DeferredEvaluationAction::Finish(current),
                        }
                    } else {
                        let frame = frames.last_mut().expect("active evaluator frame");
                        frame.completeness = frame.completeness.merge(ResultCompleteness::partial(
                            PartialReasonSet::MISSING_SEMANTIC_NODE_DATA,
                        ));
                        DeferredEvaluationAction::Finish(self.opaque(QueryError::Miss))
                    }
                }
            };

            drop(scope);
            match action {
                DeferredEvaluationAction::Continue => {}
                DeferredEvaluationAction::Push { node, context } => {
                    let parent = frames.last().expect("a child has a parent demand");
                    // Key enumeration consumes the source's domain,
                    // without demanding all of its member values.
                    let demand = if matches!(parent.stage, DeferredEvaluationStage::AwaitKeyOfBase)
                    {
                        DeferredEvaluationDemand::Shell
                    } else {
                        parent.demand
                    };
                    if self.evaluates_to_itself(node, context, demand) {
                        // The operand's one step is paid to this frame, as
                        // a pushed operand's first step is.
                        if let Err(reasons) = self.charge_connected_work() {
                            for frame in frames.iter().rev() {
                                if frame.recording {
                                    self.connected_demand.abandon_cost_scope();
                                }
                            }
                            deliver_abandoned_evaluation_facts(tracers);
                            return aborted_evaluation_outcome(&frames, None, reasons);
                        }
                        completed_child = Some(EvaluateDeferredOutcome::complete(node));
                        continue;
                    }
                    frames.push(DeferredEvaluationFrame::new(
                        node,
                        context,
                        self.operation_budget_epoch.get(),
                        demand,
                    ));
                    tracers.push(crate::fact_signature_helpers::StepwiseFactTracer::new(
                        crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(self.ctx),
                    ));
                }
                DeferredEvaluationAction::Finish(result) => {
                    let frame = frames.pop().expect("finishing an active evaluator frame");
                    let tracer = tracers.pop().expect("each evaluation owns a tracer");
                    let finalise = tracer.finish();
                    deliver_evaluation_facts(&mut tracers, &finalise);
                    let outcome = self.finish_deferred_evaluation_frame(frame, result, finalise);
                    if frames.is_empty() {
                        return outcome;
                    }
                    completed_child = Some(outcome);
                }
                DeferredEvaluationAction::Cached(cached) => {
                    let finalise = tracers
                        .pop()
                        .expect("each evaluation owns a tracer")
                        .finish();
                    deliver_evaluation_facts(&mut tracers, &finalise);
                    frames
                        .pop()
                        .expect("a memo hit belongs to an active evaluator frame");
                    let mut outcome = EvaluateDeferredOutcome::complete(cached);
                    match finalise {
                        FactReadSetFinalise::Ok(_) => {}
                        FactReadSetFinalise::NonCacheable(_) => outcome.cache_suppress = true,
                        FactReadSetFinalise::MutationUnstable => {
                            crate::request_context::observe_execution_abort(
                                crate::semantic_query::ExecutionAbort::Superseded,
                            );
                            let abort = crate::request_context::current_execution_abort()
                                .unwrap_or(crate::semantic_query::ExecutionAbort::Superseded);
                            outcome = EvaluateDeferredOutcome::aborted(
                                self.opaque(QueryError::ExecutionAborted(abort)),
                                abort,
                            );
                        }
                    }
                    if let Some(abort) = crate::request_context::current_execution_abort() {
                        outcome = EvaluateDeferredOutcome::aborted(
                            self.opaque(QueryError::ExecutionAborted(abort)),
                            abort,
                        );
                    }
                    if frames.is_empty() {
                        return outcome;
                    }
                    completed_child = Some(outcome);
                }
            }
        }
    }

    /// Admit a root evaluation that finishes as itself
    /// ([`Self::evaluates_to_itself`]) without running it: its cold run is
    /// one step under its own identity with an empty read set, so its
    /// receipt is known before it runs. The receipt is admitted as a stored
    /// completion's would be — charged once per connected demand, its
    /// request operations marked paid, and recorded as the consumer's
    /// prerequisite in place — with no frame, tracer or memo entry of its
    /// own. `false` is a demand that cannot pay for it: the evaluation then
    /// runs, and reports the limit, as a cold one does.
    fn admit_self_evaluation(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
    ) -> bool {
        let key = (node, context, demand);
        let cached = self.self_evaluation_receipts.borrow().get(&key).cloned();
        let receipt = match cached {
            Some(receipt) => receipt,
            None => {
                let Ok(roots) = self.graph().root_existing_nodes(smallvec::smallvec![node]) else {
                    return false;
                };
                let receipt = super::cost_receipt::DemandCostReceipt::new(
                    super::cost_receipt::CostIdentity::of_key(key).with_roots(roots),
                    super::cost_receipt::LogicalUsage {
                        work: 1,
                        ..Default::default()
                    },
                    Vec::new(),
                );
                self.self_evaluation_receipts
                    .borrow_mut()
                    .insert(key, Arc::clone(&receipt));
                receipt
            }
        };
        matches!(
            self.admit_served(&receipt, super::cost_receipt::Nesting::InPlace),
            super::ServedAdmission::Admitted
        )
    }

    /// Whether the evaluator finishes `node` as itself in its first step
    /// without reading a fact or naming an operand: no arm of the step
    /// rewrites it and its demand selects no value children. Such an
    /// evaluation completes as `node` with an empty read set: sealing that
    /// empty set under the node's own self-root cannot refuse, and a memo
    /// hit replays no fact. A pushed operand of this kind is therefore
    /// finished in place, paying only the step a pushed operand pays its
    /// parent, with no frame, tracer or memo probe of its own.
    fn evaluates_to_itself(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        demand: DeferredEvaluationDemand,
    ) -> bool {
        let Some(data) = self.graph().node_data(node) else {
            return false;
        };
        let rewritten = match data.as_ref() {
            SemanticNodeData::Alias(_)
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => true,
            // The mapped-index-value arm reads the declaration's kind.
            SemanticNodeData::InstantiationRef { .. } => {
                demand == DeferredEvaluationDemand::MappedIndexValue
            }
            data => {
                demand != DeferredEvaluationDemand::Shell
                    && self.selected_value_children(data, context).is_some()
            }
        };
        !rewritten
    }

    fn selected_value_children(
        &self,
        data: &SemanticNodeData,
        context: ProjectionReductionContext,
    ) -> Option<Vec<SemanticNodeId>> {
        let whole = crate::semantic_query::may_reduce_operator(context)
            && context.mode == ProjectionMode::Expanded;
        match data {
            SemanticNodeData::Union(arms) => Some(arms.iter().copied().collect()),
            SemanticNodeData::Array { element, .. } if whole => Some(vec![*element]),
            SemanticNodeData::Tuple { elements, .. } if whole => {
                Some(elements.iter().map(|e| e.value).collect())
            }
            SemanticNodeData::Object(surface) if whole => {
                Some(surface.positive_members().iter().map(|m| m.value).collect())
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(_),
                ..
            } if whole => Some(
                params
                    .iter()
                    .map(|p| p.ty)
                    .chain(std::iter::once(*return_type))
                    .collect(),
            ),
            _ => None,
        }
    }

    fn rebuild_evaluated_value(
        &self,
        node: SemanticNodeId,
        data: &SemanticNodeData,
        children: &[SemanticNodeId],
        values: &[SemanticNodeId],
    ) -> SemanticNodeId {
        if children == values {
            return node;
        }
        let rebuilt = match data {
            SemanticNodeData::Union(_) => {
                return self.intern_normalized_union_or_intersection(values, true)
            }
            SemanticNodeData::Array { readonly, .. } => SemanticNodeData::Array {
                element: values[0],
                readonly: *readonly,
            },
            SemanticNodeData::Tuple { elements, readonly } => {
                let elements = elements
                    .iter()
                    .zip(values)
                    .map(|(e, &value)| crate::semantic_query::TupleElement { value, ..e.copy() })
                    .collect();
                SemanticNodeData::Tuple {
                    elements,
                    readonly: *readonly,
                }
            }
            SemanticNodeData::Object(surface) => {
                // Entries own the surface; the member list is its derived
                // index. Rebuild the stream so both readers see the value.
                let mut values = values.iter();
                let entries = surface
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        crate::semantic_query::SurfaceEntry::Member(member) => {
                            crate::semantic_query::SurfaceEntry::Member(
                                crate::semantic_query::SurfaceMember {
                                    value: *values
                                        .next()
                                        .expect("each member has an evaluated value"),
                                    ..member.clone()
                                },
                            )
                        }
                        other => other.clone(),
                    })
                    .collect();
                SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                    entries,
                    surface.keyspace,
                    surface.has_known_index_signature(),
                ))
            }
            SemanticNodeData::Signature {
                kind,
                params,
                type_parameters,
                occurrence,
                signature_span,
                return_type_span,
                predicate,
                is_abstract,
                ..
            } => {
                let params = params
                    .iter()
                    .zip(values)
                    .map(|(p, &ty)| crate::semantic_query::FunctionParam { ty, ..p.copy() })
                    .collect();
                let return_type = values[values.len() - 1];
                SemanticNodeData::Signature {
                    kind: *kind,
                    params,
                    return_type,
                    type_parameters: type_parameters.share(),
                    occurrence: occurrence.clone(),
                    return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(
                        return_type,
                    ),
                    signature_span: *signature_span,
                    return_type_span: *return_type_span,
                    predicate: *predicate,
                    is_abstract: *is_abstract,
                }
            }
            _ => unreachable!("only selected value composition is rebuilt"),
        };
        self.graph().intern_preserving_scope(node, rebuilt)
    }

    /// The union or intersection `node` reduced over its arms' RESOLVED
    /// types, as the checker's `getUnionType` / `getIntersectionType`
    /// construct it: an arm written as a name (an alias, an enum, an
    /// application, a `typeof`) takes part in literal subsumption, the
    /// `any` / `unknown` / `never` lattice, disjoint-literal collapse and
    /// intersection distribution as the type it names. `None` when the
    /// reduction changes nothing, or no arm is such a name.
    ///
    /// A union reads a named arm that resolves to a union as that union's
    /// members; any other arm is read as its resolved type only where that
    /// type is made of literals, primitives and enum members (an
    /// intersection distributes over a named union of them only): an object
    /// type takes part in no reduction here, so its arm keeps its name. A union keeps its written names as the checker keeps its
    /// origin (`getUnionType`): one named union holding every other
    /// member is that name (`A | 1` is `A`), and named unions that do not
    /// overlap stay beside the remaining members (`A | 3 | S` is `A | S`);
    /// a reduction that removed a member of a named union answers the
    /// reduced set (`A | number` is `number`). An intersection arm that
    /// resolves to the missing-member marker makes the intersection that
    /// marker: its value is unknown.
    ///
    /// A composite whose reduction re-enters itself (an alias union naming
    /// itself through its arms) stays as written: the re-entry reads
    /// through the same `carrier_normalizing` in-progress record the
    /// carrier normalizer keeps, never a depth limit.
    pub(super) fn composite_over_resolved_arms(
        &self,
        node: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let reduction = self.begin_composite(node, false)?;
        match self.drive_structural_fact_demands(DemandFrame::Composite(reduction)) {
            DrivenRoot::Composite(reduced) => reduced,
            DrivenRoot::Demand(_) => unreachable!("a composite root finishes as a composite"),
        }
    }

    /// Run `root`, and every structural-fact demand it suspends on, from
    /// one explicit stack of frames. A composite reduction that needs a
    /// name's demand suspends; the demand is begun, and when it settles on
    /// a composite of its own, that reduction runs above it; a finished
    /// demand's outcome resumes the reduction below. The frames run in
    /// exactly the order the nested calls would, with the same connected
    /// demand held open and the same `carrier_normalizing` record, so a
    /// chain of names of any length spends no native stack per name.
    ///
    /// A name's completed demand is read once per run. A reduction reads
    /// the demand of every name its arms write, and the walk over a named
    /// union's members reads the names that union writes — the same names
    /// the arm's own demand already read — so without reuse a chain of
    /// union aliases demanded every link once per link above it, and the
    /// work doubled per link (18 links exhausted the connected budget).
    /// A completed outcome is reused only when its demand met no composite
    /// already being reduced below it: such a re-entry leaves that composite
    /// as written, which is where the demand ran, not what it names. The
    /// reuse record lives for this run only.
    fn drive_structural_fact_demands<'s>(&'s self, root: DemandFrame<'s>) -> DrivenRoot {
        let transit =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let mut stack = vec![root];
        let mut delivered: Option<StructuralFactDemandOutcome> = None;
        let mut completed: rustc_hash::FxHashMap<SemanticNodeId, StructuralFactDemandOutcome> =
            rustc_hash::FxHashMap::default();
        loop {
            let top = stack
                .last_mut()
                .expect("the driver runs while a frame is open");
            let reduction = match top {
                DemandFrame::Composite(reduction) => reduction,
                DemandFrame::Demand {
                    settled,
                    composite,
                    reentered,
                    ..
                } => {
                    if composite.is_none() {
                        // A chain that did not settle is not reduced.
                        let started = if settled.exit_reasons.is_none() {
                            let started =
                                self.begin_composite(settled.n, !settled.resolve_declaration_refs);
                            if started.is_none()
                                && self.carrier_normalizing.borrow().contains(&settled.n)
                            {
                                *reentered = true;
                            }
                            started
                        } else {
                            None
                        };
                        match started {
                            Some(reduction) => *composite = Some(reduction),
                            None => {
                                let finished = stack.pop();
                                delivered = Some(self.finish_demand_frame(
                                    finished,
                                    None,
                                    ResultCompleteness::Complete,
                                    &mut stack,
                                    &mut completed,
                                ));
                                if stack.is_empty() {
                                    return DrivenRoot::Demand(
                                        delivered.take().expect("the root's outcome"),
                                    );
                                }
                                continue;
                            }
                        }
                    }
                    composite.as_mut().expect("the reduction is started")
                }
            };
            match self.step_composite(reduction, delivered.take()) {
                CompositeStep::Demand(name) => {
                    if let Some(outcome) = completed.get(&name) {
                        delivered = Some(*outcome);
                        continue;
                    }
                    match self.begin_structural_fact_demand(name, transit, true, true) {
                        BegunDemand::Finished(outcome) => delivered = Some(outcome),
                        BegunDemand::Settled(settled) => stack.push(DemandFrame::Demand {
                            settled,
                            composite: None,
                            name: Some(name),
                            reentered: false,
                        }),
                    }
                }
                CompositeStep::Done(reduced, completeness) => {
                    let finished = stack.pop();
                    if let Some(DemandFrame::Composite(_)) = finished {
                        // A composite asked for on its own answers its node;
                        // the demands it read folded their partials where
                        // they finished.
                        return DrivenRoot::Composite(reduced);
                    }
                    delivered = Some(self.finish_demand_frame(
                        finished,
                        reduced,
                        completeness,
                        &mut stack,
                        &mut completed,
                    ));
                    if stack.is_empty() {
                        return DrivenRoot::Demand(delivered.take().expect("the root's outcome"));
                    }
                }
            }
        }
    }

    /// Finish the demand frame just popped, record its outcome for reuse when
    /// it completed with no re-entry, and hand a re-entry down to the frame
    /// below, whose outcome is built from this one.
    fn finish_demand_frame<'s>(
        &'s self,
        finished: Option<DemandFrame<'s>>,
        reduced: Option<SemanticNodeId>,
        completeness: ResultCompleteness,
        stack: &mut [DemandFrame<'s>],
        completed: &mut rustc_hash::FxHashMap<SemanticNodeId, StructuralFactDemandOutcome>,
    ) -> StructuralFactDemandOutcome {
        let Some(DemandFrame::Demand {
            settled,
            name,
            reentered,
            ..
        }) = finished
        else {
            unreachable!("a demand frame finishes as a demand")
        };
        let outcome = self.finish_structural_fact_demand(settled, reduced, completeness);
        if reentered {
            if let Some(DemandFrame::Demand {
                reentered: below, ..
            }) = stack.last_mut()
            {
                *below = true;
            }
        } else if let (Some(name), StructuralFactDemandOutcome::Complete(_)) = (name, outcome) {
            completed.insert(name, outcome);
        }
        outcome
    }

    /// Whether `arm` is written as a name: a union or intersection reads it
    /// as the type it names.
    fn is_composite_name(&self, arm: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(arm).as_deref(),
            Some(
                SemanticNodeData::Alias(_)
                    | SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::TypeOf(_)
                    | SemanticNodeData::Conditional { .. }
                    | SemanticNodeData::IndexedAccess { .. }
                    | SemanticNodeData::BareRef(_)
                    | SemanticNodeData::ImportType(_)
                    | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
            )
        )
    }

    /// Whether a resolved type is a literal, a primitive or an enum member
    /// — the types the composite reductions act on.
    fn is_composite_scalar(&self, view: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(view).as_deref(),
            Some(
                SemanticNodeData::Primitive(_)
                    | SemanticNodeData::Literal(_)
                    | SemanticNodeData::EnumLiteral(_)
                    | SemanticNodeData::Opaque(_)
            )
        )
    }

    /// Start the composite reduction of `node`: `None` when it is not a
    /// union or a reducible intersection, no arm is a name, or its reduction
    /// is already in progress (an alias union naming itself through its
    /// arms stays as written — the re-entry reads the same
    /// `carrier_normalizing` record the carrier normalizer keeps, never a
    /// depth limit). A started reduction is on that record until it is done.
    fn begin_composite(
        &self,
        node: SemanticNodeId,
        keep_origin: bool,
    ) -> Option<CompositeReduction> {
        use crate::semantic_query::composite::CompositeOriginCategory as Category;
        let mut re_decidable = true;
        let (arms, is_union) = match self.graph().node_data(node)?.as_ref() {
            SemanticNodeData::Union(members) => {
                re_decidable = matches!(
                    members.origin_category(),
                    Category::Canonical(_) | Category::CanonicalUnproven | Category::AuthoredShell
                );
                (members.members_arc(), true)
            }
            SemanticNodeData::Intersection(members)
                if matches!(
                    members.origin_category(),
                    Category::Canonical(_) | Category::CanonicalUnproven | Category::AuthoredShell
                ) =>
            {
                (members.members_arc(), false)
            }
            _ => return None,
        };
        // A union without a name still reduces as the checker's union does
        // (`"a" | string` is `string`) when its arm list is one the
        // canonical authority may re-decide; an intersection without one has
        // nothing to resolve.
        if !(is_union && re_decidable) && !arms.iter().any(|arm| self.is_composite_name(*arm)) {
            return None;
        }
        if self.carrier_normalizing.borrow().contains(&node) {
            return None;
        }
        self.carrier_normalizing.borrow_mut().push(node);
        let capacity = arms.len();
        Some(CompositeReduction {
            node,
            arms,
            is_union,
            next: 0,
            views: Vec::with_capacity(capacity),
            named_unions: Vec::new(),
            named_object_unions: Vec::new(),
            keep_origin,
            waiting: None,
            completeness: ResultCompleteness::Complete,
        })
    }

    /// The reduction is done: off the `carrier_normalizing` record.
    fn end_composite(
        &self,
        reduction: &CompositeReduction,
        reduced: Option<SemanticNodeId>,
    ) -> CompositeStep {
        let popped = self.carrier_normalizing.borrow_mut().pop();
        verter_debug_assert!(
            popped == Some(reduction.node),
            "composite reductions finish in the order they start"
        );
        CompositeStep::Done(reduced, reduction.completeness)
    }

    /// Advance `reduction` with `delivered`, the outcome of the demand it
    /// was waiting for, to its next demand or its result: each arm read as
    /// the reduction reads it, and the resolved union a named arm stands for.
    fn step_composite(
        &self,
        reduction: &mut CompositeReduction,
        delivered: Option<StructuralFactDemandOutcome>,
    ) -> CompositeStep {
        if let Some(outcome) = delivered {
            let (arm, members) = match reduction.waiting.take() {
                Some(ArmWait::Resolved(arm)) => {
                    let resolved = match outcome {
                        StructuralFactDemandOutcome::Complete(resolved) => resolved,
                        // Only the checker's explicit recovery absorbs other
                        // positions. A positional approximation cannot prove
                        // a lattice extreme or a complete union domain.
                        StructuralFactDemandOutcome::Recovered { node, reasons } => {
                            reduction.completeness = reduction
                                .completeness
                                .merge(ResultCompleteness::partial(reasons));
                            if !matches!(
                                self.graph().node_data(node).as_deref(),
                                Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
                            ) {
                                return self.end_composite(reduction, None);
                            }
                            node
                        }
                        StructuralFactDemandOutcome::Partial(reasons) => {
                            reduction.completeness = reduction
                                .completeness
                                .merge(ResultCompleteness::partial(reasons));
                            return self.end_composite(reduction, None);
                        }
                    };
                    // A union joins a named union's members, whatever they
                    // are; an intersection distributes over one made of
                    // scalars only.
                    let is_named_union = matches!(
                        self.graph().node_data(resolved).as_deref(),
                        Some(SemanticNodeData::Union(_))
                    );
                    if !is_named_union {
                        let view = if self.is_composite_scalar(resolved) {
                            resolved
                        } else {
                            arm
                        };
                        reduction.views.push(view);
                        (arm, None)
                    } else {
                        let mut walk = MembersWalk::new(arm, resolved);
                        match self.step_members(&mut walk, None) {
                            MembersStep::Demand(name) => {
                                reduction.waiting = Some(ArmWait::Members { arm, walk });
                                return CompositeStep::Demand(name);
                            }
                            MembersStep::Done(members) => {
                                reduction.take_walk_partial(&walk);
                                (arm, Some(members))
                            }
                        }
                    }
                }
                Some(ArmWait::Members { arm, mut walk }) => {
                    match self.step_members(&mut walk, Some(outcome)) {
                        MembersStep::Demand(name) => {
                            reduction.waiting = Some(ArmWait::Members { arm, walk });
                            return CompositeStep::Demand(name);
                        }
                        MembersStep::Done(members) => {
                            reduction.take_walk_partial(&walk);
                            (arm, Some(members))
                        }
                    }
                }
                None => unreachable!("an outcome is delivered only to a waiting reduction"),
            };
            if let Some(members) = members {
                let Some(members) = members else {
                    return self.end_composite(reduction, None);
                };
                if !reduction.is_union
                    && !members
                        .iter()
                        .all(|member| self.is_composite_scalar(*member))
                {
                    reduction.views.push(arm);
                    reduction.named_object_unions.push(arm);
                } else {
                    reduction
                        .views
                        .push(self.intern_normalized_union_or_intersection(&members, true));
                    reduction.named_unions.push((arm, members));
                }
            }
        }
        while reduction.next < reduction.arms.len() {
            let arm = reduction.arms[reduction.next];
            reduction.next += 1;
            if !self.is_composite_name(arm) {
                reduction.views.push(arm);
                continue;
            }
            reduction.waiting = Some(ArmWait::Resolved(arm));
            return CompositeStep::Demand(arm);
        }
        let reduced = self.reduce_over_resolved_arms(reduction);
        self.end_composite(reduction, reduced)
    }

    /// The composite once every arm is read: the reduced node, or `None`
    /// when it stays as written.
    fn reduce_over_resolved_arms(&self, reduction: &CompositeReduction) -> Option<SemanticNodeId> {
        let graph = self.graph();
        let node = reduction.node;
        let arms: &[SemanticNodeId] = &reduction.arms;
        let views = &reduction.views;
        let named_unions = &reduction.named_unions;
        if !reduction.is_union {
            if let Some(recovery) = self.named_intersection_too_complex(reduction) {
                return Some(recovery);
            }
        }
        if !reduction.is_union && views.as_slice() == arms {
            return None;
        }
        let is_opaque = |view: SemanticNodeId| {
            matches!(
                graph.node_data(view).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            )
        };
        if !reduction.is_union {
            if let Some(marker) = views.iter().copied().find(|view| is_opaque(*view)) {
                return Some(marker);
            }
            let reduced = self
                .distributed_intersection(views)
                .unwrap_or_else(|| self.intern_normalized_union_or_intersection(views, false));
            return (reduced != node).then_some(reduced);
        }
        // An arm this demand could not type leaves the union's member set
        // unknown: it stays as written.
        if views.iter().any(|view| is_opaque(*view)) {
            return None;
        }
        // A template literal arm is the type its construction builds
        // (`${string}${string}` is `string`), as the checker holds it
        // before the union reduces.
        let views: Vec<SemanticNodeId> = views
            .iter()
            .map(|view| match graph.node_data(*view).as_deref() {
                Some(SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                }) => {
                    let read = self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
                        pattern: Arc::clone(quasis),
                        args: Arc::clone(expressions.as_arc()),
                        context: self.template_literal_reduce_context(),
                    });
                    match read.value {
                        QueryResult::Value(built) if !read.result_is_partial() => built,
                        _ => *view,
                    }
                }
                _ => *view,
            })
            .collect();
        let views = views.as_slice();
        let reduced = self.intern_normalized_union_or_intersection(views, true);
        // Arms read as written whose every member the union's reduction
        // keeps: the union stays as written, in its written order.
        if views == arms {
            let kept = match graph.node_data(reduced).as_deref() {
                Some(SemanticNodeData::Union(members)) => {
                    members.len() == arms.len() && arms.iter().all(|arm| members.contains(arm))
                }
                _ => false,
            };
            if kept {
                return None;
            }
        }
        let type_set: Vec<SemanticNodeId> = match graph.node_data(reduced).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown)) => {
                return Some(reduced)
            }
            _ => vec![reduced],
        };
        if !reduction.keep_origin {
            return (reduced != node).then_some(reduced);
        }
        // The checker's union origin: the named unions, when they do not
        // overlap and the reduction kept every member they name.
        let in_named = |member: &SemanticNodeId| {
            named_unions
                .iter()
                .any(|(_, members)| members.contains(member))
        };
        let remaining: Vec<SemanticNodeId> = type_set
            .iter()
            .copied()
            .filter(|member| !in_named(member))
            .collect();
        let named_count: usize = named_unions.iter().map(|(_, members)| members.len()).sum();
        let result = match named_unions.as_slice() {
            [(named, _)] if remaining.is_empty() => *named,
            [] => reduced,
            _ if named_count + remaining.len() == type_set.len() => {
                let origin: Vec<SemanticNodeId> = remaining
                    .into_iter()
                    .chain(named_unions.iter().map(|(named, _)| *named))
                    .collect();
                // The written arms ARE the origin: the union stays as written.
                if origin.len() == arms.len() && origin.iter().all(|arm| arms.contains(arm)) {
                    return None;
                }
                self.intern_normalized_union_or_intersection(&origin, true)
            }
            _ => reduced,
        };
        (result != node).then_some(result)
    }

    /// The checker's TS2590 recovery for an intersection over named
    /// object unions it refuses to distribute
    /// ([`Self::intersection_too_complex`]): the intersection stays factored
    /// while it is read, but its cross product is weighed as the checker
    /// weighs it. `None` when the checker distributes it.
    fn named_intersection_too_complex(
        &self,
        reduction: &CompositeReduction,
    ) -> Option<SemanticNodeId> {
        if reduction.named_object_unions.is_empty() {
            return None;
        }
        self.intersection_too_complex(reduction.node)
    }

    /// Advance `walk` — the members of the resolved union a named arm
    /// stands for, with every member that is itself a named union read as
    /// that union's members, flattened as the checker's union holds them —
    /// with `delivered`, the outcome of the name it was waiting for. `None`
    /// members when a name's demand did not complete, or when the walk
    /// reaches a name on its own active path: a union that contains itself
    /// is the checker's circularity, and it stays deferred.
    fn step_members(
        &self,
        walk: &mut MembersWalk,
        delivered: Option<StructuralFactDemandOutcome>,
    ) -> MembersStep {
        let graph = self.graph();
        if let Some(outcome) = delivered {
            let node = walk
                .awaiting
                .take()
                .expect("an outcome is delivered only to a waiting walk");
            let resolved = match outcome {
                StructuralFactDemandOutcome::Complete(resolved) => resolved,
                StructuralFactDemandOutcome::Recovered { node, reasons } => {
                    walk.partial = Some(walk.partial.map_or(reasons, |held| held.union(reasons)));
                    if !matches!(
                        graph.node_data(node).as_deref(),
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. }))
                    ) {
                        return MembersStep::Done(None);
                    }
                    node
                }
                StructuralFactDemandOutcome::Partial(reasons) => {
                    walk.partial = Some(reasons);
                    return MembersStep::Done(None);
                }
            };
            match graph.node_data(resolved).as_deref() {
                Some(SemanticNodeData::Union(_)) => {
                    walk.active.insert(node);
                    walk.pending.push(MemberStep::Leave(node));
                    walk.pending.push(MemberStep::Read(resolved));
                }
                // A name for a primitive or a literal is that type.
                Some(
                    SemanticNodeData::Primitive(_)
                    | SemanticNodeData::Literal(_)
                    | SemanticNodeData::EnumLiteral(_),
                ) => walk.members.push(resolved),
                _ => walk.members.push(node),
            }
        }
        while let Some(step) = walk.pending.pop() {
            let node = match step {
                MemberStep::Read(node) => node,
                MemberStep::Leave(node) => {
                    walk.active.remove(&node);
                    walk.read.insert(node);
                    continue;
                }
            };
            if let Some(SemanticNodeData::Union(arms)) = graph.node_data(node).as_deref() {
                walk.pending
                    .extend(arms.iter().rev().map(|arm| MemberStep::Read(*arm)));
                continue;
            }
            if !self.is_composite_name(node) {
                walk.members.push(node);
                continue;
            }
            if walk.active.contains(&node) {
                return MembersStep::Done(None);
            }
            if walk.read.contains(&node) {
                continue;
            }
            walk.awaiting = Some(node);
            return MembersStep::Demand(node);
        }
        MembersStep::Done(Some(std::mem::take(&mut walk.members)))
    }

    /// The union `node` with each `keyof` arm read as the keys it settles
    /// to under `context`: the checker resolves `keyof` over a type that is
    /// not generic when it builds the type, so the union holds those keys
    /// (`keyof { a: 1 } | keyof { b: 2 }` is `"a" | "b"`). An arm whose keys
    /// remain open stay as written. `None` when no arm changes, and the
    /// completeness of the arms' evaluations: an evaluation that stopped on
    /// an operational limit makes it partial.
    fn union_over_evaluated_key_arms(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> (Option<SemanticNodeId>, ResultCompleteness) {
        let graph = self.graph();
        let arms = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.members_arc(),
            _ => return (None, ResultCompleteness::Complete),
        };
        let mut changed = false;
        let mut completeness = ResultCompleteness::Complete;
        let views: Vec<SemanticNodeId> = arms
            .iter()
            .map(|&arm| {
                if !matches!(
                    graph.node_data(arm).as_deref(),
                    Some(SemanticNodeData::KeyOf { .. })
                ) {
                    return arm;
                }
                let mut keys = self.evaluate_deferred_outcome(arm, context);
                if let Some(SemanticNodeData::KeyOf { base }) =
                    graph.node_data(keys.node).as_deref()
                {
                    let source = self.normalize_node_for_structural_fact_demand(
                        *base,
                        ProjectionReductionContext::structural_transit(),
                    );
                    if let StructuralFactDemandOutcome::Partial(reasons)
                    | StructuralFactDemandOutcome::Recovered { reasons, .. } = source
                    {
                        completeness = completeness.merge(ResultCompleteness::partial(reasons));
                    }
                    if let StructuralFactDemandOutcome::Complete(source) = source {
                        if matches!(
                            graph.node_data(source).as_deref(),
                            Some(SemanticNodeData::Object(_))
                        ) {
                            let key_domain = graph.intern_preserving_scope(
                                arm,
                                SemanticNodeData::KeyOf { base: source },
                            );
                            let consumed = self.evaluate_deferred_outcome(key_domain, context);
                            keys.completeness = keys.completeness.merge(consumed.completeness);
                            keys.availability = keys.availability.join(consumed.availability);
                            keys.cache_suppress |= consumed.cache_suppress;
                            keys.node = consumed.node;
                        }
                    }
                }
                completeness = completeness.merge(keys.completeness);
                let settled = matches!(keys.completeness, ResultCompleteness::Complete)
                    && !matches!(
                        graph.node_data(keys.node).as_deref(),
                        Some(SemanticNodeData::KeyOf { .. } | SemanticNodeData::Opaque(_))
                    );
                if settled {
                    changed = true;
                    keys.node
                } else {
                    arm
                }
            })
            .collect();
        if completeness.is_partial() {
            // An arm whose keys did not settle for want of work leaves the
            // union unresolved: the demand is partial, its node unused.
            return (None, completeness);
        }
        (
            changed.then(|| self.intern_normalized_union_or_intersection(&views, true)),
            completeness,
        )
    }

    /// Fold a LOCALLY-PRODUCED partial — one no `CacheRead` carried (a step
    /// fuse, an operational limit, a resolution-cycle stop) — into BOTH
    /// propagation channels: the request/cold-compute completeness scope
    /// (exact reason set preserved) AND the active cold-build/relation
    /// taint frame (`result_is_partial = true` + `cache_suppress = true`),
    /// so the enclosing query build refuses warm admission. The universal
    /// read-boundary fold covers `CacheRead`-carried partials only; this is
    /// the matching funnel for evaluator-local ones.
    pub(super) fn fold_local_partial_completeness(&self, reasons: PartialReasonSet) {
        if reasons.contains(PartialReasonSet::OPERATION_BUDGET) {
            self.operation_budget_epoch
                .set(self.operation_budget_epoch.get().wrapping_add(1));
        }
        crate::request_context::fold_result_completeness(ResultCompleteness::partial(reasons));
        self.fold_into_top_build_local_taint_with(true, true, reasons);
    }

    /// The checker's recovery after `refusal`, an operation that exhausted
    /// its own allowance: its error type, with `origin` the operation's
    /// authored form, if one already exists. The recovery is a resource
    /// partial, folded here with it, so no caller can keep the recovery
    /// while dropping its incompleteness.
    pub(super) fn recover_at_operation_budget(
        &self,
        refusal: crate::semantic_query::checker_policy::OperationRefusal,
        origin: Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        self.fold_local_partial_completeness(PartialReasonSet::OPERATION_BUDGET);
        crate::semantic_query::checker_policy::resource_recovery(self.graph(), refusal, origin)
    }
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod forwarding_tests {
    use super::*;
    #[cfg(test)]
    use crate::semantic_query::surface_resolution::NonEmptyReasons;
    #[cfg(test)]
    use crate::semantic_query::{FactInterpretation, FactStatus, SemanticQueryValue};

    /// Exercise the evaluator's actual close and discard boundaries with the
    /// outcome of a host-owned live read, rather than injecting an abort.
    pub fn assert_deferred_close_preserves_execution_validity<
        C: crate::resolver_core::ResolverCapabilities,
    >(
        dispatch: &ProjectSemanticDispatch<'_, C>,
        finalise: FactReadSetFinalise,
        moved: bool,
    ) {
        use crate::semantic_query::ExecutionAbort;

        let graph = dispatch.graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let source = graph.intern_node(SemanticNodeData::Alias(number));
        let context = ProjectionReductionContext::structural_transit();
        assert!(matches!(
            dispatch.normalize_node_for_structural_fact_demand(source, context),
            StructuralFactDemandOutcome::Complete(node) if node == number
        ));
        assert!(
            graph
                .evaluate_deferred_memo_get(
                    source,
                    context,
                    DeferredEvaluationDemand::Shell,
                    dispatch.ctx
                )
                .is_some(),
            "the fixture must exercise an actual warm memo"
        );
        let before = graph.memo_entry_count();
        let unscoped = dispatch.finish_deferred_evaluation_frame(
            DeferredEvaluationFrame::new(
                source,
                context,
                dispatch.operation_budget_epoch.get(),
                DeferredEvaluationDemand::Shell,
            ),
            number,
            finalise.clone(),
        );
        if moved {
            assert!(
                matches!(
                    unscoped.availability,
                    crate::semantic_query::FactStatus::Unavailable(_)
                ),
                "a direct close without a request scope must still reject torn evidence"
            );
            assert!(matches!(
                graph.node_data(unscoped.node).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::ExecutionAborted(
                    ExecutionAbort::Superseded
                )))
            ));
            assert_eq!(graph.memo_entry_count(), before);
        } else {
            assert!(unscoped.availability.is_complete());
        }
        let _validity = crate::request_context::ExecutionValidityScope::enter();
        let before = graph.memo_entry_count();
        let observation = dispatch.observe_read(|| {
            dispatch.finish_deferred_evaluation_frame(
                DeferredEvaluationFrame::new(
                    source,
                    context,
                    dispatch.operation_budget_epoch.get(),
                    DeferredEvaluationDemand::Shell,
                ),
                number,
                finalise,
            )
        });
        let discarded = observation.discard();
        // Demand a separate exact position after declining the probe. It has
        // no dependency on the source whose execution was superseded.
        let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
            base: number,
            path: Arc::from([]),
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        });
        if moved {
            assert!(
                matches!(
                    dispatch.normalize_node_for_structural_fact_demand(source, context),
                    StructuralFactDemandOutcome::Partial(_)
                ),
                "a warmed direct consumer must reject an already aborted execution"
            );
            assert!(
                matches!(discarded, Err(ExecutionAbort::Superseded)),
                "declining quality must not discard a locally detected execution failure"
            );
            assert!(
                matches!(
                    read.value,
                    QueryResult::Error(QueryError::ExecutionAborted(ExecutionAbort::Superseded))
                ),
                "the enclosing demand must carry no value: {read:?}"
            );
            assert_eq!(
                graph.memo_entry_count(),
                before,
                "an execution aborted by a declined probe must admit no candidate"
            );
        } else {
            assert!(discarded.is_ok(), "a stable close must remain discardable");
            assert!(
                matches!(read.value, QueryResult::Value(node) if node == number),
                "{read:?}"
            );
            assert!(read.fact_status.is_complete(), "{read:?}");
        }
    }

    #[test]
    fn a_deferred_forwarder_preserves_presence_only_instead_of_certifying_positions() {
        let graph = crate::semantic_query_memo::SemanticGraphStore::new();
        let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let causes = NonEmptyReasons::new(PartialReasonSet::MISSING_DEPENDENCY).unwrap();
        let outcome = EvaluateDeferredOutcome {
            node,
            completeness: ResultCompleteness::partial(causes.get()),
            availability: FactStatus::Approximate(causes, Some(FactInterpretation::PresenceOnly)),
            cache_suppress: true,
        };
        let output = outcome.forwarded_output(node, Arc::from([]));
        assert_eq!(
            output.approximation,
            Some(FactInterpretation::PresenceOnly),
            "a forwarded presence subset cannot certify completed positions"
        );
        let key = SemanticQueryKey::KeyOf {
            base: node,
            context: ProjectionReductionContext::published(ProjectionMode::Shallow),
        };
        let output: super::super::walk::QueryBuildOutput<SemanticQueryValue> = output.into();
        let (_, status) = super::super::projection_fact::adopt_projection_result(
            &graph,
            &key,
            output.result,
            output.completeness,
            output.approximation,
        );
        assert_eq!(
            status.interpretation(),
            Some(FactInterpretation::PresenceOnly)
        );
        let mut read = CacheRead::from_value_and_signature(1u8, Arc::from([]));
        read.fact_status = status;
        let copied = read.clone().map_value(|value| value + 1);
        assert_eq!(
            copied.fact_status.interpretation(),
            Some(FactInterpretation::PresenceOnly)
        );
        assert_eq!(read.fact_status, copied.fact_status);
    }
}
