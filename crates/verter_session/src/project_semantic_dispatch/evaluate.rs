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
    CacheRead, IndexKey, LiteralValue, PartialReasonSet, ProjectionMode,
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
            verter_debug_assert!(
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
            .or_partial_if(read.result_is_partial, read.partial_reason_classes());
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
        IndexKey::UniqueSymbol(identity) => IndexKey::UniqueSymbol(identity.clone()),
        IndexKey::Computed(node) => IndexKey::Computed(*node),
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
    /// (the callable realizer [`realize_callable_member`](crate::meta_resolve::dispatch_helpers::realize_callable_member)
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
    pub(crate) fn normalize_node_for_structural_fact_demand(
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
    #[cfg(test)]
    pub(crate) fn normalize_node_keeping_declaration_refs_for_tests(
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
        if read.result_is_partial {
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
                arms.iter().all(|arm| self.is_named_reference(*arm))
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
        let read = self.execute_read(SemanticQueryKey::IndexedAccess {
            base: object,
            index: clone_index_key(index),
            mode: ProjectionMode::Navigate,
        });
        if read.result_is_partial {
            return false;
        }
        let QueryResult::Value(value) = read.value else {
            return false;
        };
        self.is_named_reference(value)
    }

    /// Shared residual-carrier resolution loop backing
    /// [`Self::normalize_node_for_structural_fact_demand`] (both residual arms
    /// resolve), [`Self::peel_node_for_uninstantiated_carrier_fact_demand`]
    /// (`instantiate_instantiation_refs = false`) and, under test,
    /// `normalize_node_keeping_declaration_refs_for_tests`
    /// (`resolve_declaration_refs = false`). ONE loop, one resolver — the
    /// entry points differ ONLY in which residual arm is allowed to resolve
    /// (there is no divergent second implementation).
    fn resolve_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        instantiate_instantiation_refs: bool,
        resolve_declaration_refs: bool,
    ) -> StructuralFactDemandOutcome {
        let (_connected_guard, initial_trip) = self.enter_connected_demand(false);
        if let Some(reasons) = initial_trip {
            self.fold_local_partial_completeness(reasons);
            return StructuralFactDemandOutcome::Partial(reasons);
        }
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
                        Some((base.clone(), Some(Arc::clone(args))))
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
                            self.ctx,
                            &read.dep_signature,
                        );
                        completeness = completeness
                            .or_partial_if(read.result_is_partial, read.partial_reason_classes());
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
                SemanticNodeData::ClassExpressionInstance { surface, .. } => *surface,
                // Residual DeclRef → the canonical shallow `ResolveDecl` query
                // (the `ScopeId { canonical_id, local_scope: None }` shape the
                // canonical resolver issues).
                SemanticNodeData::DeclRef { identity } => {
                    let identity = identity.clone();
                    drop(data);
                    let read = self.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&identity.canonical_id),
                            owner: identity.owner,
                            local_scope: None,
                            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                                identity.owner,
                            ),
                        },
                        name: Arc::clone(&identity.decl_name),
                    }));
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self.ctx,
                        &read.dep_signature,
                    );
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, read.partial_reason_classes());
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
                    let slot = self.type_slot_for(
                        Arc::clone(&base.canonical_id),
                        base.owner,
                        Arc::clone(&base.decl_name),
                    );
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
                        .or_partial_if(read.result_is_partial, read.partial_reason_classes());
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
                self.alias_names_settled_type(n)
            };
            if named_by_alias {
                n = named;
            }
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

    /// How the checker prints an application of the named declaration
    /// `identity` — measured on the pinned TypeScript 7.0.2 through the
    /// corpus's two-step wrapper:
    ///
    /// - an interface or class application is printed by its name
    ///   (`GI` with `interface GI<T = number>` prints `GI<number>`);
    /// - an alias whose declared body is a type the alias itself
    ///   constructs — an object, function, mapped, array or tuple type, a
    ///   union or an intersection — is printed by the alias (`Tup<number>`,
    ///   `Fn<number>`);
    /// - an alias whose body references another declaration prints as
    ///   [`Self::printed_alias_reference`] decides;
    /// - every other alias is not named by the alias: a conditional
    ///   (`Cond` prints its selected branch), a bare parameter
    ///   (`type Lit<T> = T`) or a primitive.
    ///
    /// `None` when the declaration's kind or body cannot be recovered.
    fn printed_declaration(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> Option<PrintedDeclaration> {
        self.printed_declaration_within(identity, &mut rustc_hash::FxHashSet::default())
    }

    fn printed_declaration_within(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
        visited: &mut rustc_hash::FxHashSet<crate::semantic_query::DeclIdentity>,
    ) -> Option<PrintedDeclaration> {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        if !visited.insert(identity.clone()) {
            // An alias cycle names nothing.
            return Some(PrintedDeclaration::AliasTransparent);
        }
        match self.prepared_decl_kind(identity)? {
            TypeDeclKind::Interface | TypeDeclKind::Class => {
                return Some(PrintedDeclaration::Named)
            }
            TypeDeclKind::Alias => {}
        }
        let Some(body) = self.declared_alias_body(identity)? else {
            return Some(PrintedDeclaration::AliasTransparent);
        };
        Some(match self.graph().node_data(body).as_deref() {
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
                self.printed_alias_reference(target, &[], visited)
            }
            Some(SemanticNodeData::InstantiationRef { base: target, args }) => {
                self.printed_alias_reference(target, args, visited)
            }
            _ => PrintedDeclaration::AliasTransparent,
        })
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
    fn printed_alias_reference(
        &self,
        target: &crate::semantic_query::DeclIdentity,
        args: &[SemanticNodeId],
        visited: &mut rustc_hash::FxHashSet<crate::semantic_query::DeclIdentity>,
    ) -> PrintedDeclaration {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        if is_builtin(target) {
            return match BuiltinMappedUtility::of(&target.decl_name) {
                Some(BuiltinMappedUtility::Homomorphic) if !self.declared_union(args.first()) => {
                    PrintedDeclaration::AliasThrough
                }
                Some(_) => PrintedDeclaration::AliasNamed,
                None if self.runtime_nominal_identity(target).is_some() => {
                    PrintedDeclaration::AliasNamed
                }
                None => PrintedDeclaration::AliasTransparent,
            };
        }
        let (Some(kind), Some(generic)) = (
            self.prepared_decl_kind(target),
            self.prepared_decl_is_generic(target),
        ) else {
            return PrintedDeclaration::AliasTransparent;
        };
        match kind {
            _ if !generic => PrintedDeclaration::AliasThrough,
            TypeDeclKind::Interface | TypeDeclKind::Class => PrintedDeclaration::AliasNamed,
            TypeDeclKind::Alias if self.alias_declares_homomorphic_mapping(target) => {
                PrintedDeclaration::AliasThrough
            }
            TypeDeclKind::Alias => match self.printed_declaration_within(target, visited) {
                Some(PrintedDeclaration::AliasTransparent) | None => {
                    PrintedDeclaration::AliasTransparent
                }
                Some(_) => PrintedDeclaration::AliasNamed,
            },
        }
    }

    /// Whether the alias `identity` declares a homomorphic mapped type — a
    /// mapping over `keyof` one of its type parameters, a `Partial` /
    /// `Required` / `Readonly` application over a source that is not a
    /// union, or an application of another alias that does. The checker
    /// gives such a declaration the mapped type's own name, so an alias of
    /// it is printed by that name.
    fn alias_declares_homomorphic_mapping(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> bool {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        let mut visited = rustc_hash::FxHashSet::default();
        let mut current = identity.clone();
        loop {
            if !visited.insert(current.clone()) {
                return false;
            }
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
                    .into_complete_node()?;
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
    fn declared_alias_body(
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
                    args: complete,
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
                        let mut read = self.execute_read(SemanticQueryKey::KeyOf {
                            base: child.node,
                            context: frame.context,
                        });
                        // A `keyof` the builder kept as a carrier over a
                        // declaration, an application or a mapped type is
                        // evaluated here, at a demand for its value: its
                        // keys where they settle, as the checker prints
                        // them.
                        if let QueryResult::Value(keys) = read.value {
                            if let Some(settled) = self.settled_key_of_carrier(keys, frame.context)
                            {
                                read.value = QueryResult::Value(settled);
                            }
                        }
                        let fallback = self.opaque(QueryError::Miss);
                        self.deferred_read_action(frame, read, fallback)
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
                                let fallback = self.opaque(QueryError::Miss);
                                self.deferred_read_action(frame, read, fallback)
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
        self.fold_into_top_build_local_taint_with(true, true, reasons);
    }
}
