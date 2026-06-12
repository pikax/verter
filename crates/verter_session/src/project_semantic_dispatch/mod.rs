//! Project-global [`SemanticQueryApi`] dispatcher.
//!
//! Binds [`SemanticQueryKey`] variants onto the shared
//! [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore) memo
//! and routes them to the existing resolver/solver entry points. This is the
//! single dispatch site every reusable type-resolution operation flows
//! through, regardless of which higher-level request initiated it.
//!
//! ## Scope of this landing
//!
//! Every [`SemanticQueryKey`] variant dispatches through this module:
//!
//! - `ResolveDecl` — returns a structural node sourced from the shallow state
//!   (an `Object` surface when the declaration carries member signatures,
//!   otherwise a `Primitive(Never)` anchor that memoizes the declaration's
//!   scope/name identity).
//! - `Instantiate` — identity-preserving alias anchored to the base node.
//!   Memoizing the key is the dedup guarantee; the richer instantiation
//!   shape is produced on demand by the solver once the caller walks
//!   into it. Mode-free per the lazy-materialisation rule.
//! - `ProjectPath` — path-precise projection rooted at `base` walking each
//!   [`PathSegment`]. The empty-path form is the canonical shape of
//!   "expand the whole surface" and supersedes the retired `Expand`
//!   variant. `ProjectMember { base, member, mode }` and
//!   `IndexedAccess { base, index, mode }` admission-canonicalise to the
//!   length-1 `ProjectPath` form **before** memo hashing so sugar and
//!   canonical share one warm entry and one in-flight wait graph (plan
//!   B1a).
//! - `NormalizeUnion` / `NormalizeIntersection` — structural dedup over the
//!   supplied members with stable ordering.
//! - `KeyOf` / `MappedType` / `Conditional` — navigation operations that
//!   walk the base node's shared-graph payload. Paths that do not reach a
//!   concrete node fall through to a recorded `Opaque(Miss)` entry — this
//!   is distinct from a dispatch miss because the warm entry observes the
//!   base node identity and therefore dedups repeated asks.
//! - `TypeOf` — mirrors `ResolveDecl` but routes through the shallow
//!   value-symbol space.
//!
//! Every variant observes a dep-signature fragment (at minimum the project
//! generation; additional file hashes flow in from the base nodes' origin
//! scopes where the dispatcher can observe them). Dep-signature propagation
//! is what the publish-side completion-fence revalidation relies on for
//! transitive final-result validation.
//!
//! ## Design rules
//!
//! - Navigators stay non-owning: new semantic nodes must enter through
//!   [`SemanticQueryApi::execute`], not through ad-hoc helpers on the
//!   dispatcher.
//! - Errors, partial results, and recursion sentinels never promote to warm
//!   memo entries — the underlying [`SemanticGraphStore`] enforces this
//!   invariant at publish time.
//! - Dep-signatures returned from warm hits must fold into the caller's
//!   dependency-fact set so the publish-side completion-fence
//!   revalidation stays transitive.

use std::sync::Arc;

use verter_semantic::analysis::type_solver::host::{
    BareRefOrigin, ResolvedRootIdentity, UtilitySource,
};
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use crate::resolver_core::{BudgetDomain, BudgetExceededFailure, ResolverContext};
use crate::semantic_query::{
    BranchSelection, CacheRead, DeclIdentity, DepSignature, DepVersion, IndexKey, LiteralValue,
    NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind, ProjectionMode,
    QueryError, QueryResult, ResolveDeclKey, ResultProvenance, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    SurfaceView,
};
use crate::semantic_query_memo::SemanticGraphStore;
use verter_type_expr::{PrimitiveName, TypeExpr};

// Module tree. The sub-modules are `pub(crate)` so external callers see only
// the `ProjectSemanticDispatch` struct / trait impl, while each module owns one
// concern of the dispatcher and shares private accessors through `impl` blocks:
//   - `build`     — the `build_*` builders for every query variant that
//                   produces a new `SemanticNodeId` (`build_resolve_decl`,
//                   `build_typeof`, `build_instantiate`, `build_class_surface`,
//                   `build_resolve_macro_payload`, …).
//   - `lower` / `raise` — `TypeExpr` ⇄ `SemanticNodeId` structural conversion.
//   - `walk`      — path-walking + the non-recursive shallow-mode terminal
//                   surface synthesiser.
//   - `enumerate` — `keyof` member-name enumeration helpers.
//   - `evaluate`  — the deferred-shell fix-point evaluation loop.
//   - `substitute`— generic type-parameter substitution into the graph.
//   - `relation`  — the authoritative semantic-node assignability engine.
// `mod.rs` retains the dispatch entry points and shared dispatcher state.
pub(crate) mod absorb;
pub(crate) mod build;
pub(crate) mod enumerate;
pub(crate) mod evaluate;
pub(crate) mod lower;
pub(crate) mod raise;
pub(crate) mod relation;
pub(crate) mod relation_predicates;
pub(crate) mod substitute;
pub(crate) mod walk;

/// Declaration identity used for in-flight instantiation tracking
/// ( recursive-ref back-edge detection during body
/// lowering). `(canonical_id, name)` tuple keyed on refcount-shared
/// `Arc<str>` so membership checks are refcount compares, not string
/// compares. Lives on [`ProjectSemanticDispatch`] so nested
/// `build_instantiate` invocations share the active set.
pub(super) type InstantiateIdentity = (Arc<str>, Arc<str>);

/// Host-bound dispatcher for [`SemanticQueryApi`].
///
/// The dispatcher borrows the host for the duration of a query — every
/// `execute()` call threads through the host's
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore) and its
/// owned [`SemanticGraphStore`].
///
/// `instantiate_active` records every declaration identity currently being
/// materialised by a `build_instantiate` call chain rooted at this
/// dispatcher. When `shallow_lower_type_expr` encounters a
/// `TypeExpr::Ref` whose resolved root matches an active identity, it
/// emits an `Opaque(RecursiveRef)` back-edge immediately — no further
/// dispatch, no deep recursion into the body a second time. This is the
/// dispatch-side analogue of the retired solver's `RecursionTracker`
/// and closes the stack-bound recursion hole Session 4 traced down to
/// the `type TreeNode = { children: TreeNode[] }` materialisation path.
pub struct ProjectSemanticDispatch<'a> {
    pub(super) ctx: &'a dyn ResolverContext,
    pub(super) instantiate_active: std::cell::RefCell<smallvec::SmallVec<[InstantiateIdentity; 8]>>,
    /// Cold-build-LOCAL taint accumulator stack (universal read-boundary
    /// fold).
    ///
    /// One frame is pushed per cold build that actually runs its build
    /// closure (the `execute_via_cold_build_helper` `traced_build` path),
    /// and popped + OR-folded into the build's `QueryBuildOutput` before
    /// memo admission. While a frame is on the stack, EVERY cold-build
    /// subquery read folds its discarded `CacheRead` metadata into the TOP
    /// frame at the shared read boundary
    /// ([`Self::execute_via_cold_build_helper`], the single funnel for
    /// `execute`, `execute_read`, and every direct `execute_read`
    /// cold-build consumer in build / evaluate / enumerate / walk):
    /// `read.result_is_partial` into `result_is_partial`, AND
    /// `read.cache_suppress` into `cache_suppress`. This is the SINGLE
    /// place the partial/non-cacheable read-leak class is closed — a
    /// nested partial/non-cacheable read can no longer drop its metadata
    /// before the outer build's warm-gate decision, regardless of which
    /// helper issued it. A top-level read with no active build frame
    /// naturally no-ops (the stack is empty).
    ///
    /// Distinct from the broad per-request sticky
    /// ([`crate::request_context::current_materialization_cache_suppress`]):
    /// the request sticky records ONLY genuine partials (§1) and
    /// outlives the build; this stack is scoped to the active cold build so
    /// a benign non-cacheable nested read taints only THIS build's
    /// `cache_suppress` (memo non-admission), never the request partial
    /// sticky (which would wrongly refuse component-meta warm).
    pub(super) build_local_taint: std::cell::RefCell<smallvec::SmallVec<[BuildLocalTaint; 8]>>,
}

/// One cold-build-local taint frame: the OR-accumulator a single cold
/// build folds every nested read's `result_is_partial` / `cache_suppress`
/// into before its memo-admission decision. See
/// [`ProjectSemanticDispatch::build_local_taint`].
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BuildLocalTaint {
    /// OR of every nested read's `result_is_partial` observed while this
    /// frame was the top of the build-local stack. A genuine partial.
    pub(super) result_is_partial: bool,
    /// OR of every nested read's `cache_suppress` observed while this
    /// frame was the top of the build-local stack. Inner-memo
    /// non-cacheability — benign or partial-derived.
    pub(super) cache_suppress: bool,
}

/// Panic-safe RAII guard for a cold-build-local taint frame.
///
/// On construction it pushes a fresh [`BuildLocalTaint`] frame onto the
/// dispatch's [`ProjectSemanticDispatch::build_local_taint`] stack; on
/// drop it pops that frame UNLESS the frame was already consumed via
/// [`Self::finish`]. This mirrors [`crate::semantic_query_memo::InflightPanicGuard`]:
/// a manual `push` / `pop` pair
/// is not unwind-safe — if the cold-build closure (or the relation cold
/// compute) panics or returns early between the push and the pop, the
/// frame leaks and every subsequent read on this dispatcher folds into a
/// stale frame, corrupting later builds' memo-admission decisions.
///
/// Normal path: the owner calls [`Self::finish`], which pops the frame and
/// returns it for the OR-fold into the build's `QueryBuildOutput` (or the
/// relation's non-admission decision), marking the guard finished so
/// `drop` is a no-op. Panic / early-return path: `drop` pops the frame so
/// the stack stays balanced.
pub(super) struct BuildLocalTaintGuard<'g> {
    stack: &'g std::cell::RefCell<smallvec::SmallVec<[BuildLocalTaint; 8]>>,
    finished: bool,
}

impl<'g> BuildLocalTaintGuard<'g> {
    /// Push a fresh taint frame and return a guard that pops it on drop.
    pub(super) fn push(
        stack: &'g std::cell::RefCell<smallvec::SmallVec<[BuildLocalTaint; 8]>>,
    ) -> Self {
        stack.borrow_mut().push(BuildLocalTaint::default());
        Self {
            stack,
            finished: false,
        }
    }

    /// Normal-path completion: pop this frame and return it for the
    /// OR-fold, marking the guard finished so `drop` becomes a no-op.
    /// Returns a default (untainted) frame if the stack is somehow empty.
    pub(super) fn finish(mut self) -> BuildLocalTaint {
        self.finished = true;
        self.stack.borrow_mut().pop().unwrap_or_default()
    }
}

impl<'g> Drop for BuildLocalTaintGuard<'g> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Panic / early-return path: pop the frame this guard pushed so the
        // stack stays balanced. The popped frame is discarded — the build
        // that owned it is unwinding, so its memo admission never happens.
        self.stack.borrow_mut().pop();
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Create a dispatcher bound to `ctx`.
    ///
    /// Locked-in signature: takes `&dyn ResolverContext`,
    /// not concrete `&VerterHost`. External callers (test fixtures and
    /// `component_meta_materialize.rs`) pass `&host` directly; the
    /// implicit `&host as &dyn ResolverContext` upcast handles
    /// type-erasure at the call site because `impl ResolverContext for
    /// VerterHost` is registered in `resolver_core/resolver_context.rs`.
    #[must_use]
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        // Bump `bare_engine_constructions` when this dispatcher is
        // bound to a non-request-bound ctx. Nested
        // `ProjectSemanticDispatch` ctors inherit their ctx from the
        // enclosing `ComponentMetaQueryEngine`, so they typically do
        // NOT bump (the engine's ctx flows through); but a bare-host
        // direct `ProjectSemanticDispatch::new(host)` construction
        // (e.g. a future cold entry-point regression)
        // would bump here.
        if !ctx.is_request_bound() {
            crate::request_context::bump_bare_engine_construction();
        }
        Self {
            ctx,
            instantiate_active: std::cell::RefCell::new(smallvec::SmallVec::new()),
            build_local_taint: std::cell::RefCell::new(smallvec::SmallVec::new()),
        }
    }

    /// Fold a discarded nested-read's metadata into the TOP cold-build-local
    /// taint frame (universal read-boundary fold). Called by the shared read
    /// boundary [`Self::execute_via_cold_build_helper`] for EVERY cold-build
    /// subquery read (so `execute`, `execute_read`, the `execute_type_node`
    /// override, and every direct `execute_read` cold-build consumer fold
    /// identically) before the `CacheRead` rails are discarded by a
    /// value-only caller. No-op when no frame is on the stack (no active cold
    /// build — e.g. a top-level read from the projector, which routes
    /// partiality through the request sticky instead).
    pub(super) fn fold_into_top_build_local_taint(
        &self,
        result_is_partial: bool,
        cache_suppress: bool,
    ) {
        if let Some(top) = self.build_local_taint.borrow_mut().last_mut() {
            top.result_is_partial |= result_is_partial;
            top.cache_suppress |= cache_suppress;
        }
    }

    /// Fold a returning `CacheRead`'s rails into BOTH propagation channels —
    /// the per-request partial sticky AND the active build-local taint frame.
    /// This is the SINGLE fold funnel `execute_via_cold_build_helper` invokes
    /// at EVERY one of its return points (the post-cooperative normal tail AND
    /// the post-trip budget early-exit), so no exit can launder a partial /
    /// non-cacheable nested read past the enclosing build's warm-gate decision:
    ///   - `result_is_partial` ⟹ mark the request sticky (a genuine partial
    ///     must gate component-meta / shape / materialize warm for the whole
    ///     request) AND fold into the build-local frame.
    ///   - `cache_suppress` ⟹ fold into the build-local frame only (a benign
    ///     non-cacheable but COMPLETE nested read taints the enclosing build's
    ///     memo non-admission, never the request partial sticky).
    ///
    /// A top-level read with no active build frame naturally no-ops the
    /// build-local fold (the stack is empty); the sticky mark still fires.
    pub(super) fn fold_cache_read_rails(&self, result_is_partial: bool, cache_suppress: bool) {
        if result_is_partial {
            crate::request_context::mark_request_materialization_cache_suppress();
        }
        self.fold_into_top_build_local_taint(result_is_partial, cache_suppress);
    }

    /// Bump the per-request type-resolution audit counters (hop / mode /
    /// projection-op / conditional-decision + walker-depth high-water) for a
    /// CONSUMER dispatch `key`, BEFORE admission-time rewriting so the
    /// attribution reflects the caller's intent (path-projection sugar still
    /// counts as one hop).
    ///
    /// Shared by BOTH consumer entry points — [`SemanticQueryApi::execute`]
    /// and the chokepoint [`SemanticQueryApi::execute_type_node`] override —
    /// so a `execute_type_node` caller keeps counting after the override
    /// switched it from the value-only `execute` to `execute_read` (which
    /// does NOT bump these). The per-request dispatch-MASK trace
    /// (`record_dispatched_query_tag`) stays at the `execute_via_cold_build_helper`
    /// choke point (so nested `execute_read`-only sub-dispatches are captured
    /// there); these intent counters stay on the consumer entry. No-op when
    /// no `RequestContext` is installed on the calling thread.
    fn record_dispatch_intent_counters(&self, key: &SemanticQueryKey) {
        let Some(ctx) = crate::request_context::current_request_context() else {
            return;
        };
        let depth = self.instantiate_active.borrow().len();
        ctx.observe_type_resolution_depth(u16::try_from(depth).unwrap_or(u16::MAX));
        match key {
            SemanticQueryKey::ProjectPath { context, .. } => {
                ctx.bump_type_resolution_hop(context.mode);
                ctx.bump_type_resolution_projection_op();
            }
            SemanticQueryKey::ProjectMember { mode, .. }
            | SemanticQueryKey::IndexedAccess { mode, .. } => {
                ctx.bump_type_resolution_hop(*mode);
                ctx.bump_type_resolution_projection_op();
            }
            SemanticQueryKey::Instantiate { context, .. } => {
                ctx.bump_type_resolution_hop(context.projection_reduction.mode);
            }
            SemanticQueryKey::Conditional { .. } => {
                ctx.bump_type_resolution_conditional_decision();
                ctx.bump_type_resolution_hop(crate::semantic_query::ProjectionMode::Identity);
            }
            // Every other kind is a single dispatched hop with no
            // consumer-visible projection mode — count as Identity.
            _ => {
                ctx.bump_type_resolution_hop(crate::semantic_query::ProjectionMode::Identity);
            }
        }
    }

    /// Build the env-bearing, content-free type-space
    /// [`ResolvedDeclSlotIdentity`](crate::semantic_query::ResolvedDeclSlotIdentity)
    /// for the declaration `name` defined in `canonical`.
    ///
    /// This is the SINGLE production derivation point for an
    /// `Instantiate` / `ResolveMacroPayload` base/owner slot: it reads
    /// the DEFINING file's per-canonical env
    /// (`type_env_hash` = `T`, `lib_env_hash` = `L`) and folds the project
    /// identity (`J`) from `host_view_project_identity_for`, building the
    /// slot DIRECTLY (no `DeclKey` ↔ slot adapter). `symbol_space` is
    /// always `Type` — an `Instantiate` base / macro owner is a
    /// type-space carrier (interface / type alias / class-type / builtin
    /// utility / synthetic SFC owner). The slot stays content-free (R6);
    /// the file content version is re-sourced at value-compute time.
    #[must_use]
    pub(crate) fn type_slot_for(
        &self,
        canonical: Arc<str>,
        name: Arc<str>,
    ) -> crate::semantic_query::ResolvedDeclSlotIdentity {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical.as_ref());
        let project_identity = host
            .host_view_project_identity_for(canonical.as_ref())
            .fold_u32();
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot(
            canonical,
            name,
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        )
    }

    /// Build the env-bearing type-space slot for a built-in utility
    /// carrier (`Pick` / `Omit` / `Required` / `Partial` / …). The
    /// defining canonical is the `"__builtin__"` sentinel; builtins root
    /// self-version through their `args` nodes (no file fact), and their
    /// env dims come from the workspace-default env (the `"__builtin__"`
    /// canonical has no owning project).
    #[must_use]
    pub(crate) fn builtin_type_slot(
        &self,
        name: &str,
    ) -> crate::semantic_query::ResolvedDeclSlotIdentity {
        self.type_slot_for(Arc::from("__builtin__"), Arc::from(name))
    }

    /// The `resolve_env_hash` (`R`) dimension for a declaration defined in
    /// `canonical` — the extra env dim carried by
    /// [`InstantiateContext`](crate::semantic_query::InstantiateContext) /
    /// [`MacroPayloadContext`](crate::semantic_query::MacroPayloadContext)
    /// beyond the slot's `T` / `L` / `J`.
    #[must_use]
    pub(crate) fn resolve_env_hash_for(&self, canonical: &str) -> crate::semantic_query::HashValue {
        self.ctx
            .host_for_fact_tracer_install()
            .host_view_env_hashes_for(canonical)
            .resolve_env_hash
    }

    /// Build the [`InstantiateContext`](crate::semantic_query::InstantiateContext)
    /// for a base defined in `canonical` from the projection-reduction
    /// context `prc` plus the base canonical's `resolve_env_hash`.
    #[must_use]
    pub(crate) fn instantiate_context_for(
        &self,
        canonical: &str,
        prc: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::semantic_query::InstantiateContext {
        crate::semantic_query::InstantiateContext::new(prc, self.resolve_env_hash_for(canonical))
    }

    /// Build the [`MacroPayloadContext`](crate::semantic_query::MacroPayloadContext)
    /// for an owner defined in `canonical` from the owner canonical's
    /// `resolve_env_hash` plus the projection `mode`.
    #[must_use]
    pub(crate) fn macro_payload_context_for(
        &self,
        canonical: &str,
        mode: crate::semantic_query::ProjectionMode,
    ) -> crate::semantic_query::MacroPayloadContext {
        crate::semantic_query::MacroPayloadContext::new(self.resolve_env_hash_for(canonical), mode)
    }

    /// Build the full [`SemanticQueryKey::TypeOf`] key for the value root
    /// `root` under the projection-reduction context `prc`.
    ///
    /// This is the SINGLE production derivation point for a `TypeOf`
    /// key: it reads the value-root scope canonical's per-canonical env
    /// (`type_env_hash` = `T`, `lib_env_hash` = `L`), folds the project
    /// identity (`J`) from `host_view_project_identity_for` onto the
    /// env-bearing [`ValueRootSlotIdentity`](crate::semantic_query::ValueRootSlotIdentity),
    /// and rides `resolve_env_hash` (`R`) on the dedicated
    /// [`TypeOfContext`](crate::semantic_query::TypeOfContext) — mirror
    /// of [`Self::type_slot_for`] + [`Self::instantiate_context_for`].
    /// The slot stays content-free (R6); the file content version is
    /// re-sourced at value-compute time.
    #[must_use]
    pub(crate) fn typeof_key_for(
        &self,
        root: crate::semantic_query::ValueRootKey,
        prc: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticQueryKey {
        let host = self.ctx.host_for_fact_tracer_install();
        let canonical = root.scope.canonical_id.as_ref();
        let env = host.host_view_env_hashes_for(canonical);
        let project_identity = host.host_view_project_identity_for(canonical).fold_u32();
        SemanticQueryKey::TypeOf {
            value_root: crate::semantic_query::ValueRootSlotIdentity::new(
                root,
                project_identity,
                env.type_env_hash,
                env.lib_env_hash,
            ),
            context: crate::semantic_query::TypeOfContext::new(prc, env.resolve_env_hash),
        }
    }

    /// Push `identity` onto the active-instantiation stack. Returns `true`
    /// when the identity was not already present (caller MUST pair with
    /// `pop_instantiate_active` on the same identity). Returns `false`
    /// when the identity is already active — caller should emit
    /// `Opaque(RecursiveRef)` and must NOT pop.
    pub(super) fn push_instantiate_active(&self, identity: InstantiateIdentity) -> bool {
        let mut active = self.instantiate_active.borrow_mut();
        if active.iter().any(|existing| {
            existing.0.as_ref() == identity.0.as_ref() && existing.1.as_ref() == identity.1.as_ref()
        }) {
            return false;
        }
        active.push(identity);
        true
    }

    /// Pop the most-recent active-instantiation entry. Caller MUST only
    /// call this after a successful `push_instantiate_active`.
    pub(super) fn pop_instantiate_active(&self) {
        let mut active = self.instantiate_active.borrow_mut();
        active.pop();
    }

    /// Check whether `identity` is currently being instantiated on this
    /// dispatcher's call chain without taking ownership.
    pub(super) fn is_instantiate_active(&self, canonical_id: &str, name: &str) -> bool {
        let active = self.instantiate_active.borrow();
        active.iter().any(|(existing_canonical, existing_name)| {
            existing_canonical.as_ref() == canonical_id && existing_name.as_ref() == name
        })
    }

    pub(super) fn graph(&self) -> &Arc<SemanticGraphStore> {
        self.ctx.project_type_store().semantic_graph()
    }

    /// Integration-test shim for
    /// [`Self::substitute_semantic_type_param`]. The substitute
    /// helper is `pub(super)` and unreachable from integration
    /// test crates; this method forwards the call so the hash-cons
    /// discriminator tests can verify the memo with controlled
    /// input triples. Crate-public to keep the visibility scope as
    /// narrow as possible.
    ///
    /// `cfg`-gated to `test` / `debug_assertions`: the method is
    /// absent from release builds, so the production crate surface
    /// is unchanged.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn substitute_semantic_type_param_for_tests(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        self.substitute_semantic_type_param(node, parameter_node, arg)
    }

    /// Integration-test shim for
    /// [`Self::evaluate_deferred_semantic_node_with_context`]. Same
    /// rationale as `substitute_semantic_type_param_for_tests`:
    /// the helper is `pub(super)` and unreachable from integration
    /// test crates.
    ///
    /// `cfg`-gated to `test` / `debug_assertions`: the method is
    /// absent from release builds, so the production crate surface
    /// is unchanged.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn evaluate_deferred_semantic_node_with_context_for_tests(
        &self,
        node: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.evaluate_deferred_semantic_node_with_context(node, context)
    }

    /// Intern an opaque node carrying the supplied query error. Used as the
    /// fallback when a semantic subquery cannot be satisfied but the caller
    /// wants a node id rather than a top-level error.
    pub(super) fn opaque(&self, err: QueryError) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::Opaque(err))
    }

    /// Build the dep-signature fragment for a canonical file at a given
    /// content hash. Carries both the file-version fact and the current
    /// project generation so the completion fence picks up both.
    pub(super) fn dep_signature_for(
        &self,
        canonical_id: &Arc<str>,
        hash: [u8; 16],
    ) -> DepSignature {
        let project_gen = self.ctx.project_type_store().project_generation();
        Arc::from(
            vec![
                (canonical_id.clone(), DepVersion::WholeHash(hash)),
                (
                    canonical_id.clone(),
                    DepVersion::ProjectGeneration(project_gen),
                ),
            ]
            .into_boxed_slice(),
        )
    }

    /// Build a dep-signature fragment that records only the project
    /// generation. Used by derived semantic operations (e.g. `Instantiate`,
    /// `NormalizeUnion`) where no single canonical scope owns the result —
    /// dep signatures flow in through the warm memo hits of the bases the
    /// caller already supplied.
    pub(super) fn project_generation_signature(&self) -> DepSignature {
        let project_gen = self.ctx.project_type_store().project_generation();
        Arc::from(
            vec![(
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(project_gen),
            )]
            .into_boxed_slice(),
        )
    }

    /// Emit a `ProjectMember` origin-graph edge tagged
    /// `MemberEdgeProvenance::PublishedField` at the moment a
    /// component-meta projector admits `member` onto the user-visible
    /// surface (`defineProps` / `defineEmits` / `defineSlots` /
    /// `defineExpose` / `defineModel`).
    ///
    /// This is the production emit site that gives the Rule-5
    /// compliance validator its enforcement teeth. The validator
    /// inspects every `PublishedField` edge against the declared
    /// published surface — a `PublishedField` edge naming an
    /// off-surface member is a Rule-5 leak. Without this emit, the
    /// validator's `PublishedField` branch is dead code on real
    /// corpus data and 179/179 PASS is vacuous.
    ///
    /// `parent_surface` is the semantic node id of the macro
    /// payload's expanded surface (or the payload itself for the
    /// single-field `defineModel` projector). `member_value` is the
    /// semantic node id of the member's type body. `name` is the
    /// published member name as it appears on the user-visible
    /// surface — for `defineModel` this is the resolved model name
    /// (defaulting to `modelValue`).
    pub(crate) fn record_published_field_edge(
        &self,
        owner: &DeclIdentity,
        parent_surface: SemanticNodeId,
        member_value: SemanticNodeId,
        name: &Arc<str>,
    ) {
        let fence = self.dep_signature_for(&owner.canonical_id, owner.whole_hash);
        self.graph().record_origin_edge(
            member_value,
            OriginEdgeKind::ProjectMember,
            Arc::from(vec![parent_surface].into_boxed_slice()),
            OriginMeta::ProjectedMember {
                name: Arc::clone(name),
                provenance: verter_audit::MemberEdgeProvenance::PublishedField,
            },
            fence,
        );
    }

    /// Mode-aware lowering — the sole lowering entry point.
    ///
    /// The implicit `lower_type_expr_in_scope`
    /// wrapper that defaulted to `ProjectionMode::Expanded` is
    /// retired. Every caller MUST state mode explicitly:
    ///
    /// - Intermediate base lowering (the result is fed into a
    ///   subsequent `ProjectPath { base, .., mode }` dispatch) — use
    ///   `Navigate`. Lowering the base in `Expanded` eagerly reduces
    ///   `keyof T` / `MappedType<T>` operators at the lowering site
    ///   even when the terminal demand is shallow.
    /// - True full-expansion call sites that read the lowered node
    ///   as the published result (no path-walking follow-up) — pass
    ///   `Expanded` explicitly.
    ///
    /// The audit footprint regression (ChatMessages cold-seq
    /// `outputSchema|execute = 62`) traced to the implicit-Expanded
    /// default surviving on intermediate-base lowering sites; the
    /// fix is rule-3 of the demand-driven reducer spec.
    pub fn lower_type_expr_in_scope_with_mode(
        &self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        mode: crate::semantic_query::ProjectionMode,
    ) -> Option<SemanticNodeId> {
        self.lower_type_expr_in_scope_with_context(
            scope_canonical_id,
            expr,
            crate::semantic_query::ProjectionReductionContext::published(mode),
        )
    }

    /// Context-explicit lowering — demand-aware sibling of
    /// [`Self::lower_type_expr_in_scope_with_mode`]. The macro
    /// publication boundary lowers the slot/object carrier with
    /// `structural_transit_with_mode(Navigate)`; the transit demand
    /// propagates through every nested operator dispatch
    /// (`Instantiate` / `KeyOf` / `MappedType`) which carrier-stops
    /// via `may_reduce_operator(context) == false`. The publication
    /// terminal then walks the structural shell under
    /// `Published(Shallow)` to synthesise the one-level surface.
    ///
    /// `_with_mode(scope, expr, m)` ≡
    /// `_with_context(scope, expr, published(m))` — existing
    /// publication callers keep their semantics unchanged.
    pub fn lower_type_expr_in_scope_with_context(
        &self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        let shallow = self.ctx.shallow_file_state(scope_canonical_id)?;
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(scope_canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        let env = rustc_hash::FxHashMap::default();
        let name_resolution = rustc_hash::FxHashMap::default();
        let scope_payload = self
            .ctx
            .prepared_decl_bundle(scope_canonical_id)
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        // R15/F11 — construct the resolver-context
        // `ScopeShadowing` once at the dispatch entry point. The
        // shadow set is derived from the same `scope_payload` the
        // foundation (`524f469d`) consumed inline; any future
        // shadow-source addition lands in `ScopeShadowing::from_*`
        // rather than at every call site.
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        let id = self.shallow_lower_type_expr_with_context(
            expr,
            &env,
            &scope,
            &name_resolution,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );
        Some(id)
    }
}

pub(super) fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Test-only fact-injection slot for the dispatch's
/// `install_fact_tracer`-wrapped cold-build closure. When `Some`,
/// `dispatch_test_inject_parse_fact_if_set` observes the recorded
/// `Parse(...)` fact onto the active tracer BEFORE the inner build
/// runs, exercising the cold-publish → warm-hit path-precise fact
/// survival contract without a workspace fixture that organically
/// emits `Parse` observations through the resolver substrate.
///
/// The slot uses a `Relaxed` atomic flag for the fast-path
/// no-injection check (~1 ns per cold build) and the
/// [`std::sync::Mutex`] only when the flag is set. Production traffic
/// reads only the atomic; the mutex stays cold under normal load.
#[doc(hidden)]
pub(crate) static DISPATCH_TEST_INJECT_PARSE_FACT_ARMED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[doc(hidden)]
pub(crate) static DISPATCH_TEST_INJECT_PARSE_FACT: std::sync::Mutex<
    Option<crate::resolver_core::FactVersionRef>,
> = std::sync::Mutex::new(None);

/// Observe the injected `Parse(...)` fact onto every active tracer
/// when the slot is armed. Fast path: single relaxed atomic load when
/// the slot is unarmed (production traffic never takes the mutex).
#[inline]
pub(crate) fn dispatch_test_inject_parse_fact_if_set() {
    if !DISPATCH_TEST_INJECT_PARSE_FACT_ARMED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let injected = {
        let guard = DISPATCH_TEST_INJECT_PARSE_FACT.lock().unwrap();
        guard.clone()
    };
    if let Some(fact) = injected {
        crate::resolver_core::resolver_context::observe_fan_out(fact);
    }
}

/// RAII guard for the test-only Parse-fact injection slot. Sets the
/// fact on arm; clears on drop. Concurrent tests must serialise via a
/// shared `Mutex` because the slot is process-global.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub struct DispatchInjectParseFactGuard;

#[cfg(any(test, debug_assertions))]
impl DispatchInjectParseFactGuard {
    /// Arm the dispatch's test-only Parse-fact injection slot with
    /// `fact`. The next cold build observes `fact` onto every active
    /// tracer before the inner build runs. The returned guard clears
    /// the slot on drop.
    pub fn arm(fact: crate::resolver_core::FactVersionRef) -> Self {
        {
            let mut slot = DISPATCH_TEST_INJECT_PARSE_FACT.lock().unwrap();
            *slot = Some(fact);
        }
        DISPATCH_TEST_INJECT_PARSE_FACT_ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for DispatchInjectParseFactGuard {
    fn drop(&mut self) {
        DISPATCH_TEST_INJECT_PARSE_FACT_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut slot = DISPATCH_TEST_INJECT_PARSE_FACT.lock().unwrap();
        *slot = None;
    }
}

/// Tri-state result of [`ProjectSemanticDispatch::shallow_relation_check`].
/// The relation authority is [`ProjectSemanticDispatch::relate_nodes`];
/// this enum only carries the hot-path fast-decision cases handled
/// inline by `build_conditional` before falling through to the full
/// engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShallowRelation {
    Assignable,
    NotAssignable,
    Unknown,
}

/// Tri-state outcome of
/// [`ProjectSemanticDispatch::conditional_branch_selection`] — the ONE
/// shared conditional branch-selection oracle, factored out of
/// `build_conditional`'s relation path (the pre-relation infer-pattern
/// cases, then `shallow_relation_check`, then the full memoised
/// `relate_nodes` engine) and reused by the key-domain closedness
/// classifiers in `raise.rs` for selected-branch-only classification.
/// `Deferred` covers a genuinely undecidable relation (`Unknown`) AND
/// the lattice-extreme checks that semantically use both branches
/// (`any`) or dominate (`error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConditionalBranchSelection {
    True,
    False,
    Deferred,
}

/// Payload of a PRE-RELATION infer-pattern branch selection
/// ([`ProjectSemanticDispatch::pre_relation_infer_selection`]) — the
/// structural cases that select the TRUE branch before any relation
/// query, owned by the shared oracle so `build_conditional`'s reduction
/// and the `raise.rs` closedness classifiers cannot diverge on them.
/// The payload carries what the selection BINDS: `build_conditional`
/// substitutes the bindings into the true branch; the classifiers bind
/// the branch's infer names to the same check-derived identities.
#[derive(Debug, Clone)]
pub(super) enum InferPatternSelection {
    /// `check extends infer X ? T : F` — an infer pattern matches
    /// anything ⇒ TRUE selected with `X := check`, for ANY check.
    BareInfer { name: Arc<str> },
    /// `check extends (x: infer U, …) => infer R ? T : F` — TRUE
    /// selected with the positional `name := node` bindings extracted
    /// from the materialised check signature (params zip + return).
    FunctionInfer {
        bindings: Vec<(Arc<str>, SemanticNodeId)>,
    },
}

/// Map a [`PrimitiveName`] from the parser's IR onto the semantic-graph
/// [`PrimitiveKind`]. Kept colocated with `build_instantiate` because no
/// other dispatch builder produces primitive nodes from `TypeExpr`.
pub(super) fn map_primitive_name(name: PrimitiveName) -> PrimitiveKind {
    match name {
        PrimitiveName::String => PrimitiveKind::String,
        PrimitiveName::Number => PrimitiveKind::Number,
        PrimitiveName::Boolean => PrimitiveKind::Boolean,
        PrimitiveName::Symbol => PrimitiveKind::Symbol,
        PrimitiveName::BigInt => PrimitiveKind::BigInt,
        PrimitiveName::Any => PrimitiveKind::Any,
        PrimitiveName::Unknown => PrimitiveKind::Unknown,
        PrimitiveName::Void => PrimitiveKind::Void,
        PrimitiveName::Never => PrimitiveKind::Never,
        PrimitiveName::Null => PrimitiveKind::Null,
        PrimitiveName::Undefined => PrimitiveKind::Undefined,
        PrimitiveName::Object => PrimitiveKind::Object,
    }
}

/// Type-parameter names for built-in TS utilities, ordered positionally.
///
/// Origin edges on utility results carry these names so they match the
/// names a userland-equivalent alias would emit. `Partial<T>` gets
/// `["T"]` — not a synthesised `["T0"]` — so origin walks on
/// `Partial<T>` match `type MyPartial<T> = ...` byte-for-byte.
///
/// See the utility-equivalence rule.
pub(super) fn utility_param_names(name: &str) -> &'static [&'static str] {
    match name {
        "Partial"
        | "Required"
        | "Readonly"
        | "NonNullable"
        | "NoInfer"
        | "ReturnType"
        | "Parameters"
        | "ConstructorParameters"
        | "InstanceType"
        | "Uppercase"
        | "Lowercase"
        | "Capitalize"
        | "Uncapitalize" => &["T"],
        "Awaited" => &["P"],
        "Pick" | "Omit" => &["T", "K"],
        "Record" => &["K", "V"],
        "Extract" | "Exclude" => &["T", "U"],
        _ => &[],
    }
}

// Literal identity is preserved through the semantic graph by
// `SemanticNodeData::Literal(LiteralValue)`, which carries the precise
// literal value. Callers that need the primitive kind of a literal
// should match on `SemanticNodeData::Literal` and derive the kind
// inline rather than collapsing the literal to its underlying primitive.

/// Stable-sort + dedup a list of semantic node ids. Used to canonicalize
/// the key surface for union / intersection dispatches so
/// `NormalizeUnion({A,B})` and `NormalizeUnion({B,A})` converge on one
/// warm memo entry.
pub(super) fn canonicalize_node_list(members: &[SemanticNodeId]) -> Arc<[SemanticNodeId]> {
    let mut sorted: Vec<SemanticNodeId> = members.to_vec();
    sorted.sort_by_key(|id| id.0);
    sorted.dedup();
    Arc::from(sorted.into_boxed_slice())
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Shared cold-build entry point used by BOTH
    /// [`SemanticQueryApi::execute`] and [`Self::execute_read`].
    ///
    /// **Single-call-site invariant.** This method holds the only
    /// production `graph.execute_cooperative(...)` call site dispatched
    /// from `ProjectSemanticDispatch`. The architecture guard
    /// `dispatch_cold_build_has_one_call_site.rs` asserts this with a
    /// static scan that strips test files + `#[cfg(test)]` regions and
    /// counts matches. A second production call site would mean a
    /// second cold-build path slipped through bypassing the tracer.
    ///
    /// **Tracer scope.** The fact tracer is installed ONLY around the
    /// cold-build closure passed to `execute_cooperative`. Warm hits
    /// (when the slot is already populated) MUST NOT allocate a
    /// tracer — they short-circuit at the `try_warm_hit_fast_path`
    /// inside `execute_cooperative`. The closure here only runs on
    /// cold misses or when the prior winner aborted; the tracer cost
    /// is bounded by the cold-build cost it observes.
    ///
    /// **Build-output threading.** On `FactReadSetFinalise::Ok`, the
    /// self-version-rooted carrier is stored on
    /// `QueryBuildOutput.graph_carrier` so `warm_publish_one` records
    /// it verbatim onto the `MemoEntry`. On
    /// `FactReadSetFinalise::Overflow`, the build output is marked
    /// `cache_suppress = true` so the memo refuses to publish the
    /// entry — the caller cold-recomputes on the next request.
    //
    // arch-guard:single-execute-cooperative-call — the helper holds
    // the only production `graph.execute_cooperative(` call site. The
    // arch test parses `crates/verter_session/src/**/*.rs` (excluding
    // tests, stripping cfg(test) regions) and asserts exactly one
    // match.
    fn execute_via_cold_build_helper(
        &self,
        key: SemanticQueryKey,
    ) -> CacheRead<QueryResult<SemanticNodeId>> {
        // Per-request dispatch-mask trace. Record the INCOMING CALLER tag
        // (NOT the post-canonicalisation tag) so caller intent
        // (`ProjectMember` / `IndexedAccess`) is preserved, and record it
        // HERE — at the top of the shared cold-build choke point, before
        // admission-time key canonicalisation and before the budget
        // early-exit. Both `execute` (the `SemanticQueryApi` trait method)
        // and `execute_read` (the dep-signature-preserving subquery entry)
        // funnel through this helper, so the mask captures every
        // `SemanticQueryKey` variant dispatched anywhere during the request
        // — including nested reducer sub-dispatches that enter ONLY via
        // `execute_read` (e.g. the macro-payload `NormalizeIntersection`
        // reduction in `build.rs`, mapped-type `ProjectPath` member
        // projection). Idempotent per tag (sets one bit). No-op when no
        // `RequestContext` is installed on the calling thread.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.record_dispatched_query_tag(key.tag());
        }

        // Admission-time canonicalisation per plan B1a:
        //   - `ProjectMember { base, member, mode }` rewrites to
        //     `ProjectPath { base, path: [Member(member)], mode }` BEFORE the
        //     key is hashed into the memo, so sugar and canonical share one
        //     entry and one in-flight wait graph.
        //   - `IndexedAccess { base, index, mode }` rewrites the same way to
        //     `ProjectPath { base, path: [Index(index)], mode }`.
        //   - `NormalizeUnion` / `NormalizeIntersection` get structural
        //     member-list canonicalisation so `{A, B}` and `{B, A}` converge.
        // Other variants key off [`SemanticNodeId`]s that are already hashed
        // verbatim.
        let key = match key {
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Member(member)].into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(mode),
                }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Index(index)].into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(mode),
                }
            }
            SemanticQueryKey::NormalizeUnion { members } => SemanticQueryKey::NormalizeUnion {
                members: canonicalize_node_list(&members),
            },
            SemanticQueryKey::NormalizeIntersection { members } => {
                SemanticQueryKey::NormalizeIntersection {
                    members: canonicalize_node_list(&members),
                }
            }
            other => other,
        };

        // Post-trip fast-path early-exit. Once the request's
        // projection-op fuse has already tripped, every subsequent
        // projection-op query (MappedType / KeyOf / ProjectPath /
        // ProjectMember / IndexedAccess) entering the cooperative-
        // admission machinery would burn ~μs on the in-flight table
        // mutex, fact-tracer install, per-key warm probe, and joiner-
        // condvar entry path — only to have the build closure return
        // `BudgetExceeded` and the publish be suppressed. Empirically
        // observed on `ChatMessages.vue`: 253K post-trip MappedType
        // builds each averaging ~1ms in the materialisation lane, for
        // ~250s of pure dispatch overhead past the fuse point.
        //
        // The early-exit collapses every post-trip projection-op query
        // to a single peek + sentinel allocation, without ever entering
        // `execute_cooperative`. The published audit semantics are
        // preserved: the per-kind cold counter is bumped via the same
        // attribution arms used by the slow path, the
        // `BudgetExceeded(cache_suppress=true)` carrier is the same
        // sentinel the build closure would have produced, and
        // `failure.actual` continues to reflect the pre-trip executed
        // count (the peek is non-incrementing — see
        // [`RequestBudget::is_exhausted`]).
        //
        // The check is gated on `semantic_query_counts_toward_projection_budget`,
        // the aggregate work-budget gate: the projection operators PLUS
        // `Instantiate` / `Conditional` (the generic-expansion-storm
        // kinds) and the demand-bearing `TypeOf`. Kinds outside that
        // set (ResolveDecl, NormalizeUnion, …) bypass the early-exit —
        // their cost is not what the work budget bounds.
        if semantic_query_counts_toward_projection_budget(&key) {
            if let Some(budget) = crate::request_context::current_request_budget() {
                if budget.is_exhausted() {
                    let limit = budget.effective_projection_op_budget();
                    let failure = BudgetExceededFailure {
                        domain: BudgetDomain::ProjectionOperation,
                        limit,
                        actual: budget.projection_ops_executed_count() as u64,
                        context: format!("semantic-dispatch:post-trip:{key:?}"),
                    };
                    // Attribute the post-trip dispatch via the SAME
                    // per-kind cold counters the slow path bumps from
                    // `execute_via_cold_build_helper`'s post-cooperative
                    // attribution block. Without this the audit's
                    // `semantic_query_*_cold` rails would silently
                    // under-count post-trip dispatches once the
                    // early-exit lands, and bench attribution would lose
                    // the runaway signal.
                    if let Some(observer) = verter_audit::current_observer() {
                        use verter_audit::AuditEvent;
                        // The early-exit attribution must mirror EVERY kind
                        // the aggregate work-budget gate counts — including
                        // `Instantiate` and `Conditional`. Once those two
                        // count toward the budget they can reach this
                        // post-trip early-exit; omitting their cold-event
                        // arms here would silently under-count post-trip
                        // instantiate/conditional dispatches and lose the
                        // open-generic-storm signal in bench attribution.
                        let event = match &key {
                            SemanticQueryKey::MappedType { .. } => {
                                Some(AuditEvent::SemanticQueryMappedTypeCold)
                            }
                            SemanticQueryKey::KeyOf { .. } => {
                                Some(AuditEvent::SemanticQueryKeyOfCold)
                            }
                            SemanticQueryKey::ProjectPath { .. }
                            | SemanticQueryKey::ProjectMember { .. } => {
                                Some(AuditEvent::SemanticQueryProjectPathCold)
                            }
                            SemanticQueryKey::IndexedAccess { .. } => {
                                Some(AuditEvent::SemanticQueryIndexedAccessCold)
                            }
                            SemanticQueryKey::Instantiate { .. } => {
                                Some(AuditEvent::SemanticQueryInstantiateCold)
                            }
                            SemanticQueryKey::Conditional { .. } => {
                                Some(AuditEvent::SemanticQueryConditionalCold)
                            }
                            SemanticQueryKey::TypeOf { .. } => {
                                Some(AuditEvent::SemanticQueryTypeOfCold)
                            }
                            _ => None,
                        };
                        if let Some(event) = event {
                            observer.record_event(event);
                        }
                    }
                    // CRITICAL: the post-trip budget early-exit is a SECOND
                    // return point of this helper. It must fold its rails into
                    // the request sticky + active build-local frame BEFORE
                    // returning — identical to the post-cooperative normal tail
                    // — or a budget-tripped sub-read (whose rails a value-only
                    // caller like `execute_type_node` discards into
                    // `Opaque(Miss)`) would never taint the enclosing cold
                    // build, which would then admit a complete `Value`
                    // (`result_is_partial=false`) and launder the partial.
                    self.fold_cache_read_rails(true, true);
                    return CacheRead {
                        value: QueryResult::Error(QueryError::BudgetExceeded(failure)),
                        dep_signature: empty_signature(),
                        walker_diagnostics: Arc::from([]),
                        cache_suppress: true,
                        result_is_partial: true,
                    };
                }
            }
        }

        let graph = Arc::clone(self.graph());
        // Per-key recursion sentinel: when the memo detects same-path
        // re-entry on an `Instantiate` key, extract the decl name and
        // emit `Opaque(RecursiveRef { name })`. The materialiser
        // recognises `TypeExpr::RecursiveRef` as a leaf and stops
        // expansion, terminating recursive aliases like `type Tree =
        // { children: Tree[] }` without stack overflow.
        // Non-Instantiate re-entry falls back to `Opaque(Miss)`.
        let sentinel_key = key.clone();
        let sentinel = {
            let graph = Arc::clone(&graph);
            move || {
                if let SemanticQueryKey::Instantiate { base, .. } = &sentinel_key {
                    return graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                        name: Arc::clone(&base.merged_symbol_name),
                    }));
                }
                graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss))
            }
        };
        let key_for_build = key.clone();
        let raw_build = move || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            if semantic_query_counts_toward_projection_budget(&key_for_build) {
                if let Some(budget) = crate::request_context::current_request_budget() {
                    if budget.check_projection_op_count() {
                        let limit = budget.effective_projection_op_budget();
                        let failure = BudgetExceededFailure {
                            domain: BudgetDomain::ProjectionOperation,
                            limit,
                            actual: budget.projection_ops_executed_count() as u64,
                            context: format!("semantic-dispatch:{key_for_build:?}"),
                        };
                        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                            (
                                QueryResult::Error(QueryError::BudgetExceeded(failure)),
                                self.project_generation_signature(),
                            )
                                .into();
                        output.cache_suppress = true;
                        output.result_is_partial = true;
                        return output;
                    }
                }
            }
            match &key_for_build {
                SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
                SemanticQueryKey::TypeOf {
                    value_root,
                    context,
                } => self.build_typeof(&value_root.root, context.projection_reduction),
                SemanticQueryKey::Instantiate {
                    base,
                    args,
                    context,
                } => self.build_instantiate(base, args, *context),
                // `ProjectMember` / `IndexedAccess` are API sugar that
                // admission-time canonicalisation rewrites to
                // `ProjectPath` above. The build closure never observes
                // these variants on the rewritten key; the arms below
                // are pure exhaustiveness.
                SemanticQueryKey::ProjectMember { base, member, mode } => {
                    let path: Arc<[PathSegment]> =
                        Arc::from(vec![PathSegment::Member(Arc::clone(member))].into_boxed_slice());
                    let ctx = crate::semantic_query::ProjectionReductionContext::published(*mode);
                    self.build_project_path(*base, &path, ctx)
                }
                SemanticQueryKey::IndexedAccess { base, index, mode } => {
                    let path: Arc<[PathSegment]> =
                        Arc::from(vec![PathSegment::Index(index.clone())].into_boxed_slice());
                    let ctx = crate::semantic_query::ProjectionReductionContext::published(*mode);
                    self.build_project_path(*base, &path, ctx)
                }
                SemanticQueryKey::ProjectPath {
                    base,
                    path,
                    context,
                } => self.build_project_path(*base, path, *context),
                SemanticQueryKey::KeyOf { base, context } => self.build_key_of(*base, *context),
                SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                } => self.build_mapped_type(*source, mapper, *context),
                SemanticQueryKey::Conditional {
                    check,
                    extends,
                    true_branch,
                    false_branch,
                    distributive,
                } => self.build_conditional(
                    *check,
                    *extends,
                    *true_branch,
                    *false_branch,
                    *distributive,
                ),
                SemanticQueryKey::NormalizeUnion { members } => self.build_normalize_union(members),
                SemanticQueryKey::NormalizeIntersection { members } => {
                    self.build_normalize_intersection(members)
                }
                SemanticQueryKey::ResolvedNamedType { key } => {
                    self.build_resolved_named_type(key).into()
                }
                // The relation engine routes through its dedicated
                // `SemanticGraphStore::relation_memo` (keyed on the full
                // `RelateMemoKey`) via `relate_nodes`, not the family memo.
                // The family `execute` path for `Relate` is therefore
                // degenerate: it owns no relation logic and always yields
                // `Opaque(Miss)`, fenced on the project generation so a stale
                // miss never warms.
                SemanticQueryKey::Relate { .. } => {
                    let fence = self.project_generation_signature();
                    (QueryResult::Error(QueryError::Miss), fence).into()
                }
                SemanticQueryKey::ResolveMacroPayload {
                    owner,
                    macro_index,
                    macro_kind,
                    type_args,
                    context,
                } => self.build_resolve_macro_payload(
                    owner,
                    *macro_index,
                    *macro_kind,
                    type_args,
                    context.mode,
                ),
                // ResolveClassSurface — LIVE producer. Composes the
                // dual-space sub-queries through the ONE shared engine.
                SemanticQueryKey::ResolveClassSurface {
                    decl_slot,
                    type_args,
                    side,
                    context,
                } => self.build_class_surface(decl_slot, type_args, *side, *context),
                // TemplateLiteralReduce — LIVE producer. Folds the template
                // through the ONE shared deferred evaluator (no hand-rolled
                // concatenation), as its own live arm (NOT in the Miss group).
                SemanticQueryKey::TemplateLiteralReduce {
                    pattern,
                    args,
                    context,
                } => self.build_template_literal_reduce(pattern, args, *context),
                // ResolveAmbientNamespace / ResolveEnum / ResolveOverloadSet /
                // ApparentType / FlowNarrowingAt / ContextualTypeAt —
                // non-producing: these variants have no execute-side reducer.
                // The build returns `Opaque(Miss)` verbatim (mirroring the
                // `Relate` arm above); an `Error` result is never
                // warm-published, so nothing is admitted or cached. Returning
                // an empty `OverloadSet` for `ResolveOverloadSet` — a
                // fabricated apparent surface for `ApparentType` (whose
                // lib-member index does not exist yet) — or a fabricated
                // narrowed / contextual node for `FlowNarrowingAt` /
                // `ContextualTypeAt` (whose flow / contextual engines land in
                // U6) — would be a stub; `Miss` is the honest non-result.
                SemanticQueryKey::ResolveAmbientNamespace { .. }
                | SemanticQueryKey::ResolveEnum { .. }
                | SemanticQueryKey::ResolveOverloadSet { .. }
                | SemanticQueryKey::ApparentType { .. }
                | SemanticQueryKey::FlowNarrowingAt { .. }
                | SemanticQueryKey::ContextualTypeAt { .. } => {
                    let fence = self.project_generation_signature();
                    (QueryResult::Error(QueryError::Miss), fence).into()
                }
            }
        };
        // Wrap the raw cold-build closure with the fact tracer, then
        // build the published memo entry's self-version-rooted carrier.
        //
        // On `Ok(traced_facts)` the carrier is assembled by
        // `semantic_graph_read_set_signature` from the build's
        // `observed_self_roots` and the traced fact set: it prepends a
        // self-root `FileWholeHash` per observed self-root and merges
        // the traced cross-file facts. The producer is provenance-pure
        // — it roots the entry on the content version the build
        // OBSERVED, never a current-content re-read, so a
        // same-canonical content edit misses the warm read. A `None`
        // producer result (a torn / conflicting self-root observation,
        // or a `FileWholeHash` traced fact that disagrees with the
        // observed self-root) marks the build `cache_suppress = true`:
        // the value still flows to the caller, the memo refuses
        // admission.
        //
        // On `Overflow` the build is marked `cache_suppress = true` so
        // the memo refuses to admit the entry — caller cold-recomputes
        // on the next request.
        // Telemetry: detect whether the cold-build closure
        // actually ran so the per-kind cold/warm counters reflect what
        // happened inside `execute_cooperative` (warm hits short-
        // circuit before `traced_build` fires).
        let cold_build_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cold_build_ran_for_closure = Arc::clone(&cold_build_ran);
        let host = self.ctx.host_for_fact_tracer_install();
        let provenance = Arc::clone(&host.provenance);
        let traced_build = move || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            cold_build_ran_for_closure.store(true, std::sync::atomic::Ordering::Relaxed);
            // Open a cold-build-local taint frame for the duration of THIS
            // build. The shared read boundary folds every nested
            // sub-dispatch's discarded `CacheRead` metadata into this frame;
            // we pop + OR it into the output below, so a nested partial /
            // non-cacheable read can no longer escape the enclosing build's
            // memo-admission decision. The RAII guard pops the frame on
            // panic / early-return too (panic-safe pop), so an unwinding
            // cold build never leaks a stale frame onto the stack.
            let taint_guard = BuildLocalTaintGuard::push(&self.build_local_taint);
            let (mut output, finalise, fenced_serve_observed) =
                crate::fact_signature_helpers::install_fact_tracer(host, || {
                    // Test-only fact-injection hook. When the
                    // `dispatch_test_inject_parse_fact` slot is non-None,
                    // observe the recorded `Parse(...)` fact onto the
                    // active tracer cell BEFORE running the inner build.
                    dispatch_test_inject_parse_fact_if_set();
                    raw_build()
                });
            let build_local = taint_guard.finish();
            output.result_is_partial |= build_local.result_is_partial;
            output.cache_suppress |= build_local.cache_suppress;
            // ReturnOnly never publishes — fenced-serve arm. A build
            // whose traced scope consumed a FENCED (ReturnOnly)
            // `IndexedReady` serve computed its value basis from a
            // served-without-publication artifact, while the memo
            // entry's fact stamps (`dep_signature_for` reads the LIVE
            // project generation; the traced facts validate against a
            // fresh view) cannot be rejected read-side. The value still
            // flows to the caller; the memo refuses admission.
            output.cache_suppress |= fenced_serve_observed;
            provenance
                .memo_entry_fact_tracer_installs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            finalise_traced_build_output(output, finalise, &provenance)
        };
        let cache_read = graph.execute_cooperative(self.ctx, key.clone(), sentinel, traced_build);
        // Attribute the dispatch by `SemanticQueryKey` kind +
        // cold/warm. Cold = the `traced_build` closure ran. Warm = the
        // memo short-circuited before the closure fired.
        let is_cold = cold_build_ran.load(std::sync::atomic::Ordering::Relaxed);
        if let Some(observer) = verter_audit::current_observer() {
            use verter_audit::AuditEvent;
            let event = match &key {
                SemanticQueryKey::TypeOf { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryTypeOfCold)
                    } else {
                        Some(AuditEvent::SemanticQueryTypeOfWarm)
                    }
                }
                SemanticQueryKey::Instantiate { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryInstantiateCold)
                    } else {
                        Some(AuditEvent::SemanticQueryInstantiateWarm)
                    }
                }
                SemanticQueryKey::Conditional { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryConditionalCold)
                    } else {
                        Some(AuditEvent::SemanticQueryConditionalWarm)
                    }
                }
                SemanticQueryKey::MappedType { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryMappedTypeCold)
                    } else {
                        Some(AuditEvent::SemanticQueryMappedTypeWarm)
                    }
                }
                // Post-admission-time `IndexedAccess` is rewritten to
                // `ProjectPath` BEFORE the memo sees it, so live keys
                // are always `ProjectPath`. The arm stays for
                // exhaustiveness should the canonicalisation ever
                // shift.
                SemanticQueryKey::IndexedAccess { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryIndexedAccessCold)
                    } else {
                        Some(AuditEvent::SemanticQueryIndexedAccessWarm)
                    }
                }
                SemanticQueryKey::KeyOf { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryKeyOfCold)
                    } else {
                        Some(AuditEvent::SemanticQueryKeyOfWarm)
                    }
                }
                SemanticQueryKey::ProjectPath { .. } | SemanticQueryKey::ProjectMember { .. } => {
                    if is_cold {
                        Some(AuditEvent::SemanticQueryProjectPathCold)
                    } else {
                        Some(AuditEvent::SemanticQueryProjectPathWarm)
                    }
                }
                // ResolveDecl, NormalizeUnion, NormalizeIntersection,
                // ResolvedNamedType, Relate, ResolveMacroPayload —
                // not in the focused counter set.
                _ => None,
            };
            if let Some(event) = event {
                observer.record_event(event);
            }
        }
        // UNIVERSAL READ-BOUNDARY FOLD (the single fold point). Every
        // cold-build subquery read — issued through `execute`,
        // `execute_read`, the `execute_type_node` override, OR any direct
        // `execute_read` cold-build consumer in build / evaluate /
        // enumerate / walk — funnels through THIS helper, so folding the
        // returned `CacheRead`'s metadata into the active build-local taint
        // frame HERE closes the read-leak class in ONE place:
        //   - `cache_read.cache_suppress`  → the build-local frame (a
        //     benign non-cacheable but COMPLETE nested read taints the
        //     enclosing build's memo non-admission, but NOT the request
        //     partial sticky — component-meta still warms).
        //   - `cache_read.result_is_partial` → the build-local frame AND
        //     the per-request sticky (a genuine partial must gate
        //     component-meta / shape / materialize warm for the whole
        //     request).
        // At this point `traced_build` (if it ran a child build) has already
        // popped its OWN child frame inside `execute_cooperative`, so the
        // top of the stack is the PARENT build frame — folding the child's
        // returned metadata here propagates it exactly one level up. A
        // top-level read with no active build frame naturally no-ops
        // (`fold_into_top_build_local_taint` finds an empty stack).
        self.fold_cache_read_rails(cache_read.result_is_partial, cache_read.cache_suppress);
        tracing::debug!(
            target: "verter::dispatch::execute_via_helper",
            ?key,
            suppress = cache_read.cache_suppress,
            "execute_via_cold_build_helper"
        );
        cache_read
    }
}

/// Post-process a raw cold-build `QueryBuildOutput` into a
/// self-version-rooted one.
///
/// On `FactReadSetFinalise::Ok` it builds the published memo entry's
/// completed [`crate::fact_signature_helpers::ReadSetSignature`] carrier
/// from the build's `observed_self_roots` and the traced fact set via
/// [`crate::semantic_query_memo::semantic_graph_read_set_signature`],
/// and records the deduplicated self-root canonical set. A `None`
/// producer result (a torn / conflicting self-root observation) marks
/// the build `cache_suppress = true`. On `Overflow` it marks
/// `cache_suppress`.
///
/// `#[inline(never)]`: this is invoked once per cold build from the
/// dispatch's `traced_build` closure. Keeping it out-of-line gives its
/// `Vec`/`FxHashMap` locals their own poppable stack frame instead of
/// inflating the `traced_build` closure frame — `traced_build` sits on
/// the recursive cold-build call chain (a deeply-nested type resolution
/// nests one cold build per hop), so a fat closure frame multiplies
/// across the recursion depth.
#[inline(never)]
fn finalise_traced_build_output(
    output: crate::project_semantic_dispatch::walk::QueryBuildOutput,
    finalise: crate::resolver_core::FactReadSetFinalise,
    provenance: &crate::types::MetaProvenance,
) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
    let mut output = output;
    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(traced_facts) => {
            // Record the self-root canonicals (deduplicated) for the
            // strict warm-read validator on the published `MemoEntry`.
            let mut self_root_canonicals: Vec<Arc<str>> =
                Vec::with_capacity(output.observed_self_roots.len());
            for (canonical, _) in output.observed_self_roots.iter() {
                if !self_root_canonicals.iter().any(|c| c == canonical) {
                    self_root_canonicals.push(Arc::clone(canonical));
                }
            }
            // Fold the build's `dep_signature` fence into the traced
            // fact set BEFORE building the carrier. The published
            // `ReadSetSignature` is the SOLE cache-validity rail
            // (`MemoEntry::validate` checks the carrier's facts
            // alone); folding `dep_signature` here ensures explicit
            // dispatch-time assertions — most commonly the
            // `project_generation_signature()` a `KeyOf` /
            // `ProjectPath` / normalization / `Relate` builder records
            // — land on the carrier. Without this fold a bare
            // `bump_project_generation()` with file self-roots
            // unchanged would warm-hit a stale entry.
            // `semantic_graph_read_set_signature` already dedups
            // self-root `FileWholeHash` facts and rejects on a
            // same-canonical hash conflict, so a builder naming its
            // own self-root canonical in `dep_signature` is rejected
            // naturally if it disagrees with the observed self-root.
            let fence_facts = crate::fact_signature_helpers::dep_signature_to_fact_signature(
                &output.dep_signature,
            );
            let merged_facts: Vec<crate::resolver_core::FactVersionRef> = if fence_facts.is_empty()
            {
                traced_facts.iter().cloned().collect()
            } else {
                let mut merged: Vec<crate::resolver_core::FactVersionRef> =
                    Vec::with_capacity(traced_facts.len() + fence_facts.len());
                merged.extend(traced_facts.iter().cloned());
                for fact in fence_facts {
                    if !merged.iter().any(|existing| existing == &fact) {
                        merged.push(fact);
                    }
                }
                merged
            };
            match crate::semantic_query_memo::semantic_graph_read_set_signature(
                &output.observed_self_roots,
                &merged_facts,
            ) {
                Some(carrier) => {
                    // §18.2 fact-rooted admission: a sound self-version-rooted
                    // carrier is the FIRST gate (a torn / unrootable self-root
                    // already routed to the `None` arm below, and an overflowed
                    // tracer to the `Overflow` arm). `admit_decision` applies
                    // the SECOND, taint-narrowed gate: a `Clean` result over a
                    // sound carrier is `Warm`. `taint` is currently always
                    // `Clean`; non-`Clean` taint is produced by the §18.4
                    // input-degradation producers, so today every build
                    // publishes warm. A partial result is `Warm` only when ITS
                    // invalidation fact is on the carrier, else `ReturnOnly`.
                    match crate::semantic_query::admit::admit_decision(output.taint, &carrier) {
                        crate::semantic_query::admit::Admission::Warm => {
                            output.graph_carrier = Some(Box::new(carrier));
                            output.self_root_canonicals = Arc::from(self_root_canonicals);
                        }
                        crate::semantic_query::admit::Admission::ReturnOnly => {
                            // Non-admission: the value flows to the caller but
                            // the memo refuses to publish it. Broadcast the
                            // (valid) traced facts on a NON-ADMITTED carrier so
                            // a cooperative-admission joiner inherits the
                            // transitive dependency facts, exactly as the
                            // unrootable `None` arm does.
                            output.cache_suppress = true;
                            output.graph_carrier = Some(Box::new(carrier));
                        }
                    }
                }
                None => {
                    // Non-cacheable: refuse memo admission (the value
                    // still flows back to the caller). The build's
                    // traced cross-file dep facts are nonetheless
                    // valid — `semantic_graph_read_set_signature`
                    // refused only because the entry could not be
                    // soundly SELF-rooted (a torn self-root
                    // observation). Carry those traced facts on a
                    // NON-ADMITTED carrier: the cooperative-admission
                    // winner bubbles this carrier into its own outer
                    // tracer AND broadcasts it to cross-thread
                    // joiners, so a joiner inside an outer cold query
                    // inherits the suppressed child's transitive
                    // dependency facts exactly as a joiner of a
                    // cacheable child would. Memo admission stays
                    // gated by `cache_suppress` — this carrier is
                    // broadcast, never published. The merged
                    // dispatch-fence facts ride on the broadcast
                    // carrier too so joiners observe the same
                    // project-generation gate.
                    output.cache_suppress = true;
                    output.graph_carrier = Some(Box::new(
                        crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(
                            merged_facts.into_boxed_slice(),
                        )),
                    ));
                }
            }
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            provenance
                .memo_entry_overflow_refusals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // A tracer overflow yields no bounded fact list, so no
            // carrier can be broadcast — the joiner inherits the
            // non-cacheability through the `cache_suppress` flag the
            // cooperative-admission path propagates to joiners.
            output.cache_suppress = true;
        }
    }
    // §1 INVARIANT enforcement (the convergence point): a genuine
    // computational partial MUST imply non-cacheability. The build-local
    // taint fold (in `traced_build`) and every producer set
    // `result_is_partial` for genuine partials; enforce `result_is_partial
    // ⟹ cache_suppress` here at the single finalisation boundary so a
    // partial value can never be admitted to the semantic family memo (a
    // laundered `Value + result_is_partial=true, cache_suppress=false`
    // entry — Finding B — cannot exist). Benign non-cacheable COMPLETE
    // results (`cache_suppress=true, result_is_partial=false`) are left
    // untouched: they stay out of the memo but still warm component-meta
    // (which gates on `result_is_partial` ONLY).
    if output.result_is_partial {
        output.cache_suppress = true;
    }
    debug_assert!(
        !output.result_is_partial || output.cache_suppress,
        "§1 invariant violated at finalisation: result_is_partial \
         without cache_suppress would launder a partial into the memo"
    );
    output
}

/// Aggregate request work budget gate.
///
/// Returns `true` for every `SemanticQueryKey` kind that counts toward
/// the per-request fail-closed work budget ([`crate::request_budget`]).
/// This is an AGGREGATE semantic-expansion/work budget, NOT a per-kind
/// fuse: alongside the projection operators (`ProjectMember` /
/// `IndexedAccess` / `ProjectPath` / `KeyOf` / `MappedType`) it counts
/// `Instantiate` and `Conditional` — the two kinds that dominate an
/// open-generic expansion storm (`Pick<PropsBase<T>, …>` re-instantiating
/// cross-file generics through chained conditionals). Counting only
/// projection operators left those two unbounded, so a storm dominated
/// by them never tripped the fuse and ran past the early-exit. Including
/// them makes any storm fail closed to a structurally-valid typed PARTIAL
/// within seconds — with cache suppression — regardless of which upstream
/// layer missed.
///
/// `TemplateLiteralReduce` counts too: a template over wide finite unions
/// enumerates a cartesian product, so an unbounded re-dispatch storm over
/// template reductions is the same expansion-storm shape. (The reducer also
/// applies its own per-call product-width cap — `TEMPLATE_LITERAL_KEYSPACE_CAP`
/// — which bounds a SINGLE reduction; this gate bounds the aggregate dispatch
/// count across the request.)
///
/// `TypeOf` counts too: it is a demand-bearing projection reducer
/// (`build_typeof` lowers a value's declaration graph at the requested
/// demand), so a typeof-dominated storm — a wide value graph crossed
/// repeatedly through `typeof` roots — is the same expansion-storm shape.
/// Its child `Instantiate` / `ProjectPath` work counting alone would leave
/// the reducer itself outside the fuse.
fn semantic_query_counts_toward_projection_budget(key: &SemanticQueryKey) -> bool {
    matches!(
        key,
        SemanticQueryKey::ProjectMember { .. }
            | SemanticQueryKey::IndexedAccess { .. }
            | SemanticQueryKey::ProjectPath { .. }
            | SemanticQueryKey::KeyOf { .. }
            | SemanticQueryKey::MappedType { .. }
            | SemanticQueryKey::Instantiate { .. }
            | SemanticQueryKey::Conditional { .. }
            | SemanticQueryKey::TemplateLiteralReduce { .. }
            | SemanticQueryKey::TypeOf { .. }
    )
}

impl<'a> SemanticQueryApi for ProjectSemanticDispatch<'a> {
    fn execute(
        &self,
        key: SemanticQueryKey,
    ) -> QueryResult<SemanticQueryOutput<SemanticQueryValue>> {
        // Type-resolution audit hop / mode / projection / conditional
        // counters — shared with the `execute_type_node` consumer entry
        // (both are consumer entry points; the chokepoint override below
        // also calls this so its callers keep counting after switching to
        // `execute_read`).
        self.record_dispatch_intent_counters(&key);
        // Delegate to the shared cold-build helper. Both `execute`
        // (this method) and `execute_read` route through the helper
        // so the fact-tracer wrapper, sentinel construction, and
        // build-output threading live in one place. `execute`
        // discards the dep-signature rails from the helper's
        // `CacheRead`; `execute_read` keeps them.
        //
        // The helper resolves to a bare node id; wrap it into the
        // domain-agnostic value here at the public boundary. This wrap
        // fires ONLY on the `Value` arm, so the value is always `TypeNode`;
        // the non-producing keys (`Relate` and the `NonProducingPendingReducer`
        // variants: `ResolveAmbientNamespace`, `ResolveEnum`,
        // `ResolveOverloadSet`, `ApparentType`, `FlowNarrowingAt`,
        // `ContextualTypeAt`) return `Error(Miss)` and never reach the wrap,
        // which keeps the unconditional `TypeNode` wrap correct. The boundary provenance is
        // `clean` — a wrapper only, never a cached semantic fact.
        match self.execute_via_cold_build_helper(key).value {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::TypeNode(node),
                provenance: ResultProvenance::clean(),
            }),
            QueryResult::Recursive(n) => QueryResult::Recursive(n),
            QueryResult::Error(e) => QueryResult::Error(e),
        }
    }

    /// Consumer entry point that returns a bare [`SemanticNodeId`].
    ///
    /// The trait default narrows the value-only `execute`, dropping the
    /// `CacheRead` partiality / non-cacheability metadata of the
    /// sub-dispatch. The override drives `execute_read` instead so its read
    /// funnels through the shared read boundary
    /// ([`Self::execute_via_cold_build_helper`]) — which performs the
    /// UNIVERSAL build-local taint fold (`result_is_partial` AND
    /// `cache_suppress`) plus the per-request partial-sticky mark for every
    /// cold-build subquery read in ONE place. The override therefore does
    /// NOT re-fold here (the shared boundary already folded); it only counts
    /// dispatch intent and narrows the value half.
    ///
    /// `finalise_traced_build_output` later ORs the popped frame into the
    /// `QueryBuildOutput` and enforces `result_is_partial ⟹ cache_suppress`.
    fn execute_type_node(
        &self,
        key: SemanticQueryKey,
    ) -> QueryResult<SemanticQueryOutput<SemanticNodeId>> {
        // This is a CONSUMER entry point (the trait default routed through
        // `execute`, which bumped these counters). Now that the override
        // drives `execute_read` — which does NOT bump them — count here so
        // `execute_type_node` callers (every convenience wrapper, the
        // path-precision hop oracle) keep their attribution.
        self.record_dispatch_intent_counters(&key);
        // The shared read boundary inside `execute_read` →
        // `execute_via_cold_build_helper` performs the universal taint fold
        // (build-local `result_is_partial` + `cache_suppress`, and the
        // request partial sticky). No bespoke re-fold here — re-folding
        // would double-count the same read into the parent frame.
        let read = self.execute_read(key);
        // Narrow the value half exactly as the trait default would, wrapping
        // the bare node into the domain value with clean boundary provenance.
        match read.value {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryOutput {
                value: node,
                provenance: ResultProvenance::clean(),
            }),
            QueryResult::Recursive(n) => QueryResult::Recursive(n),
            QueryResult::Error(e) => QueryResult::Error(e),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Utilities exposed for higher-level callers
// ──────────────────────────────────────────────────────────────────────────

/// Convenience: construct a [`ResolveDeclKey`] for a top-level symbol in
/// `canonical_id`. Wrapping the arc-conversion here keeps call sites tidy
/// and avoids having each caller re-invent the scope construction.
#[must_use]
pub fn resolve_decl_key(canonical_id: &str, name: &str) -> ResolveDeclKey {
    ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical_id),
            local_scope: None,
        },
        name: Arc::from(name),
    }
}

/// Convenience: fetch the resolved semantic-node payload for a previously
/// executed key. Returns `None` if the memo has not warmed the key yet.
///
/// Accepts `&dyn ResolverContext`; the trait method
/// `dispatch_node_data` provides the same access from any context.
/// Existing callers passing `&VerterHost` upcast implicitly.
#[must_use]
pub(crate) fn node_data_for(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<Arc<SemanticNodeData>> {
    ctx.project_type_store().semantic_graph().node_data(node)
}

// ──────────────────────────────────────────────────────────────────────────
// Dispatch helpers (NON-variant; compose existing variants
// + read sidecars). These are the 3 helpers that replace the 3 originally
// proposed `SemanticQueryKey` variants per the §0 binding amendment
// (`MaterializeSurface`, `ResolvePublicInstance`, `ResolveFallthroughSurface`)
// + 1 extra utility helper (`execute_to_type_expr`).
//
// Per §0 amendment: these are NON-variant — `ProjectSemanticDispatch` does
// not gain new variants; instead, it exposes plain methods that compose
// existing dispatchers and read the
// `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar at
// `verter_semantic/src/analysis/component_meta.rs:83-110`.
//
// **No callsite changes in this commit** — callsite migrations (4a/4b/4c
// classes A/B/C, 5/6/7/8/9 classes D + R) land in 5d-5f.
// ──────────────────────────────────────────────────────────────────────────

/// `__builtin__` type-space slot for the `Pick` utility.
///
/// Returns the content-free, env-bearing
/// [`ResolvedDeclSlotIdentity`](crate::semantic_query::ResolvedDeclSlotIdentity)
/// used as `SemanticQueryKey::Instantiate.base` for the Pick builtin
/// (`build_builtin_utility` Pick arm). TEST-ONLY fixture: it carries a
/// zeroed env (project / type-env / lib-env), so it is suitable for
/// identity assertions, NOT for an env-discriminating production
/// dispatch — the production path uses
/// [`ProjectSemanticDispatch::builtin_type_slot`] with the live host env.
/// `#[cfg(test)]`-gated so no production caller can pick the zero-env
/// degenerate slot.
#[cfg(test)]
#[must_use]
pub fn pick_builtin_decl_identity() -> crate::semantic_query::ResolvedDeclSlotIdentity {
    crate::semantic_query::ResolvedDeclSlotIdentity::type_slot(
        Arc::from("__builtin__"),
        Arc::from("Pick"),
        0,
        Default::default(),
        Default::default(),
    )
}

/// `__builtin__` type-space slot for the `Omit` utility.
///
/// Mirrors [`pick_builtin_decl_identity`] for the Omit builtin. TEST-ONLY
/// fixture, `#[cfg(test)]`-gated.
#[cfg(test)]
#[must_use]
pub fn omit_builtin_decl_identity() -> crate::semantic_query::ResolvedDeclSlotIdentity {
    crate::semantic_query::ResolvedDeclSlotIdentity::type_slot(
        Arc::from("__builtin__"),
        Arc::from("Omit"),
        0,
        Default::default(),
        Default::default(),
    )
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Direct mirror of
    /// [`materialize_component_meta_structure`](crate::component_meta_materialize::materialize_component_meta_structure).
    ///
    /// Caller pattern-matches `MaterializeOutcome` directly — no
    /// conversion to `QueryResult`. Use this when callers need
    /// `PackageRefTopLevel` / `FunctionPropertyAtNested` /
    /// `RecursiveHelperCycleGuard` / `MAX_DEPTH` gates (which
    /// `ProjectPath` does NOT inherit).
    ///
    /// **Why not a `SemanticQueryKey` variant?** Per §0 binding
    /// amendment, `MaterializeSurface` is redundant with the existing
    /// `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar +
    /// `MaterializeStructureDb` cache. This helper exposes the
    /// existing function as a method on the dispatcher so consumers
    /// can route policy-gated materialisations through dispatch
    /// without inflating the variant set.
    #[allow(dead_code)]
    pub fn materialize_surface(
        &self,
        key: crate::component_meta_materialize::MaterializeStructureCacheKey,
    ) -> CacheRead<crate::component_meta_materialize::MaterializeOutcome> {
        crate::component_meta_materialize::materialize_component_meta_structure(self.ctx, key)
    }

    /// `Pick<base, members>` via the existing builtin
    /// Pick dispatch (`build_builtin_utility` Pick arm at `build.rs:870`).
    ///
    /// Inherits TS Pick semantics including modifier preservation
    /// (`optional`/`readonly` flow through). The members slice is
    /// interned as a `Union` of string literals via
    /// [`Self::intern_string_literal_union`] before dispatch — empty
    /// members produce a `Primitive(Never)` keyspace which the Pick
    /// arm reduces to an empty Object (TS spec §4.4 Pick semantics).
    ///
    /// **Why not a variant?** Per §0 binding amendment, `Pick` /
    /// `Omit` are already routed through the
    /// `build_builtin_utility` path under
    /// `SemanticQueryKey::Instantiate`. This helper is a typed
    /// convenience wrapper — the underlying memo entry is the same
    /// `Instantiate` family/slot.
    pub fn execute_pick(
        &self,
        base: SemanticNodeId,
        members: &[Arc<str>],
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        let key_set = self.intern_string_literal_union(members);
        crate::semantic_query::strip_output_provenance(self.execute_type_node(
            SemanticQueryKey::Instantiate {
                base: self.builtin_type_slot("Pick"),
                args: Arc::from(vec![base, key_set].into_boxed_slice()),
                context: self.instantiate_context_for(
                    "__builtin__",
                    crate::semantic_query::ProjectionReductionContext::published(mode),
                ),
            },
        ))
    }

    /// `Omit<base, members>` via the existing builtin
    /// Omit dispatch (`build_builtin_utility` Omit arm at `build.rs:911`).
    ///
    /// Inherits TS Omit semantics including the "Omit preserves
    /// source signatures" rule (per `build.rs:937-939` doc) — call /
    /// construct / index signatures of the base are kept when the
    /// keys-to-omit don't shadow them.
    #[allow(dead_code)]
    pub fn execute_omit(
        &self,
        base: SemanticNodeId,
        members: &[Arc<str>],
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        let key_set = self.intern_string_literal_union(members);
        crate::semantic_query::strip_output_provenance(self.execute_type_node(
            SemanticQueryKey::Instantiate {
                base: self.builtin_type_slot("Omit"),
                args: Arc::from(vec![base, key_set].into_boxed_slice()),
                context: self.instantiate_context_for(
                    "__builtin__",
                    crate::semantic_query::ProjectionReductionContext::published(mode),
                ),
            },
        ))
    }

    /// `execute_read(key)` + `raise_node_to_type_expr` in one call,
    /// returning the full `CacheRead` so `dep_signature` is
    /// preserved for the caller's fence merge. A lossy
    /// `Option<TypeExpr>` return would erase the dep-signature and
    /// break the fence-merge contract, so this helper hands the
    /// `CacheRead` through instead.
    ///
    /// Used by trampoline conversions and consumer migrations that
    /// need a `TypeExpr` for downstream `ComponentMetaAnalysis` field
    /// shape consumption.
    pub fn execute_to_type_expr(&self, key: &SemanticQueryKey) -> CacheRead<QueryResult<TypeExpr>> {
        let read = self.execute_read(key.clone());
        let typed_value = match read.value {
            QueryResult::Value(node) => match self.raise_node_to_type_expr(node) {
                Some(expr) => QueryResult::Value(expr),
                None => QueryResult::Error(QueryError::Miss),
            },
            QueryResult::Recursive(id) => QueryResult::Recursive(id),
            QueryResult::Error(e) => QueryResult::Error(e),
        };
        CacheRead {
            value: typed_value,
            dep_signature: read.dep_signature,
            walker_diagnostics: read.walker_diagnostics,
            cache_suppress: read.cache_suppress,
            result_is_partial: read.result_is_partial,
        }
    }

    /// Slot-binding-parameter type lowering for `defineSlots<T>()`.
    /// Three-hop composition: `Navigate` to the slot member, read
    /// `params[0].ty` from the slot value's `Function`, then project
    /// the binding member off the param Object in the caller's mode.
    /// Returns `QueryResult::Error(Miss)` when an intermediate hop
    /// fails or the slot value is not a `Function`.
    pub fn project_slot_binding_member(
        &self,
        base: SemanticNodeId,
        slot_name: &str,
        binding_name: &str,
        mode: ProjectionMode,
    ) -> CacheRead<QueryResult<TypeExpr>> {
        let read =
            self.project_slot_binding_member_with_terminal_id(base, slot_name, binding_name, mode);
        CacheRead {
            value: match read.value {
                QueryResult::Value((_id, expr)) => QueryResult::Value(expr),
                QueryResult::Recursive(id) => QueryResult::Recursive(id),
                QueryResult::Error(e) => QueryResult::Error(e),
            },
            dep_signature: read.dep_signature,
            walker_diagnostics: read.walker_diagnostics,
            cache_suppress: read.cache_suppress,
            result_is_partial: read.result_is_partial,
        }
    }

    /// Slot-binding terminal-id variant: same three-hop traversal as
    /// [`Self::project_slot_binding_member`], plus exposes the terminal
    /// `SemanticNodeId` alongside the raised `TypeExpr`. The audit-record
    /// assembly in `compute_evaluated_types` consumes the production-path
    /// identity directly (no audit-only re-lowering — per the
    /// demand-driven reducer spec).
    pub fn project_slot_binding_member_with_terminal_id(
        &self,
        base: SemanticNodeId,
        slot_name: &str,
        binding_name: &str,
        mode: ProjectionMode,
    ) -> CacheRead<QueryResult<(SemanticNodeId, TypeExpr)>> {
        // Hop 1: Navigate to the slot member off the macro payload base.
        let slot_path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from(slot_name))].into_boxed_slice());
        let slot_read = self.execute_read(SemanticQueryKey::ProjectPath {
            base,
            path: slot_path,
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        });
        let slot_node = match slot_read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(id) => {
                return CacheRead {
                    value: QueryResult::Recursive(id),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                    result_is_partial: slot_read.result_is_partial,
                };
            }
            QueryResult::Error(e) => {
                return CacheRead {
                    value: QueryResult::Error(e),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                    result_is_partial: slot_read.result_is_partial,
                };
            }
        };
        // Hop 2: the slot value's first-parameter type holds the bindings.
        let param0_ty = match node_data_for(self.ctx, slot_node).as_deref() {
            Some(SemanticNodeData::Function { params, .. }) => match params.first() {
                Some(param) => param.ty,
                None => {
                    return CacheRead {
                        value: QueryResult::Error(QueryError::Miss),
                        dep_signature: slot_read.dep_signature,
                        walker_diagnostics: slot_read.walker_diagnostics,
                        cache_suppress: slot_read.cache_suppress,
                        result_is_partial: slot_read.result_is_partial,
                    };
                }
            },
            _ => {
                return CacheRead {
                    value: QueryResult::Error(QueryError::Miss),
                    dep_signature: slot_read.dep_signature,
                    walker_diagnostics: slot_read.walker_diagnostics,
                    cache_suppress: slot_read.cache_suppress,
                    result_is_partial: slot_read.result_is_partial,
                };
            }
        };
        // Hop 3: project the binding member off the param Object at the
        // caller's terminal mode (path-precise rule — only the terminal
        // runs in the requested mode).
        let binding_path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from(binding_name))].into_boxed_slice());
        let binding_read = self.execute_read(SemanticQueryKey::ProjectPath {
            base: param0_ty,
            path: binding_path,
            context: crate::semantic_query::ProjectionReductionContext::published(mode),
        });
        let merged: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = slot_read
            .dep_signature
            .iter()
            .cloned()
            .chain(binding_read.dep_signature.iter().cloned())
            .collect();
        let value = match binding_read.value {
            QueryResult::Value(terminal_id) => match self.raise_node_to_type_expr(terminal_id) {
                Some(expr) => QueryResult::Value((terminal_id, expr)),
                None => QueryResult::Error(QueryError::Miss),
            },
            QueryResult::Recursive(id) => QueryResult::Recursive(id),
            QueryResult::Error(e) => QueryResult::Error(e),
        };
        CacheRead {
            value,
            dep_signature: Arc::from(merged.into_boxed_slice()),
            walker_diagnostics: binding_read.walker_diagnostics,
            cache_suppress: binding_read.cache_suppress,
            // Fold the partial signal across BOTH hops: a budget/walker
            // partial on the Navigate slot hop must taint the composed
            // result even when the terminal binding hop completed.
            result_is_partial: slot_read.result_is_partial || binding_read.result_is_partial,
        }
    }

    /// Trivial helper: lower a `[String]` member-name
    /// list to an `Arc<[PathSegment]>` for `ProjectPath` queries.
    ///
    /// Each member name becomes a `PathSegment::Member(Arc<str>)`. The
    /// result has the same length and order as the input.
    #[must_use]
    #[allow(dead_code)]
    pub fn lower_path_segments(p: &[String]) -> Arc<[PathSegment]> {
        let segs: Vec<PathSegment> = p
            .iter()
            .map(|s| PathSegment::Member(Arc::from(s.as_str())))
            .collect();
        Arc::from(segs.into_boxed_slice())
    }

    /// Trivial helper: intern a `[Arc<str>]` member-name
    /// list as a `Union` of string-literal nodes.
    ///
    /// Empty input produces `Primitive(Never)` — caller-side pickup of
    /// the TS spec §4.4 rule that `Pick<T, never>` reduces to `{}`.
    /// Single-element input produces a `Union<[lit]>` (always
    /// uniformly `Union` even at arity 1, for caller uniformity).
    /// `Pick` / `Omit` callers can pass the result directly as the
    /// 2nd argument of `Instantiate`.
    #[must_use]
    pub fn intern_string_literal_union(&self, members: &[Arc<str>]) -> SemanticNodeId {
        let graph = self.graph();
        if members.is_empty() {
            return graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        }
        let lit_ids: Vec<SemanticNodeId> = members
            .iter()
            .map(|m| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    m.to_string(),
                )))
            })
            .collect();
        graph.intern_node(SemanticNodeData::Union(Arc::from(
            lit_ids.into_boxed_slice(),
        )))
    }
}

// ──────────────────────────────────────────────────────────────────────────
// DispatchHost trait + session-owned adapter
// ──────────────────────────────────────────────────────────────────────────
//
// `DispatchHost` is the scope-free minimum-surface host seam dispatch
// builders use to reach host-owned prepared declarations, root identities,
// utility classification, and bare-reference origin classification.
//
// The session-owned adapter [`SessionDispatchHost`] implements the trait
// by consulting [`SemanticGraphStore::node_scope`] for each base node to
// reconstruct the originating scope and fetch the scope's declaration-
// scope payload via [`crate::resolver_core::bare_name_resolve`]. Dispatch
// builders take `&dyn DispatchHost` rather than `&VerterHost`, so they
// stay scope-free.

/// Scope-free, minimum-surface host seam for dispatch builders.
///
/// Builders that need to reach host-owned prepared declarations, classify
/// utility names, resolve root identities, or classify bare references take
/// `&dyn DispatchHost`. The adapter internally queries
/// [`SemanticGraphStore::node_scope`](crate::semantic_query_memo::SemanticGraphStore::node_scope)
/// on the `base` id to route each lookup through the correct per-base
/// scope — builders stay scope-free.
///
/// **Contract:** every method takes the base [`SemanticNodeId`] whose scope
/// informs the lookup. Implementations resolve identities through
/// [`crate::resolver_core::bare_name_resolve`] over the scope's
/// declaration-scope payload.
///
/// Implementations: [`SessionDispatchHost`] routes per-base via the
/// node-scope sidecar.
pub trait DispatchHost {
    /// Look up a prepared type declaration by its canonical root identity,
    /// using the scope recorded in `base`'s sidecar to select the correct
    /// declaration-scope payload.
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>>;

    /// Resolve a `(canonical_id, symbol_name)` pair into a stable root
    /// declaration identity, following re-exports and barrel hops through
    /// `base`'s scope.
    #[allow(dead_code)]
    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity>;

    /// Classify whether `name` is a built-in TS utility, user-shadowed, or
    /// unknown in `base`'s scope. Scope matters because a local binding in
    /// scope A can shadow a built-in that is not shadowed in scope B.
    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource;

    /// Classify whether `name` resolves locally or through an import in
    /// `base`'s scope. Used by lazy field expansion to keep imported
    /// object-like refs symbolic until a deeper route is requested.
    #[allow(dead_code)]
    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin;
}

/// Session-owned [`DispatchHost`] implementation.
///
/// Given a base [`SemanticNodeId`], consults
/// [`SemanticGraphStore::node_scope`] to reconstruct the originating
/// [`NodeScopeId`]. Fetches the scope's declaration-scope payload via
/// `prepared_decl_bundle` and routes each trait method through
/// [`crate::resolver_core::bare_name_resolve`] helpers:
///
/// - [`NodeScopeId::File { canonical_id, .. }`] → scope canonical + scope payload
/// - [`NodeScopeId::Global`] / exempt / missing → no scope context
///
/// The adapter holds only a host reference; scope is resolved fresh per
/// call so the adapter stays `Send + Sync` and cheap to construct.
pub struct SessionDispatchHost<'a> {
    ctx: &'a dyn ResolverContext,
}

impl<'a> SessionDispatchHost<'a> {
    /// Construct an adapter bound to `ctx`. The adapter does not retain a
    /// base node or a scope payload — it re-resolves per-base scope on
    /// every call via [`Self::base_scope`].
    ///
    /// Locked-in signature: takes `&dyn ResolverContext`.
    /// Existing call sites pass `&host` (concrete `&VerterHost`) and
    /// upcast implicitly because `impl ResolverContext for VerterHost`.
    #[must_use]
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        Self { ctx }
    }

    /// Public accessor for `base`'s recorded origin scope. Returns
    /// [`NodeScopeId::Global`] for exempt or missing nodes so every base
    /// has a well-defined routing decision.
    ///
    /// Exposed so tests can observe routing behaviour without retaining
    /// a scope payload directly.
    #[must_use]
    pub fn base_scope(&self, base: SemanticNodeId) -> NodeScopeId {
        self.ctx
            .project_type_store()
            .semantic_graph()
            .node_scope(base)
            .unwrap_or(NodeScopeId::Global)
    }

    /// Fetch the declaration-scope payload for `base`'s origin scope.
    /// Returns `None` when the scope is global/exempt, or when the scope
    /// has no prepared-decl bundle (e.g. a declaration file the host has
    /// not yet shallow-indexed). The payload is consulted by the
    /// `DispatchHost` trait methods for scope-local name resolution +
    /// shadowing.
    fn scope_payload_for_base(
        &self,
        base: SemanticNodeId,
    ) -> (
        Option<String>,
        Option<crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
    ) {
        match self.base_scope(base) {
            NodeScopeId::File { canonical_id, .. } => {
                // Instrumentation: callsite attribution for
                // `prepared_decl_bundle_warm` reads. The four
                // `DispatchHost` trait callbacks
                // (`resolve_prepared_type_decl`, `root_identity`,
                // `utility_source`, `bare_ref_origin`) all route
                // through this helper — dominant expected source of
                // the K-loop warm-read pressure.
                if let Some(obs) = verter_audit::current_observer() {
                    obs.record_event(
                        verter_audit::AuditEvent::PreparedDeclBundleCallsiteScopePayload,
                    );
                }
                let payload = self.ctx.prepared_decl_bundle(canonical_id.as_ref()).map(
                    |bundle| {
                        crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                            &bundle,
                        )
                    },
                );
                (Some(canonical_id.as_ref().to_string()), payload)
            }
            NodeScopeId::Global => (None, None),
        }
    }
}

impl<'a> DispatchHost for SessionDispatchHost<'a> {
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        let (scope_canonical, payload) = self.scope_payload_for_base(base);
        crate::resolver_core::bare_name_resolve::resolve_prepared_type_decl_via_host(
            self.ctx,
            scope_canonical.as_deref(),
            payload.as_ref(),
            root_identity,
        )
    }

    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        let (scope_canonical, payload) = self.scope_payload_for_base(base);
        // Match `SessionSolverHost::root_identity`: empty caller
        // canonical defers to the base's origin scope.
        let resolution_scope = if canonical_id.is_empty() {
            scope_canonical.as_deref().unwrap_or("")
        } else {
            canonical_id
        };
        crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            resolution_scope,
            payload.as_ref(),
            symbol_name,
        )
    }

    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource {
        use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
        let (_scope_canonical, payload) = self.scope_payload_for_base(base);
        // Scope-local shadowing takes priority: a userland `type Partial`
        // in scope wins over the built-in utility.
        if let Some(payload) = payload.as_ref() {
            if payload.scope_type_names.contains(name)
                || payload.scope_type_bindings.contains_key(name)
            {
                return UtilitySource::Shadowed;
            }
        }
        // SDK-declared intrinsics always classify as `Builtin` regardless
        // of shadowing — matches `SessionSolverHost::utility_source`.
        if let crate::intrinsic_registry::IntrinsicLookup::Found(_) = self
            .ctx
            .project_type_store()
            .intrinsic_registry()
            .lookup(name)
        {
            return UtilitySource::Builtin;
        }
        if BuiltinUtility::from_name(name).is_some() {
            UtilitySource::Builtin
        } else {
            UtilitySource::Unknown
        }
    }

    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin {
        let (_scope_canonical, payload) = self.scope_payload_for_base(base);
        if let Some(payload) = payload.as_ref() {
            if payload.import_bindings.contains_key(name) {
                return BareRefOrigin::Imported;
            }
            if payload.scope_type_bindings.contains_key(name)
                || payload.scope_type_names.contains(name)
                || payload.scope_value_names.contains(name)
            {
                return BareRefOrigin::Local;
            }
        }
        BareRefOrigin::Unknown
    }
}

#[cfg(test)]
mod tests;
