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

use std::cell::Cell;
use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    IndexKey, LiteralValue, PartialReasonSet, PathSegment, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, ResolveDeclKey, ResultCompleteness,
    ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// Hard ceiling on recursive `evaluate_deferred_semantic_node_with_context`
/// depth. The evaluator's fix-point walk usually terminates within a few
/// hops (Alias → KeyOf → IndexedAccess → leaf). Pathological mapped-type
/// patterns that combine generic instantiation with nested conditionals
/// over keyspace-derived literals (e.g.
/// `ChatMessagesSlots<T>`'s per-K loop) can fan out unboundedly through
/// the operator re-dispatch chain. The cap prevents the per-K loop from
/// running away even when the substitute hash-cons + evaluate memo
/// cannot collapse the work (every K's substituted body produces a new
/// node id whose recursive evaluator chain re-enters).
///
/// On exhaustion the evaluator returns the current node — a structural-
/// transit carrier-stop equivalent. The downstream materialiser's
/// Opaque-fallback contract keeps the surface addressable by re-dispatch
/// without leaking torn / partial results into shared caches.
///
/// **Sizing rationale.** Default thread stacks on Windows are 1 MB and
/// the evaluator frame is large (~30-50 KB of locals + temporaries with
/// debug info). 256 stack frames at 50 KB = 12 MB, which fits within
/// the workspace-test `RUST_MIN_STACK=134_217_728` (128 MB) budget while
/// staying well under the OS default thread-stack limit's worst case.
/// The ceiling is high enough to never fire on well-formed corpus
/// components (Theme / ChatMessage / Table all run within a few dozen
/// recursive entries); it fires on the pathological tail (e.g.
/// `ChatMessagesSlots<T>`'s thousand-recursive-entry chain) where work
/// would otherwise grow exponentially.
const EVALUATE_DEFERRED_DEPTH_CEILING: u32 = 256;

/// Strict step fuse for the residual-carrier resolution loop in
/// [`ProjectSemanticDispatch::normalize_node_for_structural_fact_demand`] and
/// the signature-source carrier rail
/// (`ProjectSemanticDispatch::resolve_signature_source_carrier`). The
/// `visited` set already guarantees termination on a finite graph; this is the
/// secondary bound against fresh-node regrowth (each resolution step can mint
/// a NEW node id the visited set has never seen). Real carrier chains (a
/// `DeclRef` to an alias whose body is an `InstantiationRef` whose body is a
/// `Union`) resolve in a handful of hops, far under this bound. A trip is
/// TYPED: the demand outcome carries
/// [`PartialReasonSet::STRUCTURAL_FACT_DEMAND_LIMIT`] — never a silent
/// carrier-stop a consumer could classify as a stable result.
pub(super) const STRUCTURAL_FACT_DEMAND_FUSE: u32 = 64;

thread_local! {
    /// Per-thread recursive-depth counter for
    /// `evaluate_deferred_semantic_node_with_context`. The counter is
    /// bumped at the entry of every (potentially recursive) call and
    /// decremented at exit via an RAII guard. When the counter
    /// crosses [`EVALUATE_DEFERRED_DEPTH_CEILING`] the inner call
    /// returns the carrier-stop sentinel (the input `node` itself)
    /// instead of recursing further.
    static EVALUATE_DEFERRED_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Per-thread sticky truncation flag. Set when the depth guard
    /// fires (any recursive frame on the current thread's evaluator
    /// call chain hit `over_ceiling`). Every publish site consults
    /// this flag and SKIPS publishing into
    /// `evaluate_deferred_memo` when it is set — a parent frame that
    /// consumed a truncated child's input-node carrier must not
    /// publish its derived result as a warm entry, because that
    /// result is budget-tainted (downstream operators reduced
    /// against a carrier-stop, not a fully-resolved sub-evaluation).
    ///
    /// The flag is reset to `false` only when the top-level entry
    /// frame (depth was 0 at enter) exits — every nested frame
    /// preserves the sticky state up the call chain. This routes
    /// budget-exhaustion through `ComputeAdmission::ReturnOnly`
    /// semantics at the evaluator layer: no torn / budget-exhausted
    /// result ever populates the shared memo.
    static EVALUATE_DEFERRED_TRUNCATED: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard for [`EVALUATE_DEFERRED_DEPTH`]. Increments on `enter`,
/// decrements on `Drop`. The guard's `over_ceiling` flag captures
/// whether the entry exceeded the ceiling — callers consult it to
/// fast-return without recursing.
///
/// The guard's `is_top_level` flag records whether the depth at
/// `enter` was 0 — only the top-level frame may reset the sticky
/// `EVALUATE_DEFERRED_TRUNCATED` flag on `Drop`, so a nested
/// recursive frame's exit never clears a child's truncation signal
/// before the parent has a chance to honour it.
struct DepthGuard {
    over_ceiling: bool,
    is_top_level: bool,
}

impl DepthGuard {
    fn enter() -> Self {
        EVALUATE_DEFERRED_DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth.saturating_add(1));
            let over_ceiling = depth >= EVALUATE_DEFERRED_DEPTH_CEILING;
            if over_ceiling {
                EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.set(true));
            }
            DepthGuard {
                over_ceiling,
                is_top_level: depth == 0,
            }
        })
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        EVALUATE_DEFERRED_DEPTH.with(|cell| {
            let depth = cell.get();
            cell.set(depth.saturating_sub(1));
        });
        if self.is_top_level {
            EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.set(false));
        }
    }
}

/// Returns `true` iff the depth guard has fired anywhere on the
/// current thread's evaluator call chain since the top-level entry.
/// Publish sites consult this flag to gate writes into
/// `evaluate_deferred_memo` — a `true` reading means at least one
/// recursive sub-evaluation returned its input-node carrier-stop,
/// and any derived result the current frame produced is
/// budget-tainted.
fn evaluator_truncated() -> bool {
    EVALUATE_DEFERRED_TRUNCATED.with(|flag| flag.get())
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
/// [`PartialReasonSet::PROPAGATED`]), the recursion ceiling fired
/// ([`PartialReasonSet::DEFERRED_EVALUATION_LIMIT`]), OR a recursive
/// sub-evaluation was itself partial (its exact reasons merge through).
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
        match self.graph().node_data(resolved).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => (
                IndexKey::String(Arc::from(text.as_str())),
                completeness,
                cache_suppress,
            ),
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                // Bounded integer-convention fold: `IndexKey::Number`
                // admits ONLY literals whose i64 `Display` IS the
                // canonical `js_number_to_string` spelling (the single
                // shared producer predicate —
                // `build::integer_convention_index_key`). Everything
                // else stays `TypeNode` for the walker's G4.5
                // canonical-needle recovery.
                let key = match super::build::integer_convention_index_key(*number) {
                    Some(integer) => IndexKey::Number(integer),
                    None => IndexKey::TypeNode(resolved),
                };
                (key, completeness, cache_suppress)
            }
            Some(SemanticNodeData::Alias(target)) => {
                let (key, inner_completeness, inner_suppress) =
                    self.normalized_index_key_node_outcome(*target);
                (
                    key,
                    completeness.merge(inner_completeness),
                    cache_suppress || inner_suppress,
                )
            }
            _ => (IndexKey::TypeNode(resolved), completeness, cache_suppress),
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
    /// by a `visited` set plus [`STRUCTURAL_FACT_DEMAND_FUSE`] and is FAIL-CLOSED:
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
    /// resolution was operationally truncated or faulted (step fuse, recursion
    /// ceiling, cycle, budget, unstable state, non-`Miss` fault, missing arena
    /// data, or a partial nested read). A consumer classifies ONLY a `Complete`
    /// node.
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
    /// Bounded (`visited` + [`STRUCTURAL_FACT_DEMAND_FUSE`]) and fail-closed:
    /// returns the typed [`StructuralFactDemandOutcome`] — the peel's
    /// deliberate `InstantiationRef` stop and an honest miss / stable
    /// no-progress are `Complete`, while a cycle, fuse/ceiling trip, fault,
    /// or partial nested read is `Partial(reasons)` with NO node. It is
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
        let mut steps: u32 = 0;
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
            if steps >= STRUCTURAL_FACT_DEMAND_FUSE {
                // Step-fuse trip: the chain kept minting fresh residual
                // carriers past the per-demand bound — the reached node is an
                // INTERMEDIATE carrier, not a stable stop.
                break Some(PartialReasonSet::STRUCTURAL_FACT_DEMAND_LIMIT);
            }
            if !visited.insert(n) {
                // Residual-carrier cycle (`type MutA = MutB; type MutB = MutA`):
                // the chain can never settle.
                break Some(PartialReasonSet::SAME_PATH_RECURSION);
            }
            steps += 1;
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
    /// `Complete` (and `!evaluator_truncated()`). The completeness
    /// accumulator merges every nested `execute_read`'s `result_is_partial`
    /// (the boolean bridge, lifted as [`PartialReasonSet::PROPAGATED`]) and
    /// every recursive sub-evaluation's typed completeness (exact reasons
    /// preserved; the recursion ceiling contributes
    /// [`PartialReasonSet::DEFERRED_EVALUATION_LIMIT`]), so a
    /// budget-/recursion-/fatal-tainted result is withheld REGARDLESS of
    /// whether a `RequestContext` is installed — closing the
    /// no-`RequestContext` (`audit Noop`) hole where the request-global
    /// suppress sticky reads `false`. The request sticky
    /// (`current_request_result_is_partial`) is NOT the admission
    /// authority here; `observe_component_meta_read_suppress` is retained
    /// PURELY to propagate the same partiality to the request /
    /// cold-compute scope (the component-meta / materialize warm gates).
    fn evaluate_deferred_outcome(
        &self,
        mut node: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
    ) -> EvaluateDeferredOutcome {
        // Hash-cons memo. The store-owned `evaluate_deferred_memo`
        // collapses repeated `(node, context)` evaluations across
        // call sites. The evaluator's fix-point walk is a pure
        // function of `(node, context)` because every recursive call
        // and operator re-dispatch consumes the SAME context and
        // routes through `execute_cooperative` (which is itself
        // content-keyed). The dominant per-K mapped-type loop win:
        // a K-independent subtree like `MessageBase<T>` embedded
        // inside a K-dependent value expression evaluates once per
        // K through the recursive walk pre-memo; post-memo every
        // subsequent visit collapses to one `DashMap::get`.
        let entry_node = node;
        if let Some(cached) = self
            .graph()
            .evaluate_deferred_memo_get(entry_node, reduction_context)
        {
            // A memo hit is COMPLETE **and cacheable** by construction: the
            // publish gate below admits an entry only when it is neither
            // `Partial` NOR `cache_suppress`, so a hit carries
            // `result_is_partial = false` AND needs no suppress replay.
            // Reconstructing `complete(cached)` (suppress = false) is therefore
            // faithful — it can never drop a non-cacheability restriction,
            // because a suppressed result was never admitted in the first place.
            return EvaluateDeferredOutcome::complete(cached);
        }
        // Cooperative fail-fast rail: pathological mapped-type per-K
        // loops (e.g. `ChatMessagesSlots<T>` whose per-K body produces
        // a new substituted node id per K, defeating the entry-node
        // memo) can drive `evaluate_deferred_*` into arbitrarily deep
        // recursive operator re-dispatch. The TLS depth guard caps
        // recursion at `EVALUATE_DEFERRED_DEPTH_CEILING`; on
        // exhaustion we return the input node (carrier-stop) WITHOUT
        // publishing into the memo (no torn / budget-exhausted results
        // ever populate shared caches — `ComputeAdmission::ReturnOnly`
        // policy applied at this layer).
        let _depth_guard = DepthGuard::enter();
        if _depth_guard.over_ceiling {
            // Depth-truncated carrier-stop is a partial result, typed with
            // the recursion-ceiling reason.
            return EvaluateDeferredOutcome::partial(
                entry_node,
                PartialReasonSet::DEFERRED_EVALUATION_LIMIT,
            );
        }
        let mut visited = rustc_hash::FxHashSet::default();
        visited.insert(node);
        // Entry-scoped completeness accumulator: merges every nested read's
        // `result_is_partial` (the boolean bridge, lifted as `PROPAGATED`)
        // and every recursive sub-evaluation's typed completeness. This is
        // the admission authority for the shared memo below.
        let mut completeness = ResultCompleteness::Complete;
        // Entry-scoped `cache_suppress` accumulator: OR of every nested read's
        // `cache_suppress` (benign inner-memo non-cacheability — a fenced
        // serve, a torn/unrootable self-root, a tracer overflow, a `ReturnOnly`
        // reuse) and every recursive sub-evaluation's suppress bit. Orthogonal
        // to `completeness`: a `Complete` result may still be non-cacheable.
        // Rides the returned outcome so `into_active_query_build_node` can fold
        // it into the active frame EVEN on a `Complete` outcome — the signal is
        // NOT reconstructible from the node.
        let mut cache_suppress = false;
        let result = loop {
            let Some(data) = self.graph().node_data(node) else {
                // Missing arena data: this node id resolves to NO semantic data,
                // so the `Opaque(Miss)` carrier interned below is FABRICATED, not
                // an honest classification. Mark the evaluation
                // `Partial(MISSING_SEMANTIC_NODE_DATA)` so the fabricated carrier
                // can never surface `Complete` — DISTINCT from an honest
                // `QueryError::Miss` on a nested read (which stays `Complete`: an
                // unresolved authored name is a valid semantic `Unknown`). This
                // fires on the entry node OR a target reached through an `Alias`
                // hop (`Alias(target)` advances `node = target`, and the next
                // iteration reads `target`'s absent data here).
                completeness = completeness.merge(ResultCompleteness::partial(
                    PartialReasonSet::MISSING_SEMANTIC_NODE_DATA,
                ));
                break self.opaque(QueryError::Miss);
            };
            let next = match data.as_ref() {
                SemanticNodeData::Alias(target) => *target,
                SemanticNodeData::KeyOf { base } => {
                    let base_outcome = self.evaluate_deferred_outcome(*base, reduction_context);
                    completeness = completeness.merge(base_outcome.completeness);
                    cache_suppress |= base_outcome.cache_suppress;
                    let read = self.execute_read(SemanticQueryKey::KeyOf {
                        base: base_outcome.node,
                        context: reduction_context,
                    });
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    // Two-signal fold: the deferred-shell evaluator returns a
                    // bare node (hash-cons memoised), so it folds a genuinely
                    // incomplete nested read onto the request's sticky partial
                    // flag — the component-meta / materialize warm gates
                    // consult that flag and refuse a partial-tainted result.
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::IndexedAccess { object, index } => {
                    // Path-precision rule: the deferred `T[K]` shell is
                    // a single indexed-access hop whose OBJECT is the
                    // intermediate base and whose projection of `K` is
                    // the TERMINAL segment. The object recursion demotes
                    // to `Navigate` so the intermediate surface stays
                    // shallow (its sibling members — `$emit` / `$slots` /
                    // unrelated props — are NOT eagerly expanded when the
                    // caller demanded `Expanded`), while the terminal
                    // single-hop projection runs in the CALLER's mode so
                    // a demanded `Expanded` terminal resolves its carrier.
                    let object_outcome = self.evaluate_deferred_outcome(
                        *object,
                        reduction_context.with_mode(ProjectionMode::Navigate),
                    );
                    completeness = completeness.merge(object_outcome.completeness);
                    cache_suppress |= object_outcome.cache_suppress;
                    let index = match index {
                        IndexKey::String(text) => IndexKey::String(Arc::clone(text)),
                        IndexKey::Number(number) => IndexKey::Number(*number),
                        IndexKey::TypeNode(node) => {
                            let (key, key_completeness, key_suppress) =
                                self.normalized_index_key_node_outcome(*node);
                            completeness = completeness.merge(key_completeness);
                            cache_suppress |= key_suppress;
                            key
                        }
                    };
                    let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                        base: object_outcome.node,
                        index,
                        mode: reduction_context.mode,
                    });
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    let read = self.execute_read(SemanticQueryKey::MappedType {
                        source: *source,
                        mapper: mapper.clone(),
                        context: reduction_context,
                    });
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::TypeOf(_) => {
                    // `typeof value.path<args>`: resolve the value root through
                    // the single typeof query, PROJECT the carrier's dotted
                    // path, THEN apply the carrier's instantiation `type_args`
                    // to the projected signature (resolve → project → apply,
                    // mirroring the eager lowering order). The enclosing
                    // evaluation's demand rides the key — operator recursion
                    // never widens a deferred typeof carrier past the caller's
                    // mode.
                    let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                    // Read the carrier args from the SAME borrow (owned copy so
                    // the borrow is not held across the apply call).
                    let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
                    let read = self
                        .execute_read(self.typeof_key_for(value_root.clone(), reduction_context));
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    let root = match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    };
                    let projected = if path.is_empty() {
                        root
                    } else {
                        let projection_path: Arc<[PathSegment]> = Arc::from(
                            path.iter()
                                .map(|segment| PathSegment::Member(Arc::clone(segment)))
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        );
                        let read = self.execute_read(SemanticQueryKey::ProjectPath {
                            base: root,
                            path: projection_path,
                            context: crate::semantic_query::ProjectionReductionContext::published(
                                ProjectionMode::Navigate,
                            ),
                        });
                        completeness = completeness
                            .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                        cache_suppress |= read.cache_suppress;
                        crate::request_context::observe_component_meta_read_suppress(&read);
                        match read.value {
                            QueryResult::Value(id) => id,
                            _ => break self.opaque(QueryError::Miss),
                        }
                    };
                    // Instantiation expression (`typeof C.make<string>`): apply
                    // the lowered type arguments to the projected generic
                    // signature. An arity/shape mismatch composes an honest
                    // `Opaque(Miss)` from `apply_typeof_instantiation_args`
                    // AFTER the projection.
                    if type_args.is_empty() {
                        projected
                    } else {
                        self.apply_typeof_instantiation_args(projected, &type_args)
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
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                } => {
                    // Route the carrier through the ONE shared
                    // `TemplateLiteralReduce` query producer (no inline fold).
                    // The producer computes the cartesian product over the
                    // finite literal-union choices of every interpolated
                    // expression (`` `cell:${"name"|"count"}` `` ⇒
                    // `"cell:name" | "cell:count"`), folds an all-single-literal
                    // template to one `Literal`, returns `never` for an empty
                    // product, and carrier-stops to this same hash-consed shell
                    // (so `next == node` and the loop terminates) when any
                    // expression is non-finite. A finite-but-over-cap product
                    // carrier-stops with `result_is_partial`, which the
                    // accumulator below folds so the partial is never
                    // warm-admitted. Dispatching here is what makes
                    // `TemplateLiteralReduce` appear in the request trace.
                    let pattern = Arc::clone(quasis);
                    let args = Arc::clone(expressions);
                    drop(data);
                    let read = self.execute_read(SemanticQueryKey::TemplateLiteralReduce {
                        pattern,
                        args,
                        context: self.template_literal_reduce_context(),
                    });
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break node,
                    }
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    name,
                    whole_hash: _,
                }) => {
                    let base = self.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
                    let owner_canonical = Arc::clone(canonical_id);
                    drop(data);
                    // Demand-driven reducer spec: the
                    // declaration-placeholder unwrap inherits the
                    // caller's reduction context. The implicit
                    // `Published + Expanded` unwrap was the path that
                    // re-opened nested `keyof` / `Mapped` reification
                    // during relation-engine binding; the caller's
                    // `StructuralTransit` context now carries through.
                    let read = self.execute_read(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(
                            base,
                            Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                            self.instantiate_context_for(&owner_canonical, reduction_context),
                        ),
                    ));
                    completeness = completeness
                        .or_partial_if(read.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= read.cache_suppress;
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    match read.value {
                        QueryResult::Value(id) => id,
                        _ => break self.opaque(QueryError::Miss),
                    }
                }
                // `BareRef` / `ImportType` are UNRESOLVED-reference carriers
                // produced by the query-free macro hot mirror — a `Pick<C, K>`
                // source `C`, a mapped-type source, etc. arrive here as a bare
                // `BareRef` carrier rather than a pre-resolved `DeclRef`.
                // Re-enter the SHARED carrier-subject normalization
                // (`resolve_carrier_subject_node`) — the SAME hop the
                // path-walker (walk.rs) runs for a mid-walk carrier and the
                // canonical query entry runs for a subject carrier — under
                // `StructuralTransit(Navigate)` so operator heads resolve enough
                // to reach the real declaration while the body stays
                // carrier-preserving. This is the one-hop reference resolution,
                // NOT the over-evaluation the `DeclRef` / `InstantiationRef`
                // carriers warned about below: a bare/import reference carries
                // no symbolic-form preservation policy. A reference that does not
                // resolve (normalization returns it unchanged) breaks to the
                // input node.
                SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {
                    drop(data);
                    // Carrier-subject normalization performs nested reads (the
                    // `ImportType` qualified-path `ProjectPath`, the builtin
                    // `Instantiate`, the `TypeOf` value-root/path reads) whose
                    // `cache_suppress` / `result_is_partial` MUST aggregate into
                    // THIS evaluation's accumulators: the `evaluate_deferred_memo`
                    // publish gate is a SEPARATE admission authority from any
                    // enclosing build frame, so a suppressed / partial nested
                    // carrier read would otherwise slip past it (the coupled
                    // no-poison hole with the publish gate below). The capturing
                    // variant OR-folds every nested read structurally at the shared
                    // read boundary and hands the aggregate back here — mirroring
                    // the `KeyOf` / `Mapped` / `IndexedAccess` arms that OR their
                    // `CacheRead` rails.
                    let (resolved, observed) = self
                        .resolve_carrier_subject_node_capturing_suppress(
                            node,
                            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                                ProjectionMode::Navigate,
                            ),
                        );
                    completeness = completeness
                        .or_partial_if(observed.result_is_partial, PartialReasonSet::PROPAGATED);
                    cache_suppress |= observed.cache_suppress;
                    if resolved == node {
                        break node;
                    }
                    resolved
                }
                // `DeclRef`/`InstantiationRef`
                // arms are deliberately NOT added to the deferred-
                // shell evaluator. The path-walker (walk.rs) and the
                // keyspace enumerator (enumerate.rs) handle these
                // carriers under explicit demand contexts, but the
                // deferred-shell evaluator is called from intermediate
                // IndexedAccess hops where eagerly resolving a
                // `DeclRef` would over-evaluate symbolic forms that
                // the slot-binding indexed-access preservation policy
                // expects to stay carrier-shaped (e.g.
                // `AppProps['avatar']` must stay
                // `IndexedAccess { object: DeclRef(AppProps), index }`).
                // The brief's conditional "if they are on the
                // macro-shape enumeration path" scopes this symmetry
                // to the enumeration path only.
                _ => break node,
            };
            if next == node {
                break node;
            }
            // Cyclic re-entry detected (self-referential evaluation) — return
            // current node as fix-point per guard contract row.
            if !visited.insert(next) {
                break node;
            }
            node = next;
        };
        // Depth-truncation carrier-stop. If the depth guard fired anywhere on
        // the current call chain (even in a transitive sub-evaluation), every
        // result along the unwinding path is budget-tainted. A parent frame
        // that consumed a truncated child's input-node carrier may have
        // dispatched downstream operators that returned `Opaque(Miss)` or a
        // partial reduction; admitting that derivative into the warm memo
        // would let a stale, budget-exhausted answer survive across queries.
        // Return the ENTRY node (a structural-transit carrier-stop) rather
        // than the partially-reduced `result`, so downstream re-dispatch
        // starts from the same surface the cache would observe on a future
        // cold call (Opaque-fallback contract applied consistently across the
        // call chain), and propagate partial so the caller withholds too.
        if evaluator_truncated() {
            return EvaluateDeferredOutcome {
                node: entry_node,
                completeness: completeness.merge(ResultCompleteness::partial(
                    PartialReasonSet::DEFERRED_EVALUATION_LIMIT,
                )),
                cache_suppress,
            };
        }
        // Entry-scoped no-poison admission gate (`ComputeAdmission::ReturnOnly`).
        // Publish ONLY when THIS evaluated entry is BOTH complete AND cacheable:
        //   - the completeness accumulator merged every nested read's
        //     `result_is_partial` (as `PROPAGATED`) and every recursive
        //     sub-evaluation's typed completeness, so a
        //     budget-/recursion-/fatal-tainted result is withheld here
        //     independent of any `RequestContext`; AND
        //   - the `cache_suppress` accumulator OR-ed every nested read's
        //     `cache_suppress` — a fenced serve, a torn / unrootable self-root,
        //     a tracer-signature overflow, a `ReturnOnly` cross-owner reuse:
        //     a perfectly VALID Complete result that is merely not
        //     memo-publishable. The documented `CacheRead::cache_suppress`
        //     contract is explicit — "the memo refuses insertion when this is
        //     true" — and it aggregates via OR through nested queries. Without
        //     this half, a Complete-with-suppress entry would publish and a
        //     later warm hit would reconstruct `complete(cached)` WITHOUT its
        //     non-cacheability, letting a fenced / torn / `ReturnOnly`-derived
        //     result replay and permit an enclosing warm admission — the exact
        //     no-poison hole. Withholding here is strictly fail-closed: the
        //     value still flows back `Complete` to the caller (below); only the
        //     memo insertion is skipped, so the next demand recomputes. It
        //     never makes a finite type `Partial`.
        // The request sticky propagation (`observe_component_meta_read_suppress`
        // at the arms above) feeds the request / cold-compute warm gates, but
        // the admission authority for THIS shared memo is the evaluated entry's
        // OWN completeness AND suppress.
        if !completeness.is_partial() && !cache_suppress {
            // Publish the entry-node → result mapping. Concurrent
            // publishers for the same `(entry_node, context)` resolve to
            // structurally identical results (the evaluator is pure on
            // those two inputs), so first-writer-wins is correct.
            self.graph()
                .evaluate_deferred_memo_publish(entry_node, reduction_context, result);
        }
        EvaluateDeferredOutcome {
            node: result,
            completeness,
            cache_suppress,
        }
    }

    /// Fold a LOCALLY-PRODUCED partial — one no `CacheRead` carried (a step
    /// fuse, a recursion ceiling, a resolution-cycle stop) — into BOTH
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
