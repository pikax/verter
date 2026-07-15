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

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    CacheRead, IndexKey, LiteralValue, PartialReasonSet, PathSegment, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, ResolveDeclKey, ResultCompleteness,
    ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

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
/// (a torn / unrootable self-root, a tracer-signature overflow, a `ReturnOnly`
/// cross-owner-reuse admission, a fenced serve) but distinct from a partial
/// result. A `Complete` outcome can still carry `cache_suppress = true`; that
/// signal is NOT reconstructible from the node, so it rides the outcome so
/// [`Self::into_active_query_build_node`] can fold it into the active build
/// frame EVEN on a `Complete` outcome (memo non-admission).
///
/// All fields are PRIVATE to this module: outside `evaluate.rs` the ONLY
/// way to obtain the node is [`Self::into_active_query_build_node`], which
/// folds the completeness AND the suppress bit into the active propagation
/// channels first. A caller cannot read `.node` and drop those signals — the
/// compiler's field-privacy boundary is the fail-closed rail.
#[must_use]
#[derive(Clone, Copy)]
pub(super) struct EvaluateDeferredOutcome {
    node: SemanticNodeId,
    completeness: ResultCompleteness,
    /// OR of every nested read's `cache_suppress` observed while producing
    /// this outcome. Orthogonal to `completeness`: a `Complete` result may be
    /// non-cacheable. See the struct-level docs.
    cache_suppress: bool,
}

impl EvaluateDeferredOutcome {
    /// A complete, cacheable, warm-admissible result.
    fn complete(node: SemanticNodeId) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::Complete,
            cache_suppress: false,
        }
    }

    /// A partial (never-published) carrier-stop result carrying `reasons`.
    fn partial(node: SemanticNodeId, reasons: PartialReasonSet) -> Self {
        Self {
            node,
            completeness: ResultCompleteness::partial(reasons),
            cache_suppress: false,
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
    pub(super) fn into_active_query_build_node(
        self,
        dispatch: &ProjectSemanticDispatch<'_>,
    ) -> SemanticNodeId {
        let is_partial = matches!(self.completeness, ResultCompleteness::Partial(_));
        if is_partial || self.cache_suppress {
            // The frame requirement bites when there is a partial OR a suppress
            // to fold — the sole moments a dropped signal becomes the escape
            // hatch. A frameless pure-`Complete` (nothing to fold) is permitted.
            debug_assert!(
                !dispatch.build_local_taint.borrow().is_empty(),
                "into_active_query_build_node released a Partial or cache-suppressed node with \
                 no active cold-build/relation taint frame: the completeness / suppress signal \
                 would be silently erased. A build-scoped caller must run inside a frame; a \
                 non-build caller must consume the typed StructuralFactDemandOutcome instead."
            );
        }
        match self.completeness {
            ResultCompleteness::Partial(reasons) => {
                // Partial folds BOTH channels (`result_is_partial` +
                // `cache_suppress`) into the frame and the request scope; the
                // suppress bit is subsumed.
                dispatch.fold_local_partial_completeness(reasons);
            }
            ResultCompleteness::Complete => {
                if self.cache_suppress {
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
        self.node
    }
}

/// Heap-owned continuation for one deferred-operator evaluation entry. A
/// frame advances alias/fix-point hops in place and suspends only when the
/// current operator needs an operand evaluated first. No frame is represented
/// by a Rust call frame.
enum DeferredEvaluationStage {
    EvaluateCurrent,
    AwaitKeyOfBase,
    AwaitIndexedObject { index: IndexKey },
    AwaitIndexedIndex { object: SemanticNodeId },
}

struct DeferredEvaluationFrame {
    entry_node: SemanticNodeId,
    node: SemanticNodeId,
    context: ProjectionReductionContext,
    memo_checked: bool,
    visited: rustc_hash::FxHashSet<SemanticNodeId>,
    completeness: ResultCompleteness,
    cache_suppress: bool,
    stage: DeferredEvaluationStage,
}

impl DeferredEvaluationFrame {
    fn new(node: SemanticNodeId, context: ProjectionReductionContext) -> Self {
        let mut visited = rustc_hash::FxHashSet::default();
        visited.insert(node);
        Self {
            entry_node: node,
            node,
            context,
            memo_checked: false,
            visited,
            completeness: ResultCompleteness::Complete,
            cache_suppress: false,
            stage: DeferredEvaluationStage::EvaluateCurrent,
        }
    }

    fn merge_child(&mut self, child: EvaluateDeferredOutcome) {
        self.completeness = self.completeness.merge(child.completeness);
        self.cache_suppress |= child.cache_suppress;
    }

    fn merge_read<T>(&mut self, read: &CacheRead<T>) {
        self.completeness = self
            .completeness
            .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
        self.cache_suppress |= read.cache_suppress;
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
        IndexKey::TypeNode(node) => IndexKey::TypeNode(*node),
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
pub(crate) enum StructuralFactDemandOutcome {
    /// Fully resolved to a terminal structural body OR a stable residual
    /// carrier-stop (no fuse/ceiling/fault fired). The ONLY arm that
    /// yields a node.
    Complete(SemanticNodeId),
    /// Truncated / faulted. Carries the reasons ONLY — no node.
    Partial(PartialReasonSet),
}

impl StructuralFactDemandOutcome {
    /// Fail-closed projection: the resolved node when `Complete`, `None`
    /// when `Partial`. The standard consumer disposition — a partial demand
    /// yields a refusal / conservative fallback, never a classification.
    pub(crate) fn into_complete_node(self) -> Option<SemanticNodeId> {
        match self {
            Self::Complete(node) => Some(node),
            Self::Partial(_) => None,
        }
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
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
                    None => IndexKey::TypeNode(resolved),
                }
            }
            _ => IndexKey::TypeNode(resolved),
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
    pub(crate) fn evaluate_deferred_semantic_node_with_context_for_tests(
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
    /// tracer-overflow / `ReturnOnly` benign-non-cacheability). It is the
    /// admission signal the `evaluate_deferred_memo` publish gate consults
    /// alongside completeness, and it is NOT reconstructible from the node —
    /// tests that assert the suppress-aggregation contract (e.g. a
    /// carrier-subject arm threading a nested read's `cache_suppress`) reach it
    /// ONLY through this shim. Same strict `#[cfg(any(test, feature =
    /// "test-support"))]` gate + no propagation folds as the 2-tuple form.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn evaluate_deferred_outcome_for_tests(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness, bool) {
        let outcome = self.evaluate_deferred_outcome(node, context);
        (outcome.node, outcome.completeness, outcome.cache_suppress)
    }

    /// Demand-point structural-fact normalizer for node-domain fact readers
    /// (e.g. [`CallableNodeView`](crate::meta_resolve::callable_view::CallableNodeView)).
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
    /// (the same keys [`realize_callable_member`](crate::meta_resolve::dispatch_helpers::realize_callable_member)
    /// issues), records their dep-signature facts into the active tracer, and
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
    pub(crate) fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> StructuralFactDemandOutcome {
        // Full structural-fact demand: resolve BOTH residual carriers
        // (`DeclRef` via `ResolveDecl`, `InstantiationRef` via `Instantiate`).
        self.resolve_structural_fact_demand(node, context, true)
    }

    /// Carrier-PRESERVING sibling of
    /// [`Self::normalize_node_for_structural_fact_demand`] for readers that must
    /// reach an UNINSTANTIATED `InstantiationRef` carrier (e.g. the validated
    /// Svelte-snippet positional reader
    /// [`CallableNodeView::validated_snippet_positional_params`](crate::meta_resolve::callable_view::CallableNodeView),
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
    pub(crate) fn peel_node_for_uninstantiated_carrier_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> StructuralFactDemandOutcome {
        // Carrier-preserving peel: resolve `DeclRef` shells but STOP at an
        // `InstantiationRef` (do NOT instantiate — leave the args readable).
        self.resolve_structural_fact_demand(node, context, false)
    }

    /// Shared residual-carrier resolution loop backing both
    /// [`Self::normalize_node_for_structural_fact_demand`]
    /// (`instantiate_instantiation_refs = true`) and
    /// [`Self::peel_node_for_uninstantiated_carrier_fact_demand`]
    /// (`= false`). ONE loop, one resolver — the two entry points differ ONLY by
    /// whether the `InstantiationRef` arm instantiates (there is no divergent
    /// second implementation).
    fn resolve_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        instantiate_instantiation_refs: bool,
    ) -> StructuralFactDemandOutcome {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            self.fold_local_partial_completeness(reasons);
            return StructuralFactDemandOutcome::Partial(reasons);
        }
        // Step 1: evaluate deferred shells (Alias / KeyOf / IndexedAccess /
        // Mapped / Conditional / TemplateLiteral / DeclPlaceholder / bare-import),
        // merging the evaluation's typed completeness into the demand outcome.
        let first = self.evaluate_deferred_outcome(node, context);
        let mut completeness = first.completeness;
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
        // The loop's own exit classification: `None` = a stable (Complete)
        // stop; `Some(reasons)` = an operational truncation/fault.
        let exit_reasons: Option<PartialReasonSet> = loop {
            let Some(data) = self.graph().node_data(n) else {
                // Missing arena data: the demand cannot classify what it
                // cannot read.
                break Some(PartialReasonSet::MISSING_SEMANTIC_NODE_DATA);
            };
            // TERMINAL-BEFORE-FUSE: classify whether `n` is a residual
            // resolvable carrier BEFORE consulting the fuse, so a result that
            // reached its terminal structural body on exactly the last
            // permitted step is a stable stop, never a false partial. The
            // non-residual break also covers the peel's deliberate
            // un-instantiated `InstantiationRef` stop.
            let is_residual = match data.as_ref() {
                SemanticNodeData::DeclRef { .. } => true,
                SemanticNodeData::InstantiationRef { .. } => instantiate_instantiation_refs,
                _ => false,
            };
            if !is_residual {
                break None;
            }
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
                // Residual DeclRef → the canonical shallow `ResolveDecl` query
                // (the same `ScopeId { canonical_id, local_scope: None }` shape
                // `realize_callable_member`'s DeclRef arm issues).
                SemanticNodeData::DeclRef { identity } => {
                    let identity = identity.clone();
                    drop(data);
                    let read = self.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&identity.canonical_id),
                            local_scope: None,
                        },
                        name: Arc::clone(&identity.decl_name),
                    }));
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self.ctx,
                        &read.dep_signature,
                    );
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    match read.value {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(_) => {
                            break Some(PartialReasonSet::SAME_PATH_RECURSION)
                        }
                        QueryResult::Error(err) => break demand_read_fault_reasons(&err),
                    }
                }
                // Residual InstantiationRef → the shared `Instantiate` query
                // (the `relation::record_target_shape` shape generalised): args
                // evaluate carrier-shaped under the caller's context (their
                // completeness merges into the demand outcome), the slot is
                // the base decl's type slot, and the instantiate context derives
                // from the caller's context.
                //
                // The carrier-preserving peel (`instantiate_instantiation_refs
                // == false`) never reaches this arm — `is_residual` classified
                // the un-instantiated `InstantiationRef` as its deliberate
                // stable stop above.
                SemanticNodeData::InstantiationRef { base, args } => {
                    let slot = self
                        .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                    let owner_canonical = Arc::clone(&base.canonical_id);
                    let args: Arc<[SemanticNodeId]> = Arc::from(
                        args.iter()
                            .map(|arg| {
                                let arg_outcome = self.evaluate_deferred_outcome(*arg, context);
                                completeness = completeness.merge(arg_outcome.completeness);
                                arg_outcome.node
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    drop(data);
                    let read = self.execute_read(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(
                            slot,
                            args,
                            self.instantiate_context_for(&owner_canonical, context),
                        ),
                    ));
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self.ctx,
                        &read.dep_signature,
                    );
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
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
            if next.node == n {
                // No progress — a stable fix-point carrier-stop.
                break None;
            }
            n = next.node;
        };
        if let Some(reasons) = exit_reasons {
            completeness = completeness.merge(ResultCompleteness::partial(reasons));
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
                StructuralFactDemandOutcome::Partial(reasons)
            }
        }
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
        miss_fallback: SemanticNodeId,
    ) -> DeferredEvaluationAction {
        frame.merge_read(&read);
        crate::request_context::observe_component_meta_read_suppress(&read);
        match read.value {
            QueryResult::Value(next) => frame.advance_or_finish(next),
            _ => DeferredEvaluationAction::Finish(miss_fallback),
        }
    }

    fn finish_deferred_evaluation_frame(
        &self,
        frame: DeferredEvaluationFrame,
        result: SemanticNodeId,
    ) -> EvaluateDeferredOutcome {
        if let Some(reasons) = self.connected_demand_trip() {
            return EvaluateDeferredOutcome {
                node: frame.entry_node,
                completeness: frame
                    .completeness
                    .merge(ResultCompleteness::partial(reasons)),
                cache_suppress: frame.cache_suppress,
            };
        }
        if !frame.completeness.is_partial() && !frame.cache_suppress {
            self.graph()
                .evaluate_deferred_memo_publish(frame.entry_node, frame.context, result);
        }
        EvaluateDeferredOutcome {
            node: result,
            completeness: frame.completeness,
            cache_suppress: frame.cache_suppress,
        }
    }

    fn evaluate_deferred_outcome(
        &self,
        node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        // A completed cacheable evaluation needs no connected work. Limited
        // outcomes can never reach this memo.
        if let Some(cached) = self
            .graph()
            .evaluate_deferred_memo_get(node, reduction_context)
        {
            return EvaluateDeferredOutcome::complete(cached);
        }

        // Continuations live on the heap. Authored structural depth has no
        // cap here; only connected operational limits can stop evaluation.
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            return EvaluateDeferredOutcome::partial(node, reasons);
        }
        let mut frames = vec![DeferredEvaluationFrame::new(node, reduction_context)];
        let mut completed_child: Option<EvaluateDeferredOutcome> = None;

        loop {
            if let Err(reasons) = self.charge_connected_work() {
                return aborted_evaluation_outcome(&frames, completed_child.as_ref(), reasons);
            }

            let action = if let Some(child) = completed_child.take() {
                let frame = frames
                    .last_mut()
                    .expect("a completed operand requires a suspended parent");
                frame.merge_child(child);
                match std::mem::replace(&mut frame.stage, DeferredEvaluationStage::EvaluateCurrent)
                {
                    DeferredEvaluationStage::AwaitKeyOfBase => {
                        let read = self.execute_read(SemanticQueryKey::KeyOf {
                            base: child.node,
                            context: frame.context,
                        });
                        let fallback = self.opaque(QueryError::Miss);
                        self.deferred_read_action(frame, read, fallback)
                    }
                    DeferredEvaluationStage::AwaitIndexedObject { index } => match index {
                        IndexKey::TypeNode(index_node) => {
                            frame.stage =
                                DeferredEvaluationStage::AwaitIndexedIndex { object: child.node };
                            DeferredEvaluationAction::Push {
                                node: index_node,
                                context: ProjectionReductionContext::published(
                                    ProjectionMode::Expanded,
                                ),
                            }
                        }
                        index => {
                            let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                                base: child.node,
                                index,
                                mode: frame.context.mode,
                            });
                            let fallback = self.opaque(QueryError::Miss);
                            self.deferred_read_action(frame, read, fallback)
                        }
                    },
                    DeferredEvaluationStage::AwaitIndexedIndex { object } => {
                        let index = self.normalized_index_key_from_evaluated_node(child.node);
                        let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                            base: object,
                            index,
                            mode: frame.context.mode,
                        });
                        let fallback = self.opaque(QueryError::Miss);
                        self.deferred_read_action(frame, read, fallback)
                    }
                    DeferredEvaluationStage::EvaluateCurrent => {
                        unreachable!("only a suspended frame can receive an operand")
                    }
                }
            } else {
                let (entry_node, context, check_memo) = {
                    let frame = frames
                        .last_mut()
                        .expect("the evaluator retains a root frame");
                    let check_memo = !frame.memo_checked;
                    frame.memo_checked = true;
                    (frame.entry_node, frame.context, check_memo)
                };
                let memo_hit = if check_memo {
                    self.graph().evaluate_deferred_memo_get(entry_node, context)
                } else {
                    None
                };
                if let Some(cached) = memo_hit {
                    DeferredEvaluationAction::Cached(cached)
                } else {
                    let current = frames
                        .last()
                        .expect("the evaluator retains a root frame")
                        .node;
                    if let Some(data) = self.graph().node_data(current) {
                        match data.as_ref() {
                            SemanticNodeData::Alias(target) => frames
                                .last_mut()
                                .expect("active evaluator frame")
                                .advance_or_finish(*target),
                            SemanticNodeData::KeyOf { base } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.stage = DeferredEvaluationStage::AwaitKeyOfBase;
                                DeferredEvaluationAction::Push {
                                    node: *base,
                                    context: frame.context,
                                }
                            }
                            SemanticNodeData::IndexedAccess { object, index } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.stage = DeferredEvaluationStage::AwaitIndexedObject {
                                    index: clone_index_key(index),
                                };
                                DeferredEvaluationAction::Push {
                                    node: *object,
                                    context: frame.context.with_mode(ProjectionMode::Navigate),
                                }
                            }
                            SemanticNodeData::Mapped { source, mapper } => {
                                let frame = frames.last_mut().expect("active evaluator frame");
                                let read = self.execute_read(SemanticQueryKey::MappedType {
                                    source: *source,
                                    mapper: mapper.clone(),
                                    context: frame.context,
                                });
                                let fallback = self.opaque(QueryError::Miss);
                                self.deferred_read_action(frame, read, fallback)
                            }
                            SemanticNodeData::TypeOf(_) => {
                                let (value_root, path) =
                                    data.typeof_head().expect("TypeOf carrier head");
                                let type_args: Vec<SemanticNodeId> =
                                    data.carrier_type_args().to_vec();
                                let context =
                                    frames.last().expect("active evaluator frame").context;
                                let read = self
                                    .execute_read(self.typeof_key_for(value_root.clone(), context));
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.merge_read(&read);
                                crate::request_context::observe_component_meta_read_suppress(&read);
                                match read.value {
                                    QueryResult::Value(root) => {
                                        let projected = if path.is_empty() {
                                            Some(root)
                                        } else {
                                            let projection_path: Arc<[PathSegment]> = Arc::from(
                                                path.iter()
                                                    .map(|segment| {
                                                        PathSegment::Member(Arc::clone(segment))
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .into_boxed_slice(),
                                            );
                                            let read =
                                                self.execute_read(SemanticQueryKey::ProjectPath {
                                                    base: root,
                                                    path: projection_path,
                                                    context: ProjectionReductionContext::published(
                                                        ProjectionMode::Navigate,
                                                    ),
                                                });
                                            let frame =
                                                frames.last_mut().expect("active evaluator frame");
                                            frame.merge_read(&read);
                                            crate::request_context::observe_component_meta_read_suppress(
                                                &read,
                                            );
                                            match read.value {
                                                QueryResult::Value(id) => Some(id),
                                                _ => None,
                                            }
                                        };
                                        match projected {
                                            Some(projected) => {
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
                                            None => DeferredEvaluationAction::Finish(
                                                self.opaque(QueryError::Miss),
                                            ),
                                        }
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
                            } => {
                                let read = self.execute_read(SemanticQueryKey::Conditional {
                                    check: *check,
                                    extends: *extends,
                                    true_branch: *true_branch_ref,
                                    false_branch: *false_branch_ref,
                                    distributive: *distributive,
                                });
                                let frame = frames.last_mut().expect("active evaluator frame");
                                let fallback = self.opaque(QueryError::Miss);
                                self.deferred_read_action(frame, read, fallback)
                            }
                            SemanticNodeData::TemplateLiteral {
                                quasis,
                                expressions,
                            } => {
                                let read =
                                    self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
                                        pattern: Arc::clone(quasis),
                                        args: Arc::clone(expressions),
                                        context: self.template_literal_reduce_context(),
                                    });
                                let frame = frames.last_mut().expect("active evaluator frame");
                                self.deferred_read_action(frame, read, current)
                            }
                            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                                canonical_id,
                                name,
                                whole_hash: _,
                            }) => {
                                let base =
                                    self.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
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
                                let fallback = self.opaque(QueryError::Miss);
                                self.deferred_read_action(frame, read, fallback)
                            }
                            SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {
                                let (resolved, observed) = self
                                    .resolve_carrier_subject_node_capturing_suppress(
                                        current,
                                        ProjectionReductionContext::structural_transit_with_mode(
                                            ProjectionMode::Navigate,
                                        ),
                                    );
                                let frame = frames.last_mut().expect("active evaluator frame");
                                frame.completeness = frame.completeness.or_partial_if(
                                    observed.result_is_partial,
                                    PartialReasonSet::PROPAGATED,
                                );
                                frame.cache_suppress |= observed.cache_suppress;
                                frame.advance_or_finish(resolved)
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

            match action {
                DeferredEvaluationAction::Continue => {}
                DeferredEvaluationAction::Push { node, context } => {
                    frames.push(DeferredEvaluationFrame::new(node, context));
                }
                DeferredEvaluationAction::Finish(result) => {
                    let frame = frames.pop().expect("finishing an active evaluator frame");
                    let outcome = self.finish_deferred_evaluation_frame(frame, result);
                    if frames.is_empty() {
                        return outcome;
                    }
                    completed_child = Some(outcome);
                }
                DeferredEvaluationAction::Cached(cached) => {
                    frames
                        .pop()
                        .expect("a memo hit belongs to an active evaluator frame");
                    let outcome = EvaluateDeferredOutcome::complete(cached);
                    if frames.is_empty() {
                        return outcome;
                    }
                    completed_child = Some(outcome);
                }
            }
        }
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
        crate::request_context::fold_result_completeness(ResultCompleteness::partial(reasons));
        self.fold_into_top_build_local_taint(true, true);
    }
}
