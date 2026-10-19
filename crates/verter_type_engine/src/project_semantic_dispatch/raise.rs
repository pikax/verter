//! `raise_node_to_type_expr` — SemanticNodeId → TypeExpr structural raising
//!
//! Reverse of [`shallow_lower_type_expr`](super::lower::shallow_lower_type_expr).
//! Walks one structural level of a [`SemanticNodeData`] graph payload back
//! into a [`TypeExpr`] tree. No name resolution, no generic substitution,
//! no conditional branch selection — those are the [`PathWalker`](super::walk)'s
//! job. Cycle protection via a per-call `active` visited set.
//!
//! **Authority contract:** this is the *only* `SemanticNodeId →
//! TypeExpr` lowering path in the workspace. Pair with
//! [`shallow_lower_type_expr`](super::lower::shallow_lower_type_expr)
//! (forward direction). The invariant test
//! `semantic_node_to_type_expr_has_exactly_one_path` asserts exactly one
//! `fn raise_node_to_type_expr` exists in `crates/`.

use std::sync::Arc;

use crate::semantic_query::SurfaceView;
use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

// Re-export the sealed output carriers so the existing
// `crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr`
// reference path the per-member publication projectors thread stays
// stable. The TYPES are the single canonical sealed carriers defined in
// `super::output_materialization`; this is a name re-export of the one
// owner type (NOT a second definition / dual path).
pub use super::output_materialization::{MaterializedOutputTypeExpr, OutputTypeExpr};
use super::ProjectSemanticDispatch;
use crate::instant::Instant;
use crate::semantic_query::{
    DepSignature, IndexKey, MapperKey, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, ReductionDemand, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember, TupleElement,
};

/// The owner-layer shape engine: the ONE private generic `SemanticNodeData`
/// fold and its three algebras (materialization vs node-domain facts/key vs
/// `&TypeExpr` folding). Owns the single exhaustive traversal so the
/// materialization and the node-domain facts/key cannot drift.
pub mod shape_engine;

/// Key-domain openness and closedness: the L1 carrier-stop predicates and
/// the fact-native closedness proofs, evaluated on one explicit work stack.
mod openness;
use openness::observe_closedness_walk_consult;
pub use openness::{
    builtin_lowering_argument_is_open, mapped_type_is_open_or_unknown,
    mapped_type_key_domain_is_open_or_unknown, prepared_decl_body_is_closed,
    prepared_instantiation_key_domain_is_closed, userland_instantiation_body_is_closed_object,
    utility_enumeration_domain_is_open_or_unknown, ClosednessVerdict, KeyDomainBinding,
    ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
};
pub(in crate::project_semantic_dispatch) use openness::{
    deref_slot_body, is_l1_object_filter_utility, ClosednessChain, ClosednessProofTable,
};
#[cfg(any(test, feature = "test-support"))]
pub use shape_engine::folded_nodes_for_tests;

// Crate-wide re-export of the ONE semantic-primitive-kind → `PrimitiveName`
// conversion so fact producers (the macro-output expansion sink's leaf-fact
// projection) reuse the shape engine's mapping instead of duplicating it.
pub(crate) use shape_engine::semantic_primitive_to_primitive_name;
pub use shape_engine::RaisedShallowMemberOutput;
// The materialize-fold degradation sidecar vocabulary — the sealed output
// payload (sibling `output_materialization`) carries it.
#[cfg(any(test, feature = "test-support"))]
pub use shape_engine::fold_to_type_expr;
pub use shape_engine::{DegradedLeaf, MaterializedTypeExpr};

/// Graph-native result of reducing one output demand before the terminal sink
/// performs its single display materialization.
pub struct ReducedOutputNode {
    _roots: crate::semantic_query_memo::node_roots::IdentityNodeRoots,
    node_id: SemanticNodeId,
    dep_signature: DepSignature,
    result_is_partial: bool,
}

impl ReducedOutputNode {
    #[must_use]
    pub fn node_id(&self) -> SemanticNodeId {
        self.node_id
    }

    #[must_use]
    pub fn result_is_partial(&self) -> bool {
        self.result_is_partial
    }

    /// Borrow the reduced node this output owns.
    pub fn node(
        &self,
    ) -> Result<
        crate::semantic_query_memo::node_roots::NodeRef<'_>,
        crate::semantic_query::QueryError,
    > {
        self._roots.0.node_ref(self.node_id)
    }
}

// Dispatch-wide re-export of the shape engine's failed-query-carrier
// classification (the SAME `Opaque` variant mapping its materialize /
// node-domain algebras apply), so raise boundaries fail closed on a
// projection landing on an `Unknown`-materializing failure node without
// duplicating the variant split.
pub(in crate::project_semantic_dispatch) use shape_engine::node_is_unknown_materializing_failure;

// Test-only re-export of the interned shape key so the raised-shape parity
// suite can assert (type-level) that it is an interned id, NOT a `TypeExpr`
// owner.
#[cfg(any(test, feature = "test-support"))]
pub use shape_engine::RaisedShapeKey;

// =====================================================================
// dispatch trace plumbing for cycle-BFS unit tests.
//
// `enable_dispatch_trace_for_test` returns a guard that clears the trace
// on construction and on drop. `execute_read` pushes a static-string
// discriminant for every key it processes. The discriminant index is
// stable (depends only on the variant name), so tests can assert exact
// counts of `Instantiate`, `ResolveDecl`, etc. dispatches.
//
// Plumbing is `#[cfg(test)]`-only: zero footprint outside test builds.
// =====================================================================

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    pub static DISPATCH_TRACE: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code, reason = "consumed by the dispatch-count assertions")]
pub struct DispatchTraceGuard;

#[cfg(any(test, feature = "test-support"))]
impl Drop for DispatchTraceGuard {
    fn drop(&mut self) {
        DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
    }
}

#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code, reason = "consumed by the dispatch-count assertions")]
pub fn enable_dispatch_trace_for_test() -> DispatchTraceGuard {
    DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
    DispatchTraceGuard
}

// =====================================================================
// Per-key cold and warm dispatch counters (diagnostic only).
//
// Count how many times each `SemanticQueryKey` is built cold and served
// warm, keyed by a (variant_discriminant, content_hash)
// digest. Diagnostic-only, `#[cfg(test)]`-gated, zero footprint outside
// test builds.
//
// F18 (r2 review): variant identity uses std::mem::discriminant so the
// digest does NOT need to be updated when adds new
// SemanticQueryKey variants. The discriminant is opaque but stable
// per-variant; pairs with the hash for full key identity.
// =====================================================================

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct SemanticQueryKeyDigest {
    pub variant: std::mem::Discriminant<crate::semantic_query::SemanticQueryKey>,
    pub hash: u64,
}

#[cfg(any(test, feature = "test-support"))]
impl SemanticQueryKeyDigest {
    fn from_key(key: &crate::semantic_query::SemanticQueryKey) -> Self {
        use std::hash::{Hash, Hasher};
        // Supplement §5.D.0 r17 — canonicalise the key BEFORE
        // hashing so probes via the caller's key shape (e.g.
        // `ProjectMember`) hit the same digest the warm cache stores
        // (e.g. `ProjectPath` with a length-1 path). Without this,
        // tests probing `family_cold(&original_key)` always read 0
        // because the warm cache stores the post-canonical form.
        let canonical = canonicalise_for_digest(key);
        let mut hasher = rustc_hash::FxHasher::default();
        canonical.hash(&mut hasher);
        Self {
            variant: std::mem::discriminant(canonical.as_ref()),
            hash: hasher.finish(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
fn canonicalise_for_digest(
    key: &crate::semantic_query::SemanticQueryKey,
) -> std::borrow::Cow<'_, crate::semantic_query::SemanticQueryKey> {
    use std::borrow::Cow;

    use crate::semantic_query::{PathSegment, SemanticQueryKey};
    match key {
        SemanticQueryKey::ProjectMember { base, member, mode } => {
            Cow::Owned(SemanticQueryKey::ProjectPath {
                base: *base,
                path: verter_type_expr::OwnedSlice::from(
                    vec![PathSegment::Member(
                        crate::semantic_query::PropertyKey::identifier(member.clone()),
                    )]
                    .into_boxed_slice(),
                ),
                context: crate::semantic_query::ProjectionReductionContext::published(*mode),
            })
        }
        SemanticQueryKey::IndexedAccess { base, index, mode } => {
            Cow::Owned(SemanticQueryKey::ProjectPath {
                base: *base,
                path: verter_type_expr::OwnedSlice::from(
                    vec![PathSegment::Index(index.clone())].into_boxed_slice(),
                ),
                context: crate::semantic_query::ProjectionReductionContext::published(*mode),
            })
        }
        other => Cow::Borrowed(other),
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Supplement §5.D.0 r17 — per-key COLD dispatch count.
    /// Incremented when `execute_read` enters with no warm cache entry
    /// for the key (cache miss; `build` is invoked).
    pub(crate) static DISPATCH_KEY_COLD_COUNTS:
        std::cell::RefCell<rustc_hash::FxHashMap<SemanticQueryKeyDigest, u32>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    /// Supplement §5.D.0 r17 — per-key WARM dispatch count.
    /// Incremented when `execute_read` enters with the key already in
    /// the warm cache (cache hit; `build` is NOT invoked).
    pub(crate) static DISPATCH_KEY_WARM_COUNTS:
        std::cell::RefCell<rustc_hash::FxHashMap<SemanticQueryKeyDigest, u32>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

/// Supplement §5.D.0 r17 — record a COLD dispatch entry for
/// this key (cache miss, `build` will be invoked).
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn record_dispatch_cold(key: &crate::semantic_query::SemanticQueryKey) {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_COLD_COUNTS.with(|c| {
        *c.borrow_mut().entry(digest).or_insert(0) += 1;
    });
}

/// Supplement §5.D.0 r17 — record a WARM dispatch entry for
/// this key (cache hit, returning the memoized value).
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn record_dispatch_warm(key: &crate::semantic_query::SemanticQueryKey) {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_WARM_COUNTS.with(|c| {
        *c.borrow_mut().entry(digest).or_insert(0) += 1;
    });
}

/// Supplement §5.D.0 r17 — read the COLD count for `key`.
/// Returns 0 if the key has not been dispatched on this thread since
/// thread start. The counter is monotonic; tests sample baselines and
/// deltas across paired queries.
#[cfg(any(test, feature = "test-support"))]
pub fn dispatch_cold_for(key: &crate::semantic_query::SemanticQueryKey) -> usize {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_COLD_COUNTS.with(|c| c.borrow().get(&digest).copied().unwrap_or(0) as usize)
}

/// Supplement §5.D.0 r17 — read the WARM count for `key`.
#[cfg(any(test, feature = "test-support"))]
pub fn dispatch_warm_for(key: &crate::semantic_query::SemanticQueryKey) -> usize {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_WARM_COUNTS.with(|c| c.borrow().get(&digest).copied().unwrap_or(0) as usize)
}

/// The number of keys this thread has built cold more than once since
/// thread start: each is a read a stored entry should have served.
#[cfg(any(test, feature = "test-support"))]
pub fn keys_built_cold_more_than_once() -> usize {
    DISPATCH_KEY_COLD_COUNTS.with(|c| c.borrow().values().filter(|count| **count > 1).count())
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn query_key_discriminant(key: &SemanticQueryKey) -> &'static str {
    match key {
        SemanticQueryKey::ResolveDecl(_) => "ResolveDecl",
        SemanticQueryKey::Instantiate(_) => "Instantiate",
        SemanticQueryKey::ProjectMember { .. } => "ProjectMember",
        SemanticQueryKey::IndexedAccess { .. } => "IndexedAccess",
        SemanticQueryKey::KeyOf { .. } => "KeyOf",
        SemanticQueryKey::MappedType { .. } => "MappedType",
        SemanticQueryKey::Conditional { .. } => "Conditional",
        SemanticQueryKey::TypeOf { .. } => "TypeOf",
        SemanticQueryKey::ReduceUnion { .. } => "ReduceUnion",
        SemanticQueryKey::ReduceIntersection { .. } => "ReduceIntersection",
        SemanticQueryKey::ProjectObjectSpread { .. } => "ProjectObjectSpread",
        SemanticQueryKey::ProjectPath { .. } => "ProjectPath",
        SemanticQueryKey::Relate { .. } => "Relate",
        SemanticQueryKey::ResolveMacroPayload { .. } => "ResolveMacroPayload",
        SemanticQueryKey::ResolveClassSurface { .. } => "ResolveClassSurface",
        SemanticQueryKey::ResolveAmbientNamespace { .. } => "ResolveAmbientNamespace",
        SemanticQueryKey::ResolveEnum { .. } => "ResolveEnum",
        SemanticQueryKey::ResolveOverloadSet { .. } => "ResolveOverloadSet",
        SemanticQueryKey::ClassifyBroadRuntime { .. } => "ClassifyBroadRuntime",
        SemanticQueryKey::ApparentType { .. } => "ApparentType",
        SemanticQueryKey::TemplateLiteralReduce { .. } => "TemplateLiteralReduce",
        SemanticQueryKey::FlowNarrowingAt { .. } => "FlowNarrowingAt",
        SemanticQueryKey::ContextualTypeAt { .. } => "ContextualTypeAt",
        SemanticQueryKey::LowerLocator { .. } => "LowerLocator",
        SemanticQueryKey::ClassifyMaterializationCycleGate(_) => "ClassifyMaterializationCycleGate",
        SemanticQueryKey::FlowReturn(_) => "FlowReturn",
        SemanticQueryKey::ResolveCall(_) => "ResolveCall",
        SemanticQueryKey::ClassifyTruthinessDomain { .. } => "ClassifyTruthinessDomain",
        SemanticQueryKey::AwaitedNormalize { .. } => "AwaitedNormalize",
        SemanticQueryKey::AsyncReturnPayload { .. } => "AsyncReturnPayload",
        SemanticQueryKey::SignaturesOfType { .. } => "SignaturesOfType",
        SemanticQueryKey::ReadSignatureResult(_) => "ReadSignatureResult",
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Raise a [`SemanticNodeId`] back to a [`TypeExpr`].
    ///
    /// Pure structural conversion: walks the graph payload one structural
    /// level at a time, raising sub-ids recursively. Cycle protection via
    /// a per-call `active` visited set. Returns `None` when the node is
    /// not present in the host's graph store.
    ///
    /// Use this when you have a `SemanticNodeId` (typically the result of
    /// a [`SemanticQueryApi::execute`] call or a graph-native helper) and
    /// need a [`TypeExpr`] for downstream payload construction. Operator-
    /// shape reduction (`IndexedAccess`, `Conditional`, `Mapped`,
    /// `KeyOf`, `TypeOf`) is the responsibility of the caller — typically
    /// [`Self::materialize_reduced_output_type_expr`]. This function alone
    /// is shell-only.
    pub fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<MaterializedTypeExpr> {
        let mut active = FxHashSet::default();
        // The shell-only materialization entry: delegate to the shared shape
        // engine's `MaterializeTypeExprAlg` — the SOLE exhaustive
        // `SemanticNodeData -> TypeExpr` traversal (owner-local in
        // [`shape_engine`]). The materialization and the node-domain facts/key
        // share that ONE fold, so they cannot drift — anti-drift is structural.
        // This stays MODULE-PRIVATE; out-of-module callers reach it only through
        // the authority-gated output seam ([`Self::output_shell_raise_sealed`]).
        shape_engine::fold_to_type_expr(self, node, &mut active)
    }

    /// Plain shell-only output-materialisation boundary: project a
    /// [`SemanticNodeId`] back to a compat [`TypeExpr`] WITHOUT operator-shape
    /// reduction.
    ///
    /// This is the output/compat seam for callers that already hold the exact
    /// graph node they want to materialise (typically the demanded terminal of
    /// a navigate-driven walk) and want it raised AS-IS. It is a thin wrapper
    /// over the shell-only [`Self::raise_node_to_type_expr`] primitive, so every
    /// carrier round-trips here (raw-fallback text → `TypeExpr::Unknown(UnknownValue)`, the
    /// synthetic binding, the constructor carrier, the `RecursiveRef` back-edge
    /// via `Opaque(QueryError::RecursiveRef)`); tuple-element rest fidelity
    /// rides on the `Tuple` arm's `TupleElement.rest`.
    ///
    /// Operator-shape REDUCTION (`IndexedAccess` / `Conditional` / `Mapped`
    /// / `KeyOf` / `TypeOf`) is NOT performed here — an operator node raises to
    /// its operator `TypeExpr` un-reduced. Callers that need the operator
    /// collapsed must go through [`Self::materialize_reduced_output_type_expr`].
    ///
    /// The `Option` return is the miss signal: `None` means the requested node —
    /// or a node required while raising it — is unavailable/unraisable from the
    /// live graph store. It is deliberately NOT folded into an `Unknown`-sentinel
    /// here so callers can map a miss to whatever their own output contract
    /// requires.
    // Raise-side SHELL seam (no operator reduction). `pub(super)` — the
    // output authority's `materialize_output_type_expr` boundary method (in
    // `super::engine_resources`) calls this. The
    // raw `raise_node_to_type_expr` primitive stays MODULE-PRIVATE; this is
    // the only out-of-module shell-raise reach.
    //
    // It hands back a SEALED [`OutputTypeExpr`] carrier, NEVER a bare
    // [`TypeExpr`]. That is the structural fence: a `project_semantic_dispatch`
    // SIBLING module (e.g. `mod.rs`, `evaluate.rs`) can reach this `pub(super)`
    // seam, but the value it gets back is a sealed carrier it CANNOT unwrap
    // (the inner `TypeExpr` is authority-gated via
    // [`OutputTypeExpr::into_type_expr`], and a sibling can neither mint nor
    // recover the output authority). So no `pub(super)` seam ever yields a raw
    // `TypeExpr` to a holder without the authority — closing the bare-delegator
    // laundering hole. `None` is the miss signal.
    pub(super) fn output_shell_raise_sealed(
        &self,
        node: SemanticNodeId,
    ) -> Option<super::output_materialization::OutputTypeExpr> {
        self.raise_node_to_type_expr(node).map(|materialized| {
            super::output_materialization::OutputTypeExpr::from_raise(self, materialized)
        })
    }

    /// `execute` variant that returns the full [`CacheRead`].
    ///
    /// `ProjectSemanticDispatch::execute` (the [`SemanticQueryApi`] trait
    /// method) discards the dep-signature half of the cache read; this
    /// variant keeps it so callers like [`Self::raise_and_reduce_with_context`]
    /// can accumulate dep facts across nested dispatches and merge them into
    /// the session-layer `fact_versions`. This is the dispatch entry the
    /// cold-build subtree reducer and the operator sub-reductions
    /// (`ProjectPath` / `ReduceIntersection` / macro-payload
    /// intersection normalisation) ride so their dependency facts are not
    /// dropped — the dep-signature-preserving peer of the `SemanticQueryApi`
    /// trait's `execute`.
    pub fn execute_read(
        &self,
        key: SemanticQueryKey,
    ) -> crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>> {
        // Trace the variant for cycle-BFS unit tests.
        // Records the variant before key canonicalisation so the
        // observed call shape matches the caller's intent (sugar
        // variants are recorded as the caller wrote them).
        #[cfg(any(test, feature = "test-support"))]
        DISPATCH_TRACE.with(|t| t.borrow_mut().push(query_key_discriminant(&key)));

        // Delegate to the shared cold-build helper. The helper handles
        // canonicalisation, sentinel construction, the tracer-wrapped
        // build closure, and the warm-hit fast path inside
        // `execute_cooperative`. Routing both `execute` and
        // `execute_read` through one helper ensures fact-tracer
        // installation never bypasses any cold-build path.
        super::narrow_value_cache_read(self.execute_via_cold_build_helper(key))
    }

    // The operand forcing boundary is its only caller. Armed under
    // `cfg(test)`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn execute_read_with_operand_evidence(
        &self,
        key: SemanticQueryKey,
    ) -> (
        crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>,
        Option<crate::semantic_query::operand::SemanticOperandEvidence>,
    ) {
        #[cfg(any(test, feature = "test-support"))]
        DISPATCH_TRACE.with(|t| t.borrow_mut().push(query_key_discriminant(&key)));

        let mut evidence = None;
        let read = super::narrow_value_cache_read(
            self.execute_via_cold_build_helper_capturing_operand_evidence(key, &mut evidence),
        );
        (read, evidence)
    }

    /// Context-explicit reduce-then-raise reducer (demand-driven reducer
    /// spec).
    ///
    /// The caller supplies the publication
    /// [`ProjectionReductionContext`] that flows into every operator
    /// dispatch and propagates through child traversal selection.
    /// `Published(Expanded)` keeps whole-surface behaviour;
    /// `Published(Navigate)` is the per-prop publication boundary that
    /// stops at the demanded terminal without breadth-enumerating
    /// composite members or inactive conditional branches.
    // reduce-then-raise orchestrator; the projection-output boundary
    // (materialize_reduced_output_type_expr) wraps it, and the production
    // per-member projectors (field_types.rs) reach it through that boundary.
    pub fn raise_and_reduce_with_context(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> MaterializedOutputTypeExpr {
        let _scope = self.graph().enter_node_root_scope();
        let reduced = self.reduce_output_node_with_context(node, context);
        // A raise failure after a successful reduction is PRESENT-BUT-
        // UNRAISABLE, never a genuine absence: degrade as the TYPED
        // unmaterialized `Miss` carrier — the sidecar carries the leaf, the
        // payload goes PARTIAL at the `from_parts` choke point (never
        // admitted complete), and the compat tree projects the `semanticMiss`
        // spelling.
        let type_expr = self
            .raise_node_to_type_expr(reduced.node_id)
            .unwrap_or_else(|| MaterializedTypeExpr::degraded(QueryError::Miss));
        MaterializedOutputTypeExpr::from_parts(
            Some(reduced.node_id),
            OutputTypeExpr::from_raise(self, type_expr),
            reduced.dep_signature,
            reduced.result_is_partial,
        )
    }

    /// Reduce one output demand entirely in the semantic-node domain. A true
    /// output sink can inspect node-domain shape facts and then shell-raise the
    /// returned node exactly once for display/publication.
    pub fn reduce_output_node_with_context(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> ReducedOutputNode {
        let _scope = self.graph().enter_node_root_scope();
        let mut state = ReduceState::default();
        let node_id = self.reduce_graph_node_iterative(node, context, &mut state);
        let result_is_partial = state.result_is_partial;
        ReducedOutputNode {
            _roots: crate::semantic_query_memo::node_roots::IdentityNodeRoots::in_graph(
                self.graph(),
                [node_id],
            ),
            node_id,
            dep_signature: state.into_dep_signature(),
            result_is_partial,
        }
    }

    /// Consumer-OBSERVATION variant of [`Self::raise_and_reduce_with_context`]:
    /// the demand walk a consumer performs on a PUBLISHED
    /// `SemanticTypeSource` (a registry entry / prop source read).
    ///
    /// Identical reduce machinery — same arms, same dispatch routing, no
    /// second engine — with the observation carrier fence armed (see
    /// [`ReduceState::observation`]): the demand ROOT always executes (an
    /// authored generic-alias body runs the existing `Instantiate` query, so
    /// outer type arguments substitute), while INTERIOR owner-local helper
    /// references and package-backed references stop as shallow `Ref`
    /// carriers the consumer re-resolves on demand. Workspace-imported
    /// interior branches still expand.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn raise_and_reduce_observation(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        owner_canonical: &str,
    ) -> MaterializedOutputTypeExpr {
        let mut state = ReduceState {
            observation: Some((Arc::from(owner_canonical), node)),
            ..ReduceState::default()
        };
        let reduced = self.reduce_graph_node_iterative(node, context, &mut state);
        let type_expr = self
            .raise_node_to_type_expr(reduced)
            .unwrap_or_else(|| MaterializedTypeExpr::degraded(QueryError::Miss));
        let result_is_partial = state.result_is_partial;
        MaterializedOutputTypeExpr::from_parts(
            Some(reduced),
            OutputTypeExpr::from_raise(self, type_expr),
            state.into_dep_signature(),
            result_is_partial,
        )
    }

    /// Whether the observation carrier fence stops `node` as a shallow
    /// INTERIOR carrier (see [`ReduceState::observation`]). Never stops the
    /// demand root (the outer carrier always executes so the published
    /// declaration's own substitution runs), never stops builtin-utility
    /// applications, and is inert (`false`) on every non-observation reduce
    /// pass.
    fn observation_interior_carrier_stop(
        &self,
        node: SemanticNodeId,
        decl_canonical: &str,
        state: &ReduceState,
    ) -> bool {
        let Some((owner_canonical, root)) = state.observation.as_ref() else {
            return false;
        };
        if node == *root || decl_canonical == "__builtin__" {
            return false;
        }
        decl_canonical == owner_canonical.as_ref()
            || self
                .ctx
                .workspace_is_package_backed(decl_canonical.as_ref())
    }

    /// Top-down demand-driven graph reducer
    /// (demand-driven reducer spec — stack-safe).
    ///
    /// Replaces the legacy bottom-up topological reducer. The
    /// pre-walk visited the ENTIRE reachable subgraph and then reduced
    /// every visited node — that meant `keyof T` / `{ [K in S]: V }`
    /// inside non-selected conditional branches, generic arguments, or
    /// mapped value bodies dispatched their own `KeyOf` /
    /// `MappedType` keys under `Published(Expanded)`, reifying
    /// `outputSchema` / `execute` per-member edges that no caller asked
    /// for. The demand-driven design treats `Published` as "this exact
    /// node is the demanded terminal of the current step": composite
    /// children, inactive branches, and operator-operand subgraphs are
    /// only descended into when the publication boundary demands them.
    ///
    /// Iterative work-stack with two frame kinds:
    ///
    /// - `Descend(node, context)` — entry frame. Marks
    ///   `(node, context)` visited and pushes a `Reduce(node, context)`
    ///   frame followed by the children selected for that node + context
    ///   (see [`Self::push_demand_children`]). Children are pushed AFTER
    ///   the Reduce frame so they pop FIRST — the LIFO stack acts as a
    ///   post-order traversal.
    /// - `Reduce(node, context)` — closure frame. All selected children
    ///   have been reduced into `state.mapping`; invoke
    ///   `reduce_one(node, context, state)` to produce this node's
    ///   reduction.
    ///
    /// `visited` and `mapping` are keyed by
    /// `(SemanticNodeId, ProjectionReductionContext)` so a
    /// `StructuralTransit` reduction does not collide with a
    /// `Published` reduction on the same node — they are distinct
    /// evaluations.
    ///
    /// Stack-safe for arbitrarily deep acyclic structures (≥5000
    /// levels; verified by stack-safety regression fixtures in §4.1).
    // Driven by `raise_and_reduce_with_context` above; reached in production
    // through the reduce-then-raise output boundary
    // (materialize_reduced_output_type_expr) from the per-member projectors.
    fn reduce_graph_node_iterative(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — count every iterative-reduction
        // entry. One `raise_and_reduce_with_context` produces exactly one of
        // these.
        crate::loop5_instrumentation::RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Loop-8 instrumentation — wall-clock attribution for the
        // iterative-reduction body. Calls counter already bumped
        // above (Loop 5); this guard adds NS-only on drop.
        let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new_ns_only(
            &crate::loop5_instrumentation::REDUCE_GRAPH_NODE_ITERATIVE_NS,
        );

        self.reduce_subtree(root, root_context, state)
    }

    /// Reduce the subtree rooted at `root` under `context` using the
    /// top-down demand-driven work stack. Shares `state.visited` /
    /// `state.mapping` with the caller so cycles are broken once and a
    /// dispatch-produced new node can re-enter the reducer without
    /// re-traversing already-resolved subgraphs.
    ///
    /// Each `(node, context)` pair reduces at most once per call to
    /// [`Self::raise_and_reduce_with_context`] — the visited set
    /// deduplicates entry.
    fn reduce_subtree(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        if let Some(&already) = state.mapping.get(&(root, root_context)) {
            return already;
        }
        let mut stack: Vec<ReduceFrame> = Vec::with_capacity(8);
        stack.push(ReduceFrame {
            node: root,
            context: root_context,
            kind: ReduceFrameKind::Descend,
        });
        while let Some(frame) = stack.pop() {
            match frame.kind {
                ReduceFrameKind::Descend => {
                    if !state.visited.insert((frame.node, frame.context)) {
                        // Already in-progress or completed under this
                        // context. Mapping carries the reduction once
                        // it lands; cyclic re-entry returns the raw
                        // node via the rebuild_* fall-through.
                        continue;
                    }
                    let Some(data) = super::node_data_for(self.graph(), frame.node) else {
                        // No graph data — record a self-identity
                        // reduction so callers reading `mapping`
                        // get the raw node.
                        state
                            .mapping
                            .insert((frame.node, frame.context), frame.node);
                        continue;
                    };
                    // Reduce frame is pushed FIRST so it pops AFTER
                    // children (LIFO post-order).
                    stack.push(ReduceFrame {
                        node: frame.node,
                        context: frame.context,
                        kind: ReduceFrameKind::Reduce,
                    });
                    self.push_demand_children(&data, frame.context, &mut stack);
                }
                ReduceFrameKind::Reduce => {
                    let reduced = self.reduce_one(frame.node, frame.context, state);
                    state.mapping.insert((frame.node, frame.context), reduced);
                }
            }
        }
        state
            .mapping
            .get(&(root, root_context))
            .copied()
            .unwrap_or(root)
    }

    /// Push child frames for `data` onto `stack` according to the
    /// demand-driven traversal rules.
    ///
    /// The rules are:
    ///
    /// - Aliases (`Alias`) push their target with the SAME context —
    ///   aliases are semantically transparent and inherit the caller's
    ///   publication demand.
    /// - Operator operands (`KeyOf.base`, `Mapped.source`,
    ///   `Conditional.check`/`extends`, `IndexedAccess.index` typed
    ///   nodes) push as `StructuralTransit`. Their nested operators
    ///   carrier-stop under [`super::may_reduce_operator`], so they
    ///   contribute their structural shape without reifying their own
    ///   members.
    /// - `IndexedAccess.object` pushes as `Published(Navigate)` under
    ///   any `Published` parent (`Foo['a']['b']` walks the path
    ///   navigate-only at intermediate hops; the terminal hop carries
    ///   the caller's mode through the dispatch). Under
    ///   `StructuralTransit` parents, the object pushes as
    ///   `StructuralTransit` (no path materialisation needed for a
    ///   transit walk).
    /// - `Conditional` branches are NOT pushed. The conditional
    ///   dispatch returns the selected branch as its result; that
    ///   branch is then reduced inline via
    ///   [`Self::dispatch_operator_with_recurse`]. Inactive branches
    ///   are never visited — this is the leak fix.
    /// - `Mapped.value_expr` / `Mapped.name_remap` / `Mapped.parameter_node`
    ///   are NOT pushed. The `MappedType` dispatch substitutes the
    ///   binder and evaluates per-key internally under
    ///   `StructuralTransit` (see `build.rs:1817` / `1852`).
    /// - `TypeOf` carries `value_root` + path segments — no semantic
    ///   children to descend.
    /// - Composite shapes (`Object` members, `Union` /
    ///   `Intersection` arms, `Tuple` elements, `Array` element,
    ///   `Function` params / return / type-param constraints/defaults)
    ///   are pushed ONLY under whole-surface `Published(Expanded)`.
    ///   Per the spec, per-prop `Published(Navigate)` /
    ///   `Published(Shallow)` and `StructuralTransit` callers do NOT
    ///   traverse composite children — the parent IS the demand
    ///   terminal. This is the structural leak fix: a per-member
    ///   publication that resolves to an Object stays shallow at the
    ///   object surface.
    /// - `InstantiationRef.args` push under the same context the
    ///   carrier reduces under (args become substituted into the
    ///   instantiated body; their demand follows the body's demand).
    /// - Terminals (`Primitive`, `Literal`, `TypeParam`, `Opaque`,
    ///   `Infer`, `TemplateLiteral`, `DeclRef`)
    ///   have no semantic operand children for the iterative reducer
    ///   to pre-resolve.
    fn push_demand_children(
        &self,
        data: &SemanticNodeData,
        parent_context: ProjectionReductionContext,
        stack: &mut Vec<ReduceFrame>,
    ) {
        match data {
            // Reduced by the owning family, never pre-resolved as a demand
            // child here.
            SemanticNodeData::IntrinsicApplication { .. }
            | SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::TypeOf(_)
            // The nominal terminal self-resolves; the reducer returns it
            // unchanged and there is no operand child to pre-resolve.
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::DeclRef { .. }
            // Unresolved bare-name / dynamic-import / raw-fallback /
            // synthetic-binding carriers are resolved as a whole by the
            // dispatch, not rebuilt by this reducer, so they expose no
            // operand children to pre-resolve here.
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::DeferredCallable(_)
            | SemanticNodeData::SyntheticBinding { .. } => {}
            SemanticNodeData::Alias(target) => {
                stack.push(ReduceFrame::descend(*target, parent_context));
            }
            SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                if !matches!(parent_context.mode, ProjectionMode::Navigate) {
                    stack.push(ReduceFrame::descend(*surface, parent_context));
                }
            }
            // Composite shapes — push children ONLY under whole-surface
            // `Published(Expanded)`. Per-prop / structural-transit
            // parents skip composite descent (the parent is the demand
            // terminal).
            SemanticNodeData::Object(view) => {
                if is_whole_surface_published(parent_context) {
                    for member in view.positive_members().iter() {
                        stack.push(ReduceFrame::descend(member.value, parent_context));
                    }
                    for sig in view.call_signatures.iter() {
                        stack.push(ReduceFrame::descend(*sig, parent_context));
                    }
                    for sig in view.construct_signatures.iter() {
                        stack.push(ReduceFrame::descend(*sig, parent_context));
                    }
                    for sig in view.index_signatures.iter() {
                        stack.push(ReduceFrame::descend(sig.key_type, parent_context));
                        stack.push(ReduceFrame::descend(sig.value_type, parent_context));
                    }
                    if let Some(ks) = view.keyspace {
                        stack.push(ReduceFrame::descend(ks, parent_context));
                    }
                }
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                if is_whole_surface_published(parent_context) {
                    for child in program.child_nodes() {
                        stack.push(ReduceFrame::descend(child, parent_context));
                    }
                }
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let arms = composite.composite_members().expect("composite arm");
                if is_whole_surface_published(parent_context) {
                    for arm in arms.iter() {
                        stack.push(ReduceFrame::descend(*arm, parent_context));
                    }
                }
            }
            SemanticNodeData::Array { element, .. } => {
                if is_whole_surface_published(parent_context) {
                    stack.push(ReduceFrame::descend(*element, parent_context));
                }
            }
            SemanticNodeData::Tuple { elements, .. } => {
                if is_whole_surface_published(parent_context) {
                    for el in elements.iter() {
                        stack.push(ReduceFrame::descend(el.value, parent_context));
                    }
                }
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                predicate,
                ..
            } => {
                if is_whole_surface_published(parent_context) {
                    for p in params.iter() {
                        stack.push(ReduceFrame::descend(p.ty, parent_context));
                    }
                    stack.push(ReduceFrame::descend(*return_type, parent_context));
                    for tp in type_parameters.iter() {
                        if let Some(c) = tp.constraint {
                            stack.push(ReduceFrame::descend(c, parent_context));
                        }
                        if let Some(d) = tp.default {
                            stack.push(ReduceFrame::descend(d, parent_context));
                        }
                    }
                    if let Some(target) = predicate.and_then(|predicate| predicate.ty) {
                        stack.push(ReduceFrame::descend(target, parent_context));
                    }
                }
            }
            // Operator shapes — operands push as StructuralTransit
            // (or Published(Navigate) for IndexedAccess.object under a
            // Published parent). Branches / mapper-internal nodes are
            // NEVER eagerly pushed — the dispatch picks the selected
            // branch or evaluates the mapped value per-key.
            SemanticNodeData::KeyOf { base } => {
                stack.push(ReduceFrame::descend(
                    *base,
                    structural_operand_context(parent_context),
                ));
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = indexed_access_object_context(parent_context);
                // The flow-return `ReturnType<typeof callee>['b']` member
                // hop: the OBJECT is NOT pre-reduced — reducing the
                // builtin carrier here would execute the callee's WHOLE
                // signature composition for a single-member demand. The
                // un-reduced carrier flows into the `IndexedAccess`
                // dispatch verbatim (the reduce arm's mapping fall-through)
                // and the walker's member-hop admission dispatches
                // `FlowReturn` with the single-member demand.
                let flow_member_hop = matches!(index, IndexKey::String(_))
                    && self.is_flow_return_type_member_base(*object);
                if !flow_member_hop {
                    stack.push(ReduceFrame::descend(*object, object_context));
                }
                if let IndexKey::Computed(n) = index {
                    stack.push(ReduceFrame::descend(
                        *n,
                        structural_operand_context(parent_context),
                    ));
                }
            }
            SemanticNodeData::Mapped { source, .. } => {
                // Source pushes as StructuralTransit for key enumeration.
                // value_expr / name_remap / parameter_node DO NOT push —
                // the MappedType dispatch substitutes them per-key under
                // an internal StructuralTransit evaluation (see
                // build.rs:1817 / 1852).
                stack.push(ReduceFrame::descend(
                    *source,
                    structural_operand_context(parent_context),
                ));
            }
            SemanticNodeData::Conditional { check, extends, .. } => {
                // Check / extends reduce structurally so the conditional
                // dispatch can decide the selected branch. The branches
                // themselves are NOT pre-pushed — demand-driven: only the
                // SELECTED branch is reduced (via the dispatch result).
                stack.push(ReduceFrame::descend(
                    *check,
                    structural_operand_context(parent_context),
                ));
                stack.push(ReduceFrame::descend(
                    *extends,
                    structural_operand_context(parent_context),
                ));
            }
            SemanticNodeData::TypeParam {
                constraint,
                default,
                ..
            } => {
                if is_whole_surface_published(parent_context) {
                    if let Some(c) = constraint {
                        stack.push(ReduceFrame::descend(*c, parent_context));
                    }
                    if let Some(d) = default {
                        stack.push(ReduceFrame::descend(*d, parent_context));
                    }
                }
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                // Args travel with the carrier — substituted into the
                // body if the carrier's dispatch reifies. Under
                // Navigate the carrier stays terminal so args effectively
                // stay un-reduced via the mapping fall-through.
                //
                // EXCEPT the flow-return `ReturnType<typeof callee>`
                // carrier under a non-whole-surface demand: reducing its
                // `typeof` argument here would compose the callee's full
                // signature — an eager WHOLE-body flow evaluation at a
                // position whose carrier stays terminal. The demand
                // points own that materialisation (the walker's member
                // hop dispatches the single-member `FlowReturn`; a
                // whole-surface demand reifies through `Instantiate`).
                let _ = base;
                let flow_return_carrier_stop = !is_whole_surface_published(parent_context)
                    && self.is_flow_return_type_member_base_data(data);
                if !flow_return_carrier_stop {
                    for arg in args.iter() {
                        stack.push(ReduceFrame::descend(*arg, parent_context));
                    }
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // Same-name merged contributors descend like intersection arms
                // under whole-surface publication.
                if is_whole_surface_published(parent_context) {
                    for contributor in contributors.iter() {
                        stack.push(ReduceFrame::descend(*contributor, parent_context));
                    }
                }
            }
        }
    }

    /// Reduce a single node assuming all its demand-selected children
    /// have already been reduced into `state.mapping`. Returns the
    /// reduced `SemanticNodeId`.
    ///
    /// `context` carries the publication / structural-transit demand;
    /// child lookups in `state.mapping` are keyed by
    /// `(child_node, child_context)` where `child_context` is derived
    /// per the demand-driven traversal rules (see
    /// [`Self::push_demand_children`]).
    ///
    /// Per-shape table:
    /// - Operator shapes (`IndexedAccess`, `KeyOf`, `Conditional`,
    ///   `Mapped`, `TypeOf`) dispatch the matching `SemanticQueryKey`
    ///   with the caller's `context`. `Value(reduced)` with `reduced
    ///   != node` recurses through [`Self::dispatch_operator_with_recurse`];
    ///   `Value(node)` (deferred over a free type parameter or
    ///   carrier-stop) accepts the form.
    /// - `DeclRef` / `InstantiationRef`: in `Published(Navigate)` /
    ///   `StructuralTransit`, terminal (DeclRef still follows aliases
    ///   because aliases are semantically transparent). In
    ///   `Published(Expanded)`, dispatch `ResolveDecl` /
    ///   `Instantiate`.
    /// - Composite shapes (`Object` / `Union` / `Intersection` /
    ///   `Array` / `Tuple` / `Function`) rebuild via
    ///   `intern_preserving_scope` when any child reduced; else return
    ///   `node` unchanged. Child reductions only land in `mapping`
    ///   under whole-surface `Published(Expanded)` (demand rule)
    ///   — so non-whole-surface contexts return the parent verbatim.
    /// - A `TemplateLiteral` hard-stop has no dispatch variant and
    ///   becomes `TypeExpr::Unknown(UnknownValue)` (raw text payload
    ///   carries the hard-stop message).
    /// - An `infer` declaration or reference is a binder of the deferred
    ///   conditional it sits in (a selected conditional substitutes it
    ///   away), kept as written: `T extends infer S ? S : never`.
    /// - Terminals (`Primitive` / `Literal` / `TypeParam` / `Opaque(…)`)
    ///   return `node` as-is.
    fn reduce_one(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        let Some(data) = super::node_data_for(self.graph(), node) else {
            return node;
        };
        let mode = context.mode;
        match data.as_ref() {
            // --- DeclPlaceholder unwrap (cross-file decl carrier) ---
            //
            // `ResolveDecl` returns
            // `Opaque(DeclPlaceholder { canonical_id, name, whole_hash })`
            // as a deferred carrier for cross-file declarations.
            //
            // The demand reducer unwraps the placeholder via the
            // matching `Instantiate { args: [] }` ONLY when the caller's
            // demand resolves deeply through cross-file aliases:
            //
            // - `Published(Expanded)` whole-surface — whole-surface
            //   materialisation deep-resolves cross-file aliases.
            // - `Published(Navigate)` — the navigate publication
            //   inherits the consumer's path-precision intent:
            //   intermediate hops (`IndexedAccess.object`,
            //   `DeclRef` followed through an IA chain) resolve to
            //   their bodies so the next hop / index lookup can
            //   pattern-match. The body is materialised but the
            //   surface stays shallow under Navigate because
            //   `push_demand_children` does not push composite
            //   children.
            //
            // Under `StructuralTransit` the carrier stays in place —
            // transit walks observe the placeholder structurally.
            //
            // The architectural "package-backed alias stays shallow"
            // rule is enforced at the PROJECTOR layer
            // (`node_package_backed_object_like_root_with_fence`
            // gate) — not inside the reducer. Reducer-time unwrap
            // is unconditional within a `Published` demand because
            // the projector has already decided the alias chain is
            // workspace-resolvable.
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => {
                if matches!(context.demand, ReductionDemand::StructuralTransit) {
                    return node;
                }
                // Architectural rule: package-backed alias names stay
                // shallow at the publication boundary. The placeholder
                // for a `node_modules`-resident declaration is the
                // intentional carrier — unwrapping it would inline the
                // package alias's body into the published surface and
                // violate the "imported alias names (workspace-owned
                // OR package-backed) — stay shallow regardless of
                // where they live" half of the shallow-by-default
                // rule. The projector-layer gate
                // (`node_package_backed_object_like_root_with_fence`)
                // only sees the OUTER raised root; a workspace-rooted
                // IA whose value lands on a package-backed alias
                // bypasses it. This check is the reducer-side mirror.
                if self.ctx.workspace_is_package_backed(canonical_id.as_ref()) {
                    return node;
                }
                // The placeholder's `whole_hash` is diagnostic payload only;
                // the `Instantiate` key is content-free (R6) and the cold
                // build re-sources the live whole_hash from
                // `ensure_indexed_ready_serve`.
                let _ = whole_hash;
                let base =
                    self.type_slot_for(canonical_id.clone(), *owner, name.clone());
                let inst_ctx = self.instantiate_context_for(&base.defining_canonical, context);
                let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
                    base,
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    inst_ctx,
                ));
                self.dispatch_operator_with_recurse(node, key, context, state)
            }

            // --- terminal shapes ---
            // Returned unchanged: reducing an intrinsic application is the
            // owning family's job, not the demand reducer's.
            SemanticNodeData::IntrinsicApplication { .. }
            | SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Opaque(_)
            // Raw-fallback / synthetic-binding carriers pass through this
            // reducer unchanged — the dispatch resolves them as a whole.
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::SyntheticBinding { .. } => node,

            // Unresolved bare-name / dynamic-import reference carriers: a
            // PUBLISHED demand completes carrier-head resolution through the
            // ONE shared normalization seam (the same
            // `resolve_carrier_subject_node` re-entry the path-walker's
            // terminal arm drives) — under `Navigate` the head resolves to
            // its identity carrier (shallow-by-default preserved; closed
            // builtins execute per the lowering gate), under `Expanded` the
            // resolution completes eagerly. Carrier-preserving on failure: a
            // self / opaque result keeps the shallow carrier, and a
            // structural-transit walk keeps the carrier fully terminal.
            SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {
                if matches!(context.demand, ReductionDemand::Published) {
                    let resolved = self.resolve_carrier_subject_node(node, context);
                    if resolved != node
                        && !matches!(
                            self.graph().node_data(resolved).as_deref(),
                            Some(SemanticNodeData::Opaque(_))
                        )
                    {
                        return self.reduce_subtree(resolved, context, state);
                    }
                }
                node
            }

            // --- hard-stop operator shapes (no dispatch variant) ---
            SemanticNodeData::TemplateLiteral { .. } => {
                self.opaque_unknown_with(node, "<unresolved template literal type>")
            }
            SemanticNodeData::Infer { .. } | SemanticNodeData::InferRef { .. } => node,

            // --- alias unwrap: follow target's reduction ---
            SemanticNodeData::Alias(target) => state
                .mapping
                .get(&(*target, context))
                .copied()
                .unwrap_or(*target),
            // A class expression's instance is its name under `Navigate` (the
            // shallow publication a `DeclRef` to a closed object keeps) and its
            // reduced instance surface under `Expanded`.
            SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                if matches!(mode, ProjectionMode::Navigate) {
                    node
                } else {
                    state
                        .mapping
                        .get(&(*surface, context))
                        .copied()
                        .unwrap_or(*surface)
                }
            }

            // --- operator dispatches (context-aware via underlying key) ---
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = indexed_access_object_context(context);
                let object = state
                    .mapping
                    .get(&(*object, object_context))
                    .copied()
                    .unwrap_or(*object);
                let index = index.clone();
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::IndexedAccess {
                        base: object,
                        index,
                        mode,
                    },
                    context,
                    state,
                )
            }
            SemanticNodeData::KeyOf { base } => {
                let base_context = structural_operand_context(context);
                let base = state
                    .mapping
                    .get(&(*base, base_context))
                    .copied()
                    .unwrap_or(*base);
                // raise.rs is a publication-path consumer (the bounded
                // fixed-point reducer + the typed-IR raise). Forward
                // the caller's context — `Published(Expanded)` reifies
                // the keyspace; `Published(Navigate)` /
                // `StructuralTransit` carrier-stop per
                // `may_reduce_operator`.
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::KeyOf { base, context },
                    context,
                    state,
                )
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            } => {
                let operand_context = structural_operand_context(context);
                let check = state
                    .mapping
                    .get(&(*check, operand_context))
                    .copied()
                    .unwrap_or(*check);
                let extends = state
                    .mapping
                    .get(&(*extends, operand_context))
                    .copied()
                    .unwrap_or(*extends);
                // Branches are NOT pre-pushed — the dispatch picks the
                // selected branch and `dispatch_operator_with_recurse`
                // reduces only that branch under the caller's context
                // (demand-driven: "reduce only the selected branch").
                let true_branch = *true_branch_ref;
                let false_branch = *false_branch_ref;
                let distributive = *distributive;
                let pending = pending.clone();
                let reduced = self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::Conditional {
                        check,
                        extends,
                        true_branch,
                        false_branch,
                        distributive,
                        pending: pending.clone(),
                    },
                    context,
                    state,
                );
                // OPEN-conditional demand distribution: when the oracle
                // DEFERS (the result stays a `Conditional` shell), a
                // whole-surface `Published(Expanded)` demand distributes
                // into BOTH branches — each branch reduces under the
                // caller's context and the shell is rebuilt around the
                // reduced branch refs. Decided conditionals are untouched
                // (only the selected branch was reduced), and carrier /
                // per-prop demands keep the shell fully symbolic.
                if is_whole_surface_published(context) {
                    if let Some(SemanticNodeData::Conditional {
                        check: d_check,
                        extends: d_extends,
                        true_branch_ref: d_true,
                        false_branch_ref: d_false,
                        distributive: d_dist,
                        pending: d_pending,
                    }) = self.graph().node_data(reduced).as_deref().map(crate::semantic_query::SemanticNodeData::clone)
                    {
                        let demanded_true = self.apply_conditional_branch_pending(
                            d_true,
                            d_pending.as_deref(),
                            true,
                        );
                        let demanded_false = self.apply_conditional_branch_pending(
                            d_false,
                            d_pending.as_deref(),
                            false,
                        );
                        let reduced_true = self.reduce_subtree(demanded_true, context, state);
                        let reduced_false = self.reduce_subtree(demanded_false, context, state);
                        if reduced_true != d_true
                            || reduced_false != d_false
                            || d_pending.is_some()
                        {
                            return self.graph().intern_preserving_scope(
                                reduced,
                                SemanticNodeData::Conditional {
                                    check: d_check,
                                    extends: d_extends,
                                    true_branch_ref: reduced_true,
                                    false_branch_ref: reduced_false,
                                    distributive: d_dist,
                                    pending: None,
                                },
                            );
                        }
                    }
                }
                reduced
            }
            SemanticNodeData::Mapped { source, mapper } => {
                let source_context = structural_operand_context(context);
                let source = state
                    .mapping
                    .get(&(*source, source_context))
                    .copied()
                    .unwrap_or(*source);
                // Re-key the mapper's `key_space` from any reduction the
                // source push produced. `value_expr` / `name_remap` /
                // `parameter_node` are NOT in mapping — the dispatch
                // substitutes the binder and evaluates per-key
                // internally.
                let mapper = remap_mapper(mapper, &state.mapping, source_context);
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::MappedType {
                        source,
                        mapper,
                        context,
                    },
                    context,
                    state,
                )
            }
            // The TERMINAL nominal carrier self-resolves: its content is the
            // declaring identity it already carries, so no typeof dispatch,
            // path projection, or instantiation apply runs for it at all.
            SemanticNodeData::TypeOfNominal(_) => node,
            SemanticNodeData::TypeOf(_) => {
                // `typeof value.path<args>`: resolve the value root through the
                // single typeof query, PROJECT the carrier's dotted path, THEN
                // apply the carrier's instantiation `type_args` to the projected
                // signature (resolve → project → apply, mirroring the eager
                // lowering order and the evaluate/walk arms). This is the
                // SEMANTIC reduction path (the structural raise/round-trip
                // preserves `type_args` separately in the
                // `shape_engine::fold_node` materialisation algebra); the final
                // reduced node is driven through the demand reducer below.
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                let value_root = value_root.clone();
                let path = path.clone();
                let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();

                let typeof_key = self.typeof_key_with_path(value_root, path, context);
                let root_read = self.execute_read(typeof_key);
                state.merge_dep_signature(&root_read.dep_signature);
                if root_read.result_is_partial() {
                    state.result_is_partial = true;
                    crate::request_context::mark_request_result_partial();
                }
                let projected = match root_read.value {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(_) | QueryResult::Error(_) => return node,
                };

                let final_node = if type_args.is_empty() {
                    projected
                } else {
                    self.apply_typeof_instantiation_args(projected, &type_args)
                };

                // 4. Drive the reduced node through the demand reducer (same
                //    result-threading as `dispatch_operator_with_recurse`):
                //    a self-identity result stays put; an already-reduced node
                //    is reused; otherwise its demanded children reduce under
                //    `context`.
                if final_node == node {
                    node
                } else if let Some(&already_reduced) = state.mapping.get(&(final_node, context)) {
                    already_reduced
                } else {
                    self.reduce_subtree(final_node, context, state)
                }
            }

            // --- lazy carriers ---
            SemanticNodeData::DeclRef { identity } => {
                if self.observation_interior_carrier_stop(node, &identity.canonical_id, state) {
                    // Observation fence: an INTERIOR owner-local /
                    // package-backed reference inside an observed
                    // published source stays the shallow carrier — the
                    // consumer re-resolves it on demand (registry entry
                    // / package registry respectively).
                    return node;
                }
                if matches!(mode, ProjectionMode::Navigate)
                    && userland_instantiation_body_is_closed_object(self.ctx, identity)
                {
                    // A `Published(Navigate)` terminal that lands ON a
                    // closed-object declaration (a nominal interface)
                    // stays the declaration-reference carrier — the
                    // published shape is identical to writing the plain
                    // reference, and the consumer re-resolves it on
                    // demand (shallow-by-default). This mirrors the
                    // `InstantiationRef` closed-object carve-out below.
                    // Alias / operator-bodied declarations still
                    // resolve: aliases are semantically transparent and
                    // an operator body is the demanded reduction.
                    //
                    // The verdict consulted the declaring file's
                    // prepared body — root it on the active fact tracer
                    // so a body-shape edit invalidates the published
                    // entry.
                    observe_closedness_walk_consult(self.ctx, identity.canonical_id.as_ref());
                    return node;
                }
                // Navigate follows alias chains because aliases are
                // semantically transparent. Dispatch and recurse — same
                // as Expanded for DeclRef.
                let scope = ScopeId {
                    canonical_id: identity.canonical_id.clone(),
                    owner: identity.owner,
                    local_scope: None,
                    binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(identity.owner),
                };
                let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                    scope,
                    name: identity.decl_name.clone(),
                });
                self.dispatch_operator_with_recurse(node, key, context, state)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                if self.observation_interior_carrier_stop(node, &base.canonical_id, state) {
                    // Observation fence: an INTERIOR owner-local /
                    // package-backed generic application inside an
                    // observed published source stays the shallow
                    // carrier `Ref { name, [substituted args] }` — the
                    // consumer re-resolves it on demand.
                    return node;
                }
                if matches!(context.demand, ReductionDemand::StructuralTransit) {
                    // Demand-driven: an InstantiationRef inspected by a
                    // structural-transit caller stays terminal —
                    // structural observation only; do not reify the
                    // body.
                    return node;
                }
                if matches!(mode, ProjectionMode::Navigate)
                    && base.canonical_id.as_ref() != "__builtin__"
                    && userland_instantiation_body_is_closed_object(self.ctx, base)
                {
                    // ChatMessages leak verdict: a
                    // userland `InstantiationRef` at a
                    // `Published(Navigate)` publication terminal
                    // STAYS TERMINAL **when its declared body is a
                    // closed Object surface** (a generic interface
                    // like `Tool<INPUT, OUTPUT> { outputSchema:
                    // ..., execute: ... }`). The earlier
                    // `Pub(Expanded)` hardcoding eagerly unwrapped
                    // these and fired
                    // `ProjectMember(outputSchema|execute)` audit
                    // edges that no caller demanded.
                    //
                    // Userland generic HELPERS whose body is
                    // operator-shaped (`Lookup<M, I> = M[I]`,
                    // `MyPick<X, K> = { [P in K]: X[P] }`, etc.)
                    // DO reduce even under Navigate —
                    // the type-arg substitution into an operator
                    // body is the "demanded instantiation is
                    // reduced as the terminal" case. Closed-object
                    // bodies behave like nominal interfaces, not
                    // operator helpers.
                    //
                    // Builtin utility types (`Pick`/`Omit`/...,
                    // `canonical_id == "__builtin__"`) do not match this
                    // closed-object carve-out — they are gated by the
                    // open-enumeration-domain carrier-stop (L1) below.
                    return node;
                }
                // L1 (Shallow-By-Default), route/mode-INDEPENDENT: an
                // object-filter utility (`Pick`/`Omit`) whose enumeration
                // domain (source argument) is OPEN — an unbound generic, an
                // open conditional/mapped/indexed/keyof, an instantiation
                // over open args, or an unresolved declaration — STAYS a
                // shallow carrier instead of materialising its source, in
                // EVERY demand context / mode. Materialising an open source
                // degenerates into full cross-file generic expansion (the
                // `ChatMessages.vue` `Pick<PropsBase<T>, …>` storm and the
                // `Table.vue` `Omit<CoreOptions<T>, …>` structural memo-cycle).
                // A CLOSED source (finite object surface / concrete
                // instantiation) still materialises path-precisely, in every
                // mode.
                //
                // The domain args are substituted through `mapping` first so a
                // bound type argument (a closed instantiation) is judged
                // closed and still reduces.
                let resolved_args: Vec<SemanticNodeId> = args
                    .iter()
                    .map(|id| state.mapping.get(&(*id, context)).copied().unwrap_or(*id))
                    .collect();
                if utility_enumeration_domain_is_open_or_unknown(self, base, &resolved_args) {
                    return node;
                }
                let base_key = self.type_slot_for(
                    base.canonical_id.clone(),
                    base.owner,
                    base.decl_name.clone(),
                );
                // Observation passes run the Instantiate query itself
                // CARRIER-PRESERVING (`Published(Shallow)`): the outer
                // substitution executes, but the substituted body keeps its
                // interior `DeclRef`/`InstantiationRef` carriers so the
                // observation fence can hold owner-local / package-backed
                // member values shallow while the observation walk (the
                // caller `context`) expands the rest. Non-observation passes
                // keep the caller-context-derived instantiate demand.
                let inst_projection = if state.observation.is_some() {
                    ProjectionReductionContext::published(ProjectionMode::Shallow)
                        .with_orthogonal_axes_from(context)
                } else {
                    context
                };
                let inst_ctx = self.instantiate_context_for(&base.canonical_id, inst_projection);
                let args: Arc<[SemanticNodeId]> = Arc::from(
                    args.iter()
                        .map(|id| state.mapping.get(&(*id, context)).copied().unwrap_or(*id))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(base_key, args, inst_ctx));
                self.dispatch_operator_with_recurse(node, key, context, state)
            }

            // --- composite rebuilds via intern_preserving_scope ---
            //
            // Composite children are only reduced under whole-surface
            // `Published(Expanded)` (demand traversal rule). For
            // per-prop `Published(Navigate)` / `Published(Shallow)` and
            // `StructuralTransit`, the composite parent is the demand
            // terminal — the `rebuild_*` helpers see no child entries
            // in `mapping` and return `node` unchanged.
            SemanticNodeData::Object(_) => {
                rebuild_object(self, node, &state.mapping, context).unwrap_or(node)
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                let rebuilt = program.map_child_nodes(|child| {
                    state
                        .mapping
                        .get(&(child, context))
                        .copied()
                        .unwrap_or(child)
                });
                if rebuilt == *program {
                    node
                } else {
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::ObjectSpreadProgram(rebuilt),
                    )
                }
            }
            SemanticNodeData::Union(arms) => rebuild_union_or_intersection(
                self,
                node,
                arms,
                arms.origin_category(),
                /* is_union */ true,
                &state.mapping,
                context,
            )
            .unwrap_or(node),
            SemanticNodeData::Intersection(arms) => rebuild_union_or_intersection(
                self,
                node,
                arms,
                arms.origin_category(),
                /* is_union */ false,
                &state.mapping,
                context,
            )
            .unwrap_or(node),
            SemanticNodeData::Array { element, readonly } => {
                let new_elem = state
                    .mapping
                    .get(&(*element, context))
                    .copied()
                    .unwrap_or(*element);
                if new_elem == *element {
                    node
                } else {
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element: new_elem,
                            readonly: *readonly,
                        },
                    )
                }
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                rebuild_tuple(self, node, elements.as_arc(), *readonly, &state.mapping, context)
                    .unwrap_or(node)
            }
            SemanticNodeData::DeferredCallable(_) => {
                if is_whole_surface_published(context) {
                    self.materialize_returned_callable(node)
                } else {
                    node
                }
            }
            SemanticNodeData::Signature {
                kind,
                params,
                return_type,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                return_type_span,
                predicate,
                is_abstract,
            } => {
                if is_whole_surface_published(context) {
                    if let crate::semantic_query::SignatureReturnCarrier::DeferredFlow(source) = return_carrier {
                        let (returned, inferred) = match self.execute_function_return_source(
                            source.source(), source.identity().anchor.canonical_id.as_ref(),
                        ) {
                            super::flow_return::FunctionReturnNode::Flow(result) =>
                                (source.apply_result(self, result.return_type()), result.inferred_predicate()),
                            super::flow_return::FunctionReturnNode::Aborted(abort) =>
                                (self.opaque(QueryError::ExecutionAborted(abort)), None),
                            _ => (self.opaque(QueryError::Miss), None),
                        };
                        let names: Vec<_> = type_parameters.iter().map(|p| Arc::<str>::from(&p.name)).collect();
                        let arguments: Vec<_> = type_parameters.iter().map(|p| p.param).collect();
                        let returned = self.bind_flow_return_own_clause(returned,
                            source.identity().anchor.canonical_id.as_ref(), source.identity().anchor.owner,
                            &names, &arguments).unwrap_or(returned);
                        let demanded = self.graph().intern_preserving_scope(node, SemanticNodeData::Signature {
                            kind: *kind, params: params.share(), return_type: returned,
                            type_parameters: type_parameters.share(), occurrence: occurrence.clone(),
                            return_carrier: crate::semantic_query::SignatureReturnCarrier::Function(source.source().clone()),
                            signature_span: *signature_span, return_type_span: *return_type_span,
                            predicate: predicate.or(inferred), is_abstract: *is_abstract,
                        });
                        return self.reduce_subtree(demanded, context, state);
                    }
                }
                rebuild_function(
                self,
                node,
                *kind,
                params,
                *return_type,
                type_parameters,
                occurrence.clone(),
                return_carrier.clone(),
                *signature_span,
                *return_type_span,
                *predicate,
                &state.mapping,
                context,
            )
            .unwrap_or(node)
            },
            SemanticNodeData::MergedDecl { contributors } => {
                // Reduce the peer-merged surface, then drive it through the
                // reducer so its demanded children reduce under `context`.
                let merged = self.reduce_merged_decl(contributors);
                if let Some(&already) = state.mapping.get(&(merged, context)) {
                    already
                } else {
                    self.reduce_subtree(merged, context, state)
                }
            }
        }
    }

    /// Helper: dispatch `key` and accumulate the dep_signature from the
    /// `CacheRead`.
    ///
    /// On `Value(result)`:
    /// - if `result == node` (deferred form), return `node` unchanged.
    /// - if `result != node`, recursively reduce `result` (it may itself
    ///   contain further operator nodes), then return that.
    ///
    /// On `Recursive(id)` or `Error(_)`: return `node` (deferred form).
    fn dispatch_operator_with_recurse(
        &self,
        node: SemanticNodeId,
        key: SemanticQueryKey,
        context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — every operator-node dispatch issues
        // one `execute_read` (which routes through `execute_cooperative`).
        crate::loop5_instrumentation::DISPATCH_OPERATOR_WITH_RECURSE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Loop-7 instrumentation — per-`SemanticQueryKey`-variant
        // wall-clock attribution. The kind index is captured BEFORE
        // `execute_read` consumes the key. The wall-clock window
        // covers `execute_read` (warm-hit fast-path AND cold-build
        // close-down) AND any recursive `reduce_subtree` follow-up the
        // dispatch triggers — i.e. the entire wall-clock cost
        // attributable to this single operator-node dispatch.
        let kind_idx = crate::loop5_instrumentation::kind_index_for_key(&key);
        crate::loop5_instrumentation::DISPATCH_OPERATOR_KIND_CALLS[kind_idx]
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dispatch_started = Instant::now();

        let read = self.execute_read(key);
        state.merge_dep_signature(&read.dep_signature);
        // Partial-result propagation: a `result_is_partial=true` read
        // means the semantic dispatch produced a PARTIAL outcome (budget
        // exhaustion / cancellation / same-path recursion / walker
        // fatal). Such a result must not warm any shared cache. Mark the
        // per-reduce-state flag so the enclosing
        // `raise_and_reduce_with_context` returns a `MaterializedTypeExpr`
        // carrying `result_is_partial=true`, AND raise the request-scoped
        // sticky bit so downstream callers (the projector second pass, the
        // final ComponentMeta cache admission gate) observe it without
        // needing a hand-threaded return value. A benign non-cacheable
        // read (`cache_suppress` without partiality — ReturnOnly /
        // fenced serve / unrootable self-root) is NOT folded here: it refuses
        // only its own inner-memo admission and MUST NOT suppress a
        // complete component-meta result.
        if read.result_is_partial() {
            state.result_is_partial = true;
            crate::request_context::mark_request_result_partial();
        }
        let result = match read.value {
            QueryResult::Value(result) => {
                if result == node {
                    node
                } else if let Some(&already_reduced) = state.mapping.get(&(result, context)) {
                    already_reduced
                } else {
                    // The dispatch produced a new node not yet in
                    // `mapping`. Drive it through the iterative
                    // reducer's worklist so its demanded children are
                    // selectively reduced under `context` (the
                    // demand-traversal rule). Cycle protection: the
                    // shared `visited` set deduplicates re-entry.
                    self.reduce_subtree(result, context, state)
                }
            }
            QueryResult::Recursive(_id) => node,
            QueryResult::Error(error @ QueryError::IncompleteSemanticOperand { .. }) => {
                // This demanded reduction completed without a usable value.
                // Preserve its typed failure instead of republishing the
                // authored subject as if it were a deliberate carrier stop.
                self.graph()
                    .intern_preserving_scope(node, SemanticNodeData::Opaque(error))
            }
            QueryResult::Error(_) => node,
        };

        let elapsed_ns = dispatch_started.elapsed().as_nanos() as u64;
        crate::loop5_instrumentation::DISPATCH_OPERATOR_KIND_NS[kind_idx]
            .fetch_add(elapsed_ns, std::sync::atomic::Ordering::Relaxed);
        crate::loop5_instrumentation::DISPATCH_OPERATOR_TOTAL_NS
            .fetch_add(elapsed_ns, std::sync::atomic::Ordering::Relaxed);
        result
    }

    /// Helper: convert a reducer-driven hard-stop into an
    /// `Opaque(QueryError::Other(reason))` interned at the origin node's
    /// scope so subsequent raises render the documented sentinel.
    fn opaque_unknown_with(&self, origin: SemanticNodeId, reason: &str) -> SemanticNodeId {
        self.graph().intern_preserving_scope(
            origin,
            SemanticNodeData::Opaque(QueryError::Other(verter_type_expr::OwnedStr::from(reason))),
        )
    }
}

/// `(SemanticNodeId, ProjectionReductionContext)` → `SemanticNodeId`
/// map used by the reducer to fetch already-reduced operand / child
/// reductions. Keyed by the operand's own context so a structural-
/// transit reduction does not collide with a publication reduction of
/// the same node.
type MappingMap =
    rustc_hash::FxHashMap<(SemanticNodeId, ProjectionReductionContext), SemanticNodeId>;

/// Top-down demand reducer work-stack frame.
#[derive(Clone, Copy)]
enum ReduceFrameKind {
    /// Mark `(node, context)` visited, push the matching `Reduce`
    /// frame, then push the demand-selected children.
    Descend,
    /// All demand-selected children have been reduced into
    /// `state.mapping`; invoke `reduce_one(node, context, state)` and
    /// record the result.
    Reduce,
}

struct ReduceFrame {
    node: SemanticNodeId,
    context: ProjectionReductionContext,
    kind: ReduceFrameKind,
}

impl ReduceFrame {
    #[inline]
    fn descend(node: SemanticNodeId, context: ProjectionReductionContext) -> Self {
        Self {
            node,
            context,
            kind: ReduceFrameKind::Descend,
        }
    }
}

/// `true` when `ctx` is the demand-driven whole-surface publication
/// demand (`Published + Expanded`). Composite-child traversal pushes
/// descend frames ONLY in this case; per-prop `Published(Navigate)` /
/// `Published(Shallow)` and any `StructuralTransit` walk treat the
/// composite parent as the demand terminal.
#[inline]
fn is_whole_surface_published(ctx: ProjectionReductionContext) -> bool {
    matches!(ctx.demand, ReductionDemand::Published) && matches!(ctx.mode, ProjectionMode::Expanded)
}

/// Derive the `IndexedAccess.object` operand context from the parent
/// IndexedAccess's reduction context.
///
/// The demand-driven reducer spec ("`Foo['a']['b']`: intermediate hops are
/// navigate-only, terminal uses caller mode") describes the
/// PATH-WALK semantics inside the dispatch's `ProjectPath` builder.
/// At the iterative-reducer layer the object operand must be REDUCED
/// enough for the `IndexedAccess` dispatch to look up the index — so
/// a generic instantiation like `Pick<Foo, 'a'>` materialises its
/// body and the indexed access can pick `'a'` out of it.
///
/// Under any `Published` parent → demote the object operand to
/// [`ProjectionMode::Navigate`] (demand + provenance + merge_role
/// preserved). The object operand is the INTERMEDIATE hop of the
/// indexed-access path (`Root['a']` in `Root['a']['b']`): it must be
/// navigated (followed through aliases / generic bodies so the outer
/// `IndexedAccess` dispatch can look up the index) but NOT expanded.
/// Only the OUTER `IndexedAccess` dispatch keeps the caller's mode, so
/// the terminal consumed segment is the sole one that expands. This is
/// the path-precision rule "intermediate hops run in Navigate, the
/// terminal hop runs in the caller's mode" applied to the
/// raise/materialize reducer. Inheriting the parent's mode here would
/// over-expand the intermediate object (e.g. materialise the sibling
/// members of `Root['a']` that the path never selects) — the
/// shallow-by-default violation this demotion fixes.
///
/// Under `StructuralTransit` parent → `StructuralTransit` (the transit
/// walk observes the object structurally without materialising it). Every
/// operand demotion preserves the orthogonal Vue heritage policy so a runtime
/// declaration carrier still filters producer-addressed heritage before it
/// resolves.
#[inline]
pub(crate) fn indexed_access_object_context(
    parent_context: ProjectionReductionContext,
) -> ProjectionReductionContext {
    if matches!(parent_context.demand, ReductionDemand::Published) {
        parent_context.with_mode(ProjectionMode::Navigate)
    } else {
        structural_operand_context(parent_context)
    }
}

/// Structural operand context: retain the reducer's historical structural
/// provenance/merge defaults while preserving the orthogonal runtime heritage
/// policy. Operand lowering must carrier-stop, but it cannot erase the policy
/// before an imported declaration head is instantiated.
#[inline]
fn structural_operand_context(
    parent_context: ProjectionReductionContext,
) -> ProjectionReductionContext {
    ProjectionReductionContext::structural_transit().with_orthogonal_axes_from(parent_context)
}

/// Re-key a `MapperKey` using `mapping` (substituting any reduced
/// `key_space` id from the demand walk). The mapper's `value_expr` /
/// `name_remap` / `parameter_node` are NOT looked up — the dispatch
/// substitutes them per-key internally under a structural-transit
/// evaluation (see [`crate::project_semantic_dispatch::evaluate`]'s
/// context-explicit variant).
fn remap_mapper(
    mapper: &MapperKey,
    mapping: &MappingMap,
    source_context: ProjectionReductionContext,
) -> MapperKey {
    let mut new_mapper = mapper.clone();
    new_mapper.key_space = mapping
        .get(&(mapper.key_space, source_context))
        .copied()
        .unwrap_or(mapper.key_space);
    new_mapper
}

fn rebuild_object<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let data = super::node_data_for(dispatch.graph(), node)?;
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return None;
    };
    let mut changed = false;
    let new_members: Arc<[SurfaceMember]> = {
        let mut out: Vec<SurfaceMember> = Vec::with_capacity(view.positive_members().len());
        for m in view.positive_members().iter() {
            let new_value = mapping.get(&(m.value, context)).copied().unwrap_or(m.value);
            if new_value != m.value {
                changed = true;
            }
            out.push(SurfaceMember {
                value: new_value,
                ..m.clone()
            });
        }
        Arc::from(out.into_boxed_slice())
    };
    let new_calls: Arc<[SemanticNodeId]> = {
        let mut out = Vec::with_capacity(view.call_signatures.len());
        for sig in view.call_signatures.iter() {
            let new_sig = mapping.get(&(*sig, context)).copied().unwrap_or(*sig);
            if new_sig != *sig {
                changed = true;
            }
            out.push(new_sig);
        }
        Arc::from(out.into_boxed_slice())
    };
    let new_constructs: Arc<[SemanticNodeId]> = {
        let mut out = Vec::with_capacity(view.construct_signatures.len());
        for sig in view.construct_signatures.iter() {
            let new_sig = mapping.get(&(*sig, context)).copied().unwrap_or(*sig);
            if new_sig != *sig {
                changed = true;
            }
            out.push(new_sig);
        }
        Arc::from(out.into_boxed_slice())
    };
    if !changed {
        return Some(node);
    }
    let mut members = new_members
        .iter()
        .map(crate::semantic_query::SurfaceMember::clone);
    let mut calls = new_calls.iter().copied();
    let mut constructs = new_constructs.iter().copied();
    let entries = view
        .entries
        .iter()
        .map(|entry| match entry {
            crate::semantic_query::SurfaceEntry::Member(_) => {
                crate::semantic_query::SurfaceEntry::Member(
                    members.next().expect("derived member index matches stream"),
                )
            }
            crate::semantic_query::SurfaceEntry::CallSignature(_) => {
                crate::semantic_query::SurfaceEntry::CallSignature(
                    calls.next().expect("derived call index matches stream"),
                )
            }
            crate::semantic_query::SurfaceEntry::ConstructSignature(_) => {
                crate::semantic_query::SurfaceEntry::ConstructSignature(
                    constructs
                        .next()
                        .expect("derived construct index matches stream"),
                )
            }
            crate::semantic_query::SurfaceEntry::IndexSignature(signature) => {
                crate::semantic_query::SurfaceEntry::IndexSignature(signature.clone())
            }
        })
        .collect();
    let new_view =
        SurfaceView::from_entries(entries, view.keyspace, view.has_known_index_signature());
    Some(
        dispatch
            .graph()
            .intern_preserving_scope(node, SemanticNodeData::Object(new_view)),
    )
}

fn rebuild_union_or_intersection<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    arms: &[SemanticNodeId],
    category: crate::semantic_query::composite::CompositeOriginCategory,
    is_union: bool,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_arms: Vec<SemanticNodeId> = arms
        .iter()
        .map(|arm| {
            let new = mapping.get(&(*arm, context)).copied().unwrap_or(*arm);
            if new != *arm {
                changed = true;
            }
            new
        })
        .collect();
    if !changed {
        return Some(node);
    }
    // Order- and scope-preserving rebuild: the reduced arms replace the
    // originals 1:1, inheriting the original carrier's semantics.
    let new_arms: Arc<[SemanticNodeId]> = Arc::from(new_arms.into_boxed_slice());
    let data = if is_union {
        SemanticNodeData::Union(
            crate::semantic_query::composite::CompositeList::rebuilt_from(category, new_arms),
        )
    } else {
        SemanticNodeData::Intersection(
            crate::semantic_query::composite::CompositeList::rebuilt_from(category, new_arms),
        )
    };
    Some(dispatch.graph().intern_preserving_scope(node, data))
}

fn rebuild_tuple<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    elements: &Arc<[TupleElement]>,
    readonly: bool,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_elements: Vec<TupleElement> = elements
        .iter()
        .map(|el| {
            let new_value = mapping
                .get(&(el.value, context))
                .copied()
                .unwrap_or(el.value);
            if new_value != el.value {
                changed = true;
            }
            TupleElement {
                value: new_value,
                ..el.copy()
            }
        })
        .collect();
    if !changed {
        return Some(node);
    }
    Some(dispatch.graph().intern_preserving_scope(
        node,
        SemanticNodeData::Tuple {
            elements: crate::semantic_query::PayloadArray::from(new_elements.into_boxed_slice()),
            readonly,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn rebuild_function<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    kind: crate::semantic_query::SignatureKind,
    params: &crate::semantic_query::PayloadArray<crate::semantic_query::FunctionParam>,
    return_type: SemanticNodeId,
    type_parameters: &crate::semantic_query::PayloadArray<crate::semantic_query::TypeParamDecl>,
    occurrence: Option<crate::semantic_query::SignatureNodeOccurrence>,
    return_carrier: crate::semantic_query::SignatureReturnCarrier,
    signature_span: Option<verter_span::Span>,
    return_type_span: Option<verter_span::Span>,
    predicate: Option<crate::semantic_query::SignaturePredicate>,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_predicate = predicate.map(|predicate| {
        predicate.map_type(|target| {
            let new_target = mapping.get(&(target, context)).copied().unwrap_or(target);
            changed |= new_target != target;
            new_target
        })
    });
    let new_params: Vec<crate::semantic_query::FunctionParam> = params
        .iter()
        .map(|p| {
            let new_ty = mapping.get(&(p.ty, context)).copied().unwrap_or(p.ty);
            if new_ty != p.ty {
                changed = true;
            }
            crate::semantic_query::FunctionParam {
                ty: new_ty,
                ..p.copy()
            }
        })
        .collect();
    let new_return = mapping
        .get(&(return_type, context))
        .copied()
        .unwrap_or(return_type);
    if new_return != return_type {
        changed = true;
    }
    let new_type_params: Vec<crate::semantic_query::TypeParamDecl> = type_parameters
        .iter()
        .map(|tp| {
            let new_constraint = tp
                .constraint
                .map(|c| mapping.get(&(c, context)).copied().unwrap_or(c));
            let new_default = tp
                .default
                .map(|d| mapping.get(&(d, context)).copied().unwrap_or(d));
            if new_constraint != tp.constraint || new_default != tp.default {
                changed = true;
            }
            crate::semantic_query::TypeParamDecl {
                constraint: new_constraint,
                default: new_default,
                ..tp.copy()
            }
        })
        .collect();
    if !changed {
        return Some(node);
    }
    Some(dispatch.graph().intern_preserving_scope(
        node,
        SemanticNodeData::Signature {
            kind,
            is_abstract: dispatch.signature_is_abstract(node),
            params: crate::semantic_query::PayloadArray::from(new_params.into_boxed_slice()),
            return_type: new_return,
            type_parameters: crate::semantic_query::PayloadArray::from(
                new_type_params.into_boxed_slice(),
            ),
            // Node remapping preserves source provenance — the occurrence
            // verbatim, the declared carrier retargeted at the remapped
            // return.
            occurrence,
            return_carrier: match return_carrier {
                crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                    crate::semantic_query::SignatureReturnCarrier::Declared(new_return)
                }
                crate::semantic_query::SignatureReturnCarrier::Function(source) => {
                    crate::semantic_query::SignatureReturnCarrier::Function(source)
                }
                other @ (crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_)) => other,
            },
            signature_span,
            return_type_span,
            predicate: new_predicate,
        },
    ))
}

// The materialization-grade result of the reduce-then-raise boundary is
// the sealed [`MaterializedOutputTypeExpr`] carrier
// (`super::output_materialization`): a private inner sealed `type_expr`
// payload (capability-gated unwrap) plus the readable facts-rail metadata
// (`node_id` / `dep_signature` / `result_is_partial`). The all-`pub`-field
// `MaterializedTypeExpr` struct that previously lived here is retired —
// handing back a bare `TypeExpr` field at the boundary was the laundering
// surface the output-materialization capability fence closes.

// Accumulator for `raise_and_reduce_with_context`; reached in production
// through the reduce-then-raise output boundary from the per-member
// projectors.
#[derive(Default)]
struct ReduceState {
    visited: FxHashSet<(SemanticNodeId, ProjectionReductionContext)>,
    mapping: MappingMap,
    dep_facts: Vec<crate::semantic_query::DepEntry>,
    /// OR-fold of every `read.result_is_partial()` observed by `reduce_one`
    /// during this reduce pass. Propagated into the returned
    /// `MaterializedTypeExpr.result_is_partial()` so direct consumers
    /// (e.g. `field_types::materialize_component_meta_type_expr_until_stable_full`)
    /// can refuse to publish a PARTIAL result into shared caches without
    /// needing to inspect the request-scoped TLS flag. NOT set by a
    /// benign non-cacheable read (`cache_suppress` without partiality).
    result_is_partial: bool,
    /// Consumer-OBSERVATION carrier fence: `Some((owner_canonical, root))`
    /// when this reduce pass is the observation of a PUBLISHED
    /// `SemanticTypeSource` (the consumer read of a registry entry / prop
    /// source). The observation executes the OUTER carrier — the demand
    /// `root` always resolves/instantiates, so an authored generic-alias
    /// body runs the existing `Instantiate` query and outer type arguments
    /// substitute — but INTERIOR named carriers stop shallow when the
    /// consumer can re-resolve them on demand:
    ///
    /// - an OWNER-local helper reference (`canonical == owner_canonical`)
    ///   stays the shallow `Ref` carrier — the owner's registry publishes
    ///   the helper as its own entry, so the consumer re-resolves it there
    ///   (shallow-by-default);
    /// - a PACKAGE-backed reference stays symbolic — the reducer-side
    ///   mirror of the "package alias names stay shallow" publication rule.
    ///
    /// Workspace-imported interior branches still expand (they have no
    /// registry entry of their own to re-resolve against). `None` (every
    /// non-observation reduce pass) keeps the pre-existing demand-walk
    /// behaviour unchanged.
    observation: Option<(Arc<str>, SemanticNodeId)>,
}

// `ReduceState` helpers for `raise_and_reduce_with_context`; reached in
// production through the reduce-then-raise output boundary from the
// per-member projectors.
impl ReduceState {
    fn merge_dep_signature(&mut self, sig: &DepSignature) {
        for entry in sig.iter() {
            // Light dedup on (canonical, version) — keeps the
            // accumulated list O(unique_facts) rather than
            // O(num_dispatches × facts_per_dispatch).
            if !self.dep_facts.contains(entry) {
                // The clone shares the entry's owned canonical.
                self.dep_facts.push(entry.clone());
            }
        }
    }

    fn into_dep_signature(self) -> DepSignature {
        DepSignature::new(self.dep_facts)
    }
}

// =====================================================================
// Node-domain raised-shape decision surface (owner-local).
//
// The classifiers + equality primitives below are the graph-native decision
// surface the Kind-B callers use: they read the TRUE bottom-up
// `RaisedShapeFacts` / interned `RaisedShapeKey` computed by the shared
// [`shape_engine`] (the `RaisedShapeAlg` fold), WITHOUT materialising a
// `TypeExpr`. The materialization (`MaterializeTypeExprAlg`) and the
// node-domain facts/key share ONE traversal ([`shape_engine::fold_node`]), so
// they CANNOT drift — anti-drift is structural. The parity tests
// (`super::raised_shape_tests`) prove `bottom_up_classifier(node) ≡
// legacy_TypeExpr_predicate ∘ materialize_for_test(node)`; the materialization
// is pinned independently by the raise / materialization suite.
// =====================================================================

/// Combined facts + node-vs-`TypeExpr` equality of ONE node fold. Re-exported so
/// the Kind-B sink adapters read both the route-gate facts AND the no-op/changed
/// decision from a single projection.
pub use shape_engine::NodeShapeEq;
/// The publication-scoring facts of a raised shape (`symbolic_carriers` /
/// `generic_detail` / `structural_top_level` / `exact_unknown_root`). Re-exported
/// so `crate::meta_resolve::scoring`'s comparison formula reads the SAME facts the
/// node front and the `&TypeExpr` front both produce.
pub use shape_engine::PublicationScore;
/// The NODE-BOUND raised-shape facts witness (its `RaisedShapeFacts` — the inner
/// facts type, kept PRIVATE to the shape engine — PAIRED with the `SemanticNodeId`
/// they were computed for). Re-exported so the Kind-B sink adapters and the
/// `route_admission` mint helpers read the facts and the bound node from ONE sealed
/// value — the sole cross-module admission input. Cross-module readers use the
/// witness's `pub(crate)` passthrough getters, never the inner `RaisedShapeFacts`
/// type directly.
pub use shape_engine::RaisedNodeShapeFacts;

/// The [`PublicationScore`] of `node` — folded through the publication algebra
/// over the shared [`shape_engine`] traversal, WITHOUT materialising a
/// `TypeExpr`. `None` when the whole raise is `None`. DISPATCH-taking primary —
/// the publication finaliser scores both candidate carriers' nodes through this.
#[must_use]
pub fn project_node_publication_score_with_dispatch<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<PublicationScore> {
    shape_engine::project_node_publication_score(dispatch, node)
}

/// The [`PublicationScore`] of an existing `&TypeExpr` — the TypeExpr front of the
/// SHARED publication formula. Reproduces the historical
/// `count_*_in_expr`/`type_expr_has_structural_top_level`/`matches!(_, Unknown)`
/// semantics EXACTLY, so `compare_type_expr_improvement`'s existing callers see
/// byte-identical verdicts.
#[must_use]
#[cfg(any(test, feature = "test-support"))]
pub fn type_expr_publication_score(expr: &TypeExpr) -> PublicationScore {
    shape_engine::type_expr_publication_score(expr)
}

// The node-domain decision API has ONE signature form: every member takes the
// caller's `&ProjectSemanticDispatch`. A caller already holds one at every
// production decision site, so no member re-runs `ProjectSemanticDispatch::new`
// behind the caller's back — the redundant dispatch construction a ctx-taking
// convenience form performs is what made a per-element equality probe pay one
// dispatch per element.

/// `true` when `node` can be shell-raised to a `TypeExpr` at all
/// (`raise(node).is_some()`). DISPATCH-taking primary.
#[must_use]
pub fn node_can_shell_raise_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    shape_engine::project_node_facts(dispatch, node).is_some_and(|facts| facts.can_shell_raise())
}

/// The node-bound [`RaisedNodeShapeFacts`] witness of `node` (its facts — the
/// materialized-miss + expanded-surface gate the Kind-B route helpers apply —
/// PAIRED with `node`). `None` when the whole raise is `None`. DISPATCH-taking
/// primary. Folds through the FACTS-ONLY [`shape_engine`] algebra — no structural
/// key interned. A mint site passes the witness straight to a `route_admission`
/// `admit_*` helper, which binds the carrier to `witness.node()`.
#[must_use]
pub fn node_raised_shape_facts_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<RaisedNodeShapeFacts> {
    shape_engine::project_node_facts(dispatch, node)
}

/// Classify a node's normalized raised shape into the closed shallow member
/// output vocabulary without reverse-materializing a `TypeExpr`.
#[must_use]
pub fn node_shallow_member_output_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<RaisedShallowMemberOutput> {
    shape_engine::project_node_shallow_member_output(dispatch, node)
}

/// Whether `node`'s OWN raised ROOT term is a typed unmaterialised degradation
/// (the node-domain root-sentinel fact). A whole-raise `None` never reaches
/// this fact (the terminal degrades it as the typed `Miss` carrier upstream,
/// so the question is moot there); the compat tree is inert and never feeds
/// this classification. DISPATCH-taking primary — the cache-admission gate
/// reads this off the reduced-output carrier node instead of materialising it.
#[must_use]
pub fn node_root_is_unmaterialized_sentinel_with_dispatch<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    shape_engine::project_node_root_sentinel(dispatch, node).unwrap_or(false)
}

/// Node-domain gate for the macro-output expansion sink: `true` when `node`'s
/// whole raise MISSES (an absent / unraisable node) OR its raised ROOT term is
/// an unmaterialised sentinel — the two no-faithful-output cases the sink maps
/// to its `None` arm, mirroring the shell-raise oracle's `None`. Distinct from
/// [`node_root_is_unmaterialized_sentinel_with_dispatch`], whose whole-raise
/// `None` arm deliberately answers `false` for the cache-admission gate.
#[must_use]
pub fn node_raise_misses_or_root_sentinel_with_dispatch<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    shape_engine::project_node_root_sentinel(dispatch, node).unwrap_or(true)
}

/// Node-domain equivalent of `type_expr_root_is_published_operator(raise(node))`:
/// whether `node`'s NORMALIZED raised root is a published surface operator
/// (`Ref` / `KeyOf` / `IndexedAccess` / `Conditional` / `TypeOf`, or a `Mapped`
/// whose value root is NOT `semanticMiss`). Reads the post-normalized raised root
/// off the ROOT-ONLY projection (the source it normalizes through) — so it
/// answers IDENTICALLY to the `TypeExpr` predicate on the raised value, including
/// for shapes the raw-node walk mis-classifies (e.g. an `Intersection([{}, op])`
/// the root-only projection collapses to its operator arm). A whole-raise `None`
/// is `false`.
#[must_use]
pub fn node_root_is_published_operator_with_dispatch<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    shape_engine::project_node_root_is_published_operator(dispatch, node).unwrap_or(false)
}

/// Node-domain equivalent of `type_expr_contains_semantic_miss(raise(node))`:
/// whether `node`'s raised shape carries a semantic miss ANYWHERE in its tree
/// (`!RaisedShapeFacts.materialized`). `None` when the whole raise is `None`,
/// letting the caller distinguish "no miss" (`Some(false)`) from "unraisable"
/// (`None`) — DISTINCT from the root-only
/// [`node_root_is_unmaterialized_sentinel_with_dispatch`]. A consumer that
/// treats an unraisable node as a miss collapses the `None` itself
/// (`.unwrap_or(true)`); that reading is not a separate member here.
/// DISPATCH-taking primary — a publication carrier path reads this off the
/// reduced-output carrier node instead of materialising it to a `TypeExpr` and
/// running a raised-string walk.
//
// Production consumer: the publication reducer's input-side no-poison gate
// (`meta_resolve::projectors::output_sink::reduce_field_value_node` /
// `reduce_published_field_types` — the deleted TypeExpr helper
// `reduce_field_type_expr_with_mode` is gone), which seals the INPUT as the
// published carrier only when the input is CONFIDENTLY miss-free
// (`Some(false)`). The parity suite additionally exercises it as the
// node-vs-`TypeExpr` equivalence proof.
#[must_use]
pub fn node_contains_semantic_miss_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<bool> {
    shape_engine::project_node_contains_semantic_miss(dispatch, node)
}

/// The node-domain declaration facts of `node` in ONE fold: whether every
/// rendered leaf is declaration-safe (no implicit `any` / unknown /
/// synthetic slot binding / return-less function) AND the reachable
/// `typeof <value>` dependency paths. `None` when the whole raise is
/// `None`. The terminal splice pipeline decides on this — it materializes
/// at most once afterwards, solely for display.
pub fn node_declaration_facts_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> Option<(
    bool,
    std::collections::BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
)> {
    shape_engine::project_node_declaration_facts(dispatch, node)
}

/// Combined `RaisedShapeFacts` + node-vs-`TypeExpr` equality of `node` in ONE
/// fold: the route-gate facts AND the no-op/changed decision (`eq_to_expr`) from
/// a single key-bearing fold (the input `expr` is interned into the SAME
/// interner). A site needing both reads this instead of folding the node twice.
/// DISPATCH-taking primary. `None` when the whole raise is `None`.
#[must_use]
pub fn node_raised_shape_for_eq_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    expr: &TypeExpr,
) -> Option<NodeShapeEq> {
    shape_engine::project_node_shape_for_eq(dispatch, node, expr)
}

/// Raised-shape equality between two nodes: `Some(true)`/`Some(false)` when
/// BOTH nodes raise to `Some`, `None` when EITHER raise is `None`.
///
/// Compares the interned [`shape_engine::RaisedShapeKey`] — NOT the node id:
/// carriers drop identity on raise (different `DeclRef`/`BareRef`/`TypeParam`
/// nodes can raise to equal shapes and vice-versa), so the raised-shape key is
/// the comparison subject.
//
// Node-vs-node member of the node-domain decision API: the query-engine
// admitted-node compare (`route_projection_nodes_eq`) and the Vue authored-
// locator coverage proof read successive nodes through this form. Both probe
// element-wise inside a loop, so taking the caller's dispatch keeps the whole
// probe on one dispatch instead of minting one per element.
#[must_use]
pub fn raised_shape_eq_nodes_with_dispatch<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> Option<bool> {
    shape_engine::raised_shape_eq_nodes(dispatch, a, b)
}

/// Raised-shape equality between a node and a `TypeExpr`: `Some(bool)` when the
/// node raises to `Some`, `None` when the raise is `None`. The input `TypeExpr`
/// is folded into the SAME interned key space (no node materialisation).
//
// Production no-op/changed gates read both facts AND equality from one fold via
// `node_raised_shape_for_eq_with_dispatch`, so this standalone node-vs-`TypeExpr`
// form is exercised by the parity suite (the equivalence proof: it must equal
// `raise(node) == Some(expr)`).
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "production gates read facts + equality from one fold via \
                  node_raised_shape_for_eq_with_dispatch; the standalone equality form \
                  is exercised by the parity suite"
    )
)]
#[must_use]
pub fn raised_shape_eq_node_type_expr_with_dispatch<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
    expr: &TypeExpr,
) -> Option<bool> {
    shape_engine::raised_shape_eq_node_type_expr(dispatch, node, expr)
}

#[cfg(test)]
mod tests {
    use super::indexed_access_object_context;

    #[test]
    fn indexed_access_object_demotion_preserves_vue_heritage_policy() {
        use crate::semantic_query::{
            ProjectionMode, ProjectionReductionContext, ReductionDemand, SurfaceProvenanceContext,
            VueHeritagePolicy,
        };

        let filtered_parent = ProjectionReductionContext::vue_runtime_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        );
        let unfiltered_parent = ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        );
        let filtered = indexed_access_object_context(filtered_parent);
        let unfiltered = indexed_access_object_context(unfiltered_parent);

        assert_eq!(filtered.demand, ReductionDemand::StructuralTransit);
        assert_eq!(filtered.mode, ProjectionMode::Shallow);
        assert_eq!(
            filtered.vue_heritage_policy,
            VueHeritagePolicy::SuppressIgnored,
            "indexed-access operand demotion must not erase runtime filtering"
        );
        assert_eq!(unfiltered.vue_heritage_policy, VueHeritagePolicy::RetainAll);
        assert_ne!(
            filtered, unfiltered,
            "filtered/unfiltered operands must enter distinct PRC family identities"
        );

        // Mutation recipe: restore the fresh `structural_transit()` fallback
        // in `indexed_access_object_context`; `filtered` becomes RetainAll and
        // both operand contexts collapse.
    }
}
