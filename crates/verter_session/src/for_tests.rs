//! Re-export shim for integration tests in
//! `crates/verter_session/tests/*.rs`. Integration tests build the lib
//! WITHOUT `cfg(test)` set, so they cannot reach `pub(crate)` items
//! through the normal `cfg(test)` test-helper pattern. This module is
//! gated `cfg(any(test, feature = "test-support"))` at its declaration site in
//! `lib.rs` so release builds never compile it (`debug_assertions` is
//! OFF in `cargo build --release`).
//!
//! All entries here are thin re-exports or one-call wrappers that route
//! test-only access through a single grep-able name. Internal production
//! callers continue to reach the underlying symbols directly.
//!
//! `WorldSnapshot` and its companions (`OverlayIdentity`,
//! `WorldSnapshotDims`, and the per-dimension `*Dims` carriers) are
//! intentionally NOT re-exported here: they are `pub(crate)` and must
//! not be reachable from outside `verter_session`, even in debug
//! builds. The construction contract is exercised by
//! `#[cfg(test)] mod tests` inline in
//! `src/cache_runtime/world_snapshot.rs`, which can struct-init the
//! type directly without a parallel constructor.

pub use crate::capture_token::{
    assert_no_stack_overflow, with_active_capture, with_active_capture_returning, CacheId,
    CacheKeyFilter, CacheProvenance, CanonicalId, CaptureGuard, CaptureSnapshot, CaptureToken,
    DispatchEntry, EdgeIdentity, InternedId, KeyFamily, SignatureHash, StackOverflow,
};
/// Tier 1B: re-export the cooperative-batch primitive so the
/// selective component-meta integration tests can probe its
/// existence. Internal callers continue to reach
/// `crate::semantic_query_memo::SemanticGraphStore` directly.
pub use crate::semantic_query_memo::{BatchExpandError, SemanticGraphStore};

/// Re-export the family-domain mapping probe so the `g_block` guards can
/// assert `Relate` maps to a DEDICATED `FamilyKey::Relate` (never aliasing
/// `IndexedAccess`) without exposing the `pub(super)` `FamilyKey` taxonomy.
pub use crate::semantic_query_memo::family_variant_label_for_tests;

/// Re-export the `FamilyKey` size probe so the `g_block` guards can pin
/// the keyspace size discipline (the `Relate` payload must stay boxed, never
/// embedded by value) without exposing the `pub(super)` taxonomy.
pub use crate::semantic_query_memo::family_key_size_for_tests;

/// Re-export the canonical display projection so the `g_block` integration
/// guards can call it. `display` is `pub` (forced by E0364: it lives in a
/// `pub mod display`, so it cannot be more private than its enclosing module);
/// the integration suite reaches it through this surface, mirroring the
/// `SemanticGraphStore` re-export above.
pub use crate::semantic_query::display::{display, DisplayString};

/// Re-export the validate-running probe surface so the
/// `family_warm_read_releases_mutex_before_validate.rs`
/// concurrency-fitness discriminator can arm + assert the post-fix
/// snapshot+outside-lock-validate invariant.
pub use crate::semantic_query_memo::{ValidateRunningProbeGuard, VALIDATE_RUNNING_PROBE_TEST_LOCK};

/// Integration tests that drive the counter-helper dual-target
/// write (`record_inflight_aborted_retry` /
/// `record_cold_abort_swept`) need a `DepSignature` constructor
/// to feed `execute_cooperative` and a guard struct to force the
/// cold-abort branch. Both surfaces are `#[doc(hidden)]` and
/// gated through this `for_tests` module so production callers
/// never reach them.
pub use crate::semantic_query_memo::{
    empty_signature_for_tests, test_trigger_inflight_abort, TestForceColdAbortGuard,
};

/// Carrier type for cache-entry dependency signatures. Integration
/// tests construct `ReadSetSignature` directly when seeding
/// fixtures into `ComponentMetaResultEntry` / `MaterializeStructureEntry` /
/// `OwnerImportSurface` / `MemoEntry`.
pub use crate::fact_signature_helpers::ReadSetSignature;

/// Re-export the cooperative-admission outcome enum so integration
/// tests in `crates/verter_session/tests/*.rs` can name the type
/// (`cache_runtime` is `pub(crate)`, so the canonical path is not
/// reachable from outside the crate). Internal production callers
/// reach `crate::cache_runtime::singleflight::ComputeAdmission`
/// directly.
pub use crate::cache_runtime::singleflight::ComputeAdmission;

/// Constructs `ComputeAdmission::Failed` for the
/// `compute_admission_failed_variant_is_constructible` discriminator
/// in `tests/cases/g_block/block_1_i_discriminators.rs`. The Failed variant is
/// part of the three-variant contract for
/// `cooperative_admit_with_post_publish`; this helper proves it is
/// constructible so the variant cannot be silently dropped.
pub fn cooperative_admission_failed_variant_for_tests(
) -> crate::cache_runtime::singleflight::ComputeAdmission<(), ()> {
    crate::cache_runtime::singleflight::ComputeAdmission::Failed
}

/// Fan `sig` into every active tracer on the current thread's TLS
/// stack. Integration tests use this to verify that the multi-level
/// fan-out delivers observations into all nested tracer scopes without
/// going through `FactReadSetCell::observe` directly (which only writes
/// to the cell it's called on, not the full stack).
pub fn observe_fan_out_borrowed_for_tests(sig: &[crate::resolver_core::FactVersionRef]) {
    crate::resolver_core::resolver_context::observe_fan_out_borrowed(sig);
}

/// Bracket one cold-compute closure with a push-style fact tracer.
///
/// Thin re-export of [`crate::fact_signature_helpers::install_fact_tracer`]
/// for integration tests that need to verify tracer finalisation,
/// overflow telemetry, and the returned `FactReadSetFinalise` variant.
pub fn install_fact_tracer_for_tests<F, R>(
    host: &crate::VerterHost,
    f: F,
) -> (R, crate::resolver_core::FactReadSetFinalise)
where
    F: FnOnce() -> R,
{
    crate::fact_signature_helpers::install_fact_tracer(
        &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host),
        f,
    )
}

/// Open a REAL cacheability tracer scope and hand `f` the scope's
/// `CacheabilityProbe` — the same primitive every production producer uses.
///
/// The shared-cache funnels REQUIRE a probe, and a probe can be minted only by
/// [`crate::fact_signature_helpers::with_cacheability_scope`]. An integration
/// test that drives a funnel directly (`RouteDb`, `ImportedRootDb`) therefore
/// needs a scope of its own. This is NOT an escape hatch around the admission
/// contract — it IS the contract: a test whose closure consumes a non-cacheable
/// read is refused admission exactly as production is.
///
/// Returns `(value, non_cacheable)` — the scope's verdict, sampled after it
/// pops.
pub fn with_cacheability_scope_for_tests<F, R>(host: &crate::VerterHost, f: F) -> (R, bool)
where
    F: for<'t> FnOnce(&crate::fact_signature_helpers::CacheabilityProbe<'t>) -> R,
{
    crate::fact_signature_helpers::with_cacheability_scope(
        &crate::fact_signature_helpers::FactTracerBasisSource::unbound(host),
        f,
    )
}

/// Convert a dispatch-fence `DepSignature` into a
/// `Vec<FactVersionRef>`.
///
/// Re-export for integration tests verifying the bridge conversion.
pub fn dep_signature_to_fact_signature_for_tests(
    sig: &crate::semantic_query::DepSignature,
) -> Vec<crate::resolver_core::FactVersionRef> {
    crate::fact_signature_helpers::dep_signature_to_fact_signature(sig)
}

/// Read `host`'s per-host overflow-at-install counter value.
///
/// Integration tests use this to verify that `FactSignatureOverflow`
/// telemetry fires and the counter increments on overflow. Per-host so
/// an overflow forced on one host never bumps the counter another host's
/// delta assertion reads.
pub fn read_signature_overflow_at_install(host: &crate::VerterHost) -> u64 {
    crate::fact_signature_helpers::read_signature_overflow_at_install(host)
}

/// Arm the materialiser's test-only fact-injection knob with `n`
/// synthetic `FileWholeHash` observations per cold-compute call.
/// When `n > FACT_SIGNATURE_CAP` (1024), the cold compute's
/// installed fact tracer finalises with `Overflow`, exercising the
/// `materialize_structure_overflow_refusals` admission-refusal path
/// without requiring a workspace fixture that organically emits
/// thousands of facts. The returned guard zeroes the knob on drop.
///
/// Integration tests in
/// `crates/verter_session/tests/cases/g_family/family_bcd_*overflow*.rs` use
/// this to discriminate the `MaterializeOutcome::Value` vs `Tainted`
/// distinction on admission refusal. Per-host so the forced state never
/// leaks into a concurrent materialise on a different host.
pub fn materialize_force_overflow_observations_for_tests(
    host: &crate::VerterHost,
    n: usize,
) -> MaterializeForceOverflowGuard<'_> {
    MaterializeForceOverflowGuard::arm(host, n)
}

/// Host-scoped RAII guard that arms and clears the per-host materialiser
/// fact-injection knob
/// [`crate::VerterHost::materialize_force_overflow_observations`].
///
/// When the knob is set to `N > 0`, the materialiser's cold-compute
/// closure observes `N` synthetic `FileWholeHash` facts via
/// `observe_fan_out` BEFORE returning, deterministically forcing the
/// installed fact tracer to either overflow (when `N > FACT_SIGNATURE_CAP`)
/// or accumulate a large signature. Drives the discriminating
/// Overflow-returns-valid-result test without a pathological workspace
/// fixture.
///
/// The guard lives here (not in the seal-scoped
/// `component_meta_materialize.rs`) so the resolver-tier seal — which
/// forbids naming the concrete `VerterHost` type — is preserved. The
/// production cold path reads the knob through
/// `ctx.host_for_fact_tracer_install()`; only this test-only guard needs
/// to name the host directly to arm/clear the field.
pub struct MaterializeForceOverflowGuard<'h> {
    host: &'h crate::VerterHost,
}

impl<'h> MaterializeForceOverflowGuard<'h> {
    /// Set `host`'s forced observation count to `n` and return the guard.
    fn arm(host: &'h crate::VerterHost, n: usize) -> Self {
        host.materialize_force_overflow_observations
            .store(n, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

impl Drop for MaterializeForceOverflowGuard<'_> {
    fn drop(&mut self) {
        self.host
            .materialize_force_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Arm the per-host materialiser GENUINE-in-scope-partial injection knob
/// and return an RAII guard that clears it on drop.
///
/// When armed, the materialiser's cold-compute closure folds a partial
/// into its active [`crate::request_context::ColdComputeCompletenessScope`]
/// via the EXACT production rail a budget-tripped child read uses
/// ([`crate::request_context::mark_request_result_partial`]).
/// The per-cold-compute completeness therefore goes `Partial` and the
/// `MaterializeStructureDb` admission gate
/// (`refuse_result_cache_admission_if_partial`) must refuse the entry —
/// the SAME outcome a real budget trip produces, with a deterministic
/// trigger. Per-host so the forced state never leaks into a concurrent
/// materialise on a different host.
pub fn materialize_force_in_scope_partial_for_tests(
    host: &crate::VerterHost,
) -> MaterializeForceInScopePartialGuard<'_> {
    MaterializeForceInScopePartialGuard::arm(host)
}

/// Host-scoped RAII guard for the per-host materialiser in-scope-partial
/// injection knob
/// [`crate::VerterHost::materialize_force_in_scope_partial`]. Mirrors
/// [`MaterializeForceOverflowGuard`] for the genuine-partial fold path.
pub struct MaterializeForceInScopePartialGuard<'h> {
    host: &'h crate::VerterHost,
}

impl<'h> MaterializeForceInScopePartialGuard<'h> {
    /// Arm `host`'s in-scope-partial knob and return the guard.
    fn arm(host: &'h crate::VerterHost) -> Self {
        host.materialize_force_in_scope_partial
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

impl Drop for MaterializeForceInScopePartialGuard<'_> {
    fn drop(&mut self) {
        self.host
            .materialize_force_in_scope_partial
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Arm the per-host mid-compute generation-bump knob
/// ([`crate::VerterHost::materialize_force_mid_compute_generation_bump`])
/// and return an RAII guard that clears it on drop. The next
/// materialiser cold compute bumps the project generation once, so the
/// runtime's post-compute revalidation rejects the freshly-built entry
/// through the exact production admission-refusal path.
pub fn materialize_force_mid_compute_generation_bump_for_tests(
    host: &crate::VerterHost,
) -> MaterializeForceMidComputeGenerationBumpGuard<'_> {
    host.materialize_force_mid_compute_generation_bump
        .store(true, std::sync::atomic::Ordering::Relaxed);
    MaterializeForceMidComputeGenerationBumpGuard { host }
}

/// Host-scoped RAII guard for the mid-compute generation-bump knob.
pub struct MaterializeForceMidComputeGenerationBumpGuard<'h> {
    host: &'h crate::VerterHost,
}

impl Drop for MaterializeForceMidComputeGenerationBumpGuard<'_> {
    fn drop(&mut self) {
        self.host
            .materialize_force_mid_compute_generation_bump
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Arm the per-host relation-memo fact-injection knob and return an RAII
/// guard that zeroes it on drop. Mirrors
/// [`materialize_force_overflow_observations_for_tests`] for the relation
/// engine's overflow-refusal path.
pub fn relation_force_overflow_observations_for_tests(
    host: &crate::VerterHost,
    n: usize,
) -> RelationForceOverflowGuard<'_> {
    RelationForceOverflowGuard::arm(host, n)
}

/// Host-scoped RAII guard that arms and clears the per-host relation-memo
/// fact-injection knob
/// the host's `relation_knobs.force_overflow_observations` knob.
///
/// When the knob is set to `N > 0`, the relation engine's cold-compute path
/// observes `N` synthetic `FileWholeHash` facts before finalising the
/// read-set, deterministically forcing overflow (when `N > FACT_SIGNATURE_CAP`)
/// so the overflow-returns-result-without-admission test discriminates without
/// a pathological multi-file fixture. The knob is zeroed on drop so a panicking
/// test never leaks the forced state into a concurrent relation on another host.
pub struct RelationForceOverflowGuard<'h> {
    host: &'h crate::VerterHost,
}

impl<'h> RelationForceOverflowGuard<'h> {
    /// Set `host`'s forced observation count to `n` and return the guard.
    fn arm(host: &'h crate::VerterHost, n: usize) -> Self {
        host.relation_knobs
            .force_overflow_observations
            .store(n, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

impl Drop for RelationForceOverflowGuard<'_> {
    fn drop(&mut self) {
        self.host
            .relation_knobs
            .force_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Arm the per-host augmentation-folder torn-contributor injection knob and
/// return an RAII guard that clears it on drop. Mirrors
/// [`relation_force_overflow_observations_for_tests`] for the cross-file
/// declaration-augmentation fold's `source_env_unobservable` no-warm rail.
///
/// Gated `#[cfg(any(test, feature = "test-support"))]` alongside the host field
/// and the collection-side load: a production build carries no knob at all.
#[cfg(any(test, feature = "test-support"))]
pub fn augmentation_force_source_env_unobservable_for_tests(
    host: &crate::VerterHost,
    forced: bool,
) -> AugmentationForceUnobservableGuard<'_> {
    AugmentationForceUnobservableGuard::arm(host, forced)
}

/// Host-scoped RAII guard that arms and clears the per-host augmentation-folder
/// torn-contributor injection knob
/// [`crate::VerterHost::augmentation_force_source_env_unobservable`].
///
/// When armed, the shared augmenter-fold treats EVERY augmenter as an
/// unobservable (torn / unhealable / unservable) contributor, so the collector
/// yields the tainted state — exercising the fold of that no-warm bit into the
/// enclosing query's `QueryBuildOutput.cache_suppress` without a torn multi-file
/// fixture. The knob is cleared on drop so a panicking test never leaks the
/// forced state into a concurrent request on another host.
#[cfg(any(test, feature = "test-support"))]
pub struct AugmentationForceUnobservableGuard<'h> {
    host: &'h crate::VerterHost,
}

#[cfg(any(test, feature = "test-support"))]
impl<'h> AugmentationForceUnobservableGuard<'h> {
    /// Set `host`'s torn-contributor injection flag and return the guard.
    fn arm(host: &'h crate::VerterHost, forced: bool) -> Self {
        host.augmentation_force_source_env_unobservable
            .store(forced, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for AugmentationForceUnobservableGuard<'_> {
    fn drop(&mut self) {
        self.host
            .augmentation_force_source_env_unobservable
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Arm `host`'s compile-tier test-only fact-injection knob with `n`
/// synthetic `FileWholeHash` observations per cold-compute call. When
/// `n > FACT_SIGNATURE_CAP` (1024), the cold compute's installed fact
/// tracer finalises with `Overflow`, exercising the
/// refuse-publish-on-overflow contract on `CompileSlot` without
/// requiring a workspace fixture that organically emits thousands of
/// facts. The returned guard zeroes the knob on drop. Per-host so the
/// forced state never leaks into a concurrent compile on a different
/// host.
pub fn compile_force_overflow_observations_for_tests(
    host: &crate::VerterHost,
    n: usize,
) -> CompileForceOverflowGuard<'_> {
    CompileForceOverflowGuard::arm(host, n)
}

#[doc(hidden)]
pub use crate::host_resolve::CompileForceOverflowGuard;

/// Reset `host`'s compile-tier prefetch invocation counter to zero. Call
/// immediately before a cold compute so the post-compute read counts
/// only that compute. The cold-compute path installs the prefetch ONLY
/// for `Session` cache mode (it pre-populates the compile-tier fact
/// tracer, which is itself `Session`-only), so a routing test arms this,
/// runs one cold compute per requested mode, and asserts the counter
/// stays `0` for `Content` / `Stateless` and increments for `Session`.
pub fn reset_compile_tier_prefetch_invocations_for_tests(host: &crate::VerterHost) {
    crate::host_resolve::reset_compile_tier_prefetch_invocations(host);
}

/// Read `host`'s compile-tier prefetch invocation counter. Pair with
/// [`reset_compile_tier_prefetch_invocations_for_tests`] around a single
/// cold compute for a deterministic observation of the Session-only
/// prefetch gate.
pub fn compile_tier_prefetch_invocations_for_tests(host: &crate::VerterHost) -> usize {
    crate::host_resolve::compile_tier_prefetch_invocations(host)
}

/// Read the materialiser's `materialize_structure_overflow_refusals`
/// counter for the given host. Test surface; production callers reach
/// this through the `host_audit_runtime().snapshot()` provenance
/// surface.
pub fn read_materialize_structure_overflow_refusals(host: &crate::VerterHost) -> u64 {
    host.provenance
        .materialize_structure_overflow_refusals
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// Read the workspace `semantic_transitive` dependency set for
/// `canonical_id` (the cross-file macro/type dependency axis maintained
/// by `sync_transitive_macro_type_dependencies` →
/// `WorkspaceAccess::replace_semantic_transitive`). Returns an empty set
/// when the file has no snapshot. Integration tests use this to assert
/// the semantic axis is CLEARED when a compute carries no
/// `macro_type_deps` — the clearing is unconditional and must survive
/// the empty-deps collector-setup skip. `WorkspaceAccess` is reachable
/// only through `ws()` (`pub(crate)`), so this thin read is routed here.
pub fn workspace_semantic_transitive_deps_for_tests(
    host: &crate::VerterHost,
    canonical_id: &str,
) -> std::collections::BTreeSet<String> {
    host.ws()
        .dependency_snapshot(canonical_id)
        .map(|snap| snap.semantic_transitive)
        .unwrap_or_default()
}

/// Arm the dispatch's test-only Parse-fact injection slot. The
/// next cold build through `ProjectSemanticDispatch::execute()`
/// observes the supplied `Parse(...)` fact onto every active
/// tracer BEFORE the inner build runs. The returned guard clears
/// the slot on drop. Integration tests in
/// `crates/verter_session/tests/dispatch_*_fact_*.rs` use this to
/// discriminate the cold-publish → warm-hit path-precise survival
/// contract.
pub fn dispatch_inject_parse_fact_for_tests(
    fact: crate::resolver_core::FactVersionRef,
) -> crate::project_semantic_dispatch::DispatchInjectParseFactGuard {
    crate::project_semantic_dispatch::DispatchInjectParseFactGuard::arm(fact)
}

/// Test-visibility shim exposing the canonical
/// [`execute_type_node`](crate::semantic_query::SemanticQueryApi::execute_type_node)
/// to integration-test crates (dispatch is `pub(crate)`); it returns the typed
/// [`SemanticQueryOutput<SemanticNodeId>`](crate::semantic_query::SemanticQueryOutput)
/// verbatim — NOT a stripped node API and NOT a second resolver/admission path.
/// `ProjectSemanticDispatch` is `pub(crate)`, so test crates in
/// `crates/verter_session/tests/**` cannot construct it directly; this forwards
/// to the one canonical dispatch and hands back its result unchanged.
pub fn dispatch_execute_type_node_for_tests(
    host: &crate::VerterHost,
    key: crate::semantic_query::SemanticQueryKey,
) -> crate::semantic_query::QueryResult<
    crate::semantic_query::SemanticQueryOutput<crate::semantic_query::SemanticNodeId>,
> {
    use crate::semantic_query::SemanticQueryApi;
    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host).execute_type_node(key)
}

/// Production-shaped, NON-DECONSTRUCTABLE builder of a sealed
/// [`SemanticQueryKey::Instantiate`](crate::semantic_query::SemanticQueryKey)
/// key for integration tests.
///
/// The `Instantiate` payload is the opaque
/// [`InstantiateKey`](crate::semantic_query::InstantiateKey) (private
/// fields) whose [`InstantiateContext`](crate::semantic_query::InstantiateContext)
/// carries the sealed, `pub(crate)` [`InstantiateBodySource`](crate::semantic_query::InstantiateBodySource)
/// source-kind axis — none of which an external crate can construct.
/// This helper is the ONLY way `tests/cases/**` can mint the key, and it
/// hands back the OPAQUE `SemanticQueryKey` — never a raw
/// `InstantiateContext` / `InstantiateBodySource` and never a
/// deconstructable `{ base, args, context }` shape.
///
/// It routes through the SAME production choke point
/// (`ProjectSemanticDispatch::instantiate_context_for`), so the caller
/// does NOT choose the source kind: a real canonical base ⇒ `FileBacked(P)`
/// (the live parse-env dim); the sentinel non-file bases (`""` /
/// `"__builtin__"` / `"<synthetic>"`) ⇒ `NonFile`. A test therefore cannot
/// forge the unsound `NonFile`-context-on-a-real-file-base shape.
///
/// Gated `#[cfg(any(test, feature = "test-support"))]` — STRICTER than the
/// enclosing `debug_assertions`-gated module, so a plain debug build
/// (e.g. the debug LSP) cannot reach it; only genuine test / test-support
/// builds compile it. `test-support` is turned on for `verter_session`'s
/// own test targets by the `[dev-dependencies]` self-edge, so BOTH gate
/// surfaces (`cargo nextest run --workspace` and
/// `cargo test -p verter_session --tests`) compile it.
#[cfg(any(test, feature = "test-support"))]
pub fn instantiate_key_for_tests(
    host: &crate::VerterHost,
    base: crate::semantic_query::ResolvedDeclSlotIdentity,
    args: std::sync::Arc<[crate::semantic_query::SemanticNodeId]>,
    prc: crate::semantic_query::ProjectionReductionContext,
) -> crate::semantic_query::SemanticQueryKey {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let context = dispatch.instantiate_context_for(base.defining_canonical.as_ref(), prc);
    crate::semantic_query::SemanticQueryKey::Instantiate(
        crate::semantic_query::InstantiateKey::new(base, args, context),
    )
}

/// Reduce a `MergedDecl` contributor list to its canonical peer-merged graph
/// node (the mutating reducer that interns the `Object` / heritage
/// `Intersection`). Exposed so display guards can assert the read-only display
/// projection renders byte-identically to this canonical reduced surface — the
/// two paths share one peer-merge engine, so any divergence is a regression.
pub fn reduce_merged_decl_to_graph_node(
    store: &crate::semantic_query_memo::SemanticGraphStore,
    contributors: &[crate::semantic_query::SemanticNodeId],
) -> crate::semantic_query::SemanticNodeId {
    crate::project_semantic_dispatch::walk::reduce_merged_decl_with_graph(store, contributors)
}

/// Drive `ProjectSemanticDispatch::lower_type_expr_in_scope_with_context`
/// from integration tests so they can exercise the
/// `structural_transit_with_mode` substrate from outside the crate.
pub fn dispatch_lower_type_expr_in_scope_with_context_for_tests(
    host: &crate::VerterHost,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    context: crate::semantic_query::ProjectionReductionContext,
) -> Option<crate::semantic_query::SemanticNodeId> {
    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host)
        .lower_type_expr_in_scope_with_context(scope_canonical_id, expr, context)
}

/// Integration-test shim that drives the
/// `substitute_semantic_type_param` helper so its hash-cons
/// discriminator tests can exercise the memo with controlled
/// input triples without reaching through the entire query
/// pipeline.
pub fn dispatch_substitute_for_tests(
    host: &crate::VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    parameter_node: crate::semantic_query::SemanticNodeId,
    arg: crate::semantic_query::SemanticNodeId,
) -> crate::semantic_query::SemanticNodeId {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    dispatch.substitute_semantic_type_param_for_tests(node, parameter_node, arg)
}

/// Integration-test shim that drives the
/// `evaluate_deferred_semantic_node_with_context` helper so its
/// hash-cons + depth-budget discriminator tests can exercise
/// the evaluator with controlled (node, context) inputs
/// without reaching through the full query pipeline. Returns the typed
/// outcome as a `(node, completeness)` pair — the completeness rides
/// along so depth/budget discriminators can assert the typed partial
/// reasons; never a restored bare-node API.
///
/// Gated `#[cfg(any(test, feature = "test-support"))]` — STRICTER than the
/// enclosing `debug_assertions`-gated module, mirroring
/// [`instantiate_key_for_tests`]. The `(node, completeness)` pair is a `.0`
/// bare-node escape from the node-hiding rail, so it must NOT exist in an
/// ordinary debug build (e.g. the debug LSP): `test-support` is off in
/// `default` yet turned on for `verter_session`'s own test / integration
/// targets by the `[dev-dependencies]` self-edge, so this shim is reachable
/// from genuine test code in BOTH the unit and integration builds and is
/// COMPILE-ABSENT in every production profile.
#[cfg(any(test, feature = "test-support"))]
pub fn dispatch_evaluate_deferred_for_tests(
    host: &crate::VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> (
    crate::semantic_query::SemanticNodeId,
    crate::semantic_query::ResultCompleteness,
) {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    dispatch.evaluate_deferred_semantic_node_with_context_for_tests(node, context)
}

/// Returns `true` iff `host.active_session_view()` returns `None`.
///
/// This shim is needed because `ResolverContext` is sealed — integration
/// tests cannot call the trait method directly. The shim calls through the
/// sealed trait on the concrete `VerterHost` impl.
pub fn active_session_view_is_none_for_tests(host: &crate::VerterHost) -> bool {
    use crate::resolver_core::ResolverContext;
    host.active_session_view().is_none()
}

/// Drive the AppConfigNoOverrideProofDb production producer
/// from integration tests. The producer takes a `&dyn ResolverContext`
/// internally (per the seal contract); this wrapper accepts a
/// concrete `&VerterHost` reference so integration tests in
/// `tests/cases/g_family/family_bcd_*.rs` can drive it end-to-end.
pub fn app_config_no_override_proof_get_or_compute_for_tests(
    host: &crate::VerterHost,
    key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
) -> Option<std::sync::Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
    crate::component_meta_caches::app_config_no_override_proof_get_or_compute(host, key)
}

/// Drive the family-A `BinderIdentityFacts` production producer
/// ([`crate::binder_identity_facts::produce_binder_identity_facts`])
/// from integration tests. The producer takes a `&dyn ResolverContext`
/// internally (per the seal contract); this wrapper accepts a concrete
/// `&VerterHost` so `tests/cases/**` can drive the demand-produced
/// artifact end-to-end (warm hit / cold recompute / invalidation).
pub fn binder_identity_facts_get_or_compute_for_tests(
    host: &crate::VerterHost,
    canonical: &str,
) -> Option<std::sync::Arc<crate::binder_identity_facts::BinderIdentityFactsEntry>> {
    crate::binder_identity_facts::produce_binder_identity_facts(host, canonical)
}

/// Return the published `ComponentMetaResultEntry`'s
/// `ReadSetSignature` for `owner_canonical`, or `None` when no
/// entry is cached for the owner's current content.
///
/// The lookup key is composed exactly as
/// `publish_component_meta_cache_entry` composes it — owner
/// canonical + current `IndexedReady` whole-hash + the default
/// `ComponentMetaOptions` fingerprint — so it matches the
/// published entry. Integration tests in
/// `tests/cases/g_component/component_meta_signature_is_tracer_owned.rs` and
/// `tests/cases/g_component/component_meta_family_producers_observe_cross_file_deps.rs`
/// use it to inspect the tracer-owned `facts` rail of the
/// published signature.
pub fn component_meta_result_signature_for_owner(
    host: &crate::VerterHost,
    owner_canonical: &str,
) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
    let whole_hash = host
        .ensure_indexed_ready(owner_canonical)
        .map(|ir| ir.whole_hash)?;
    // Address the exact slot production published — same env axes via the
    // canonical builder, not a hand-rolled 2-field key.
    let key = host.component_meta_result_key(
        owner_canonical,
        &crate::host_manage::ComponentMetaOptions::default(),
    );
    host.project_type_store()
        .component_meta_results()
        .get(&key, whole_hash)
        .map(|entry| entry.read_set_signature.clone())
}

/// Compute the FINALIZED fact-tracer read set for a cold
/// `get_component_meta` of `owner_canonical`. Installs a fresh
/// `with_fact_tracer` scope around `resolve_component_meta` +
/// `extract_component_meta_from_resolved` (the exact body the
/// production `get_component_meta` cold path traces) and returns
/// `read_set.finalise()`.
///
/// Returns `None` when the owner does not resolve to a component.
/// Integration tests use this to assert the published
/// `ComponentMetaResultEntry` signature EQUALS the finalized
/// tracer read set (the tracer-owned signature).
pub fn component_meta_cold_traced_read_set_for_tests(
    host: &crate::VerterHost,
    owner_canonical: &str,
) -> Option<crate::resolver_core::FactReadSetFinalise> {
    let canonical = host.resolve_alias_or_canonical(owner_canonical);
    let (resolved_opt, read_set) =
        host.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
            crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
                host.resolve_component_meta(
                    canonical.as_str(),
                    crate::types::ProjectionMode::Expanded,
                )
                .map(|resolved| {
                    let _ = crate::host_manage::extract_component_meta_from_resolved(
                        host,
                        canonical.as_str(),
                        &resolved,
                        true,
                        ctx,
                    );
                })
            })
        });
    resolved_opt?;
    Some(read_set.finalise())
}

/// Discriminating probes for the dispatch DepSignature→fact bridges. Used by
/// `tests/cases/g_misc0/dispatch_bridges_convert_project_generation.rs`.
pub use crate::tests::dispatch_bridges::{
    dispatch_dep_signature_facts_for_tests, observe_fence_entry_for_tests,
};

/// Test-only probe: returns `true` iff the scheduler holds an
/// artifact snapshot for the `(canonical, profile)` pair. Mirrors
/// the artifact-substrate side of the compile-tier carrier
/// invariant `present in compile_slots ⇒ admitted cache entry`:
/// after an overflowed compile the scheduler must NOT carry an
/// artifact for the refused profile.
pub fn compile_scheduler_artifact_present_for_tests(
    host: &crate::VerterHost,
    canonical_id: &str,
    profile: &crate::types::CompileProfile,
) -> bool {
    let profile_hash = crate::hash::compile_profile_hash(profile);
    host.scheduler
        .try_get_artifact(canonical_id, profile_hash)
        .is_some()
}

/// Test-only probe: returns `true` iff the scheduler holds ANY
/// artifact snapshot for the `(canonical, profile)` pair regardless
/// of generation coherence. Discriminates the eviction-on-refusal
/// invariant from the generation-coherence filter on
/// [`try_get_artifact`](verter_scheduler::scheduler::Scheduler::try_get_artifact):
/// a stale artifact left in the map (because the refusal arm did not
/// call `remove_artifact_if_not_newer_than`) is invisible to
/// `try_get_artifact` after a generation bump, but visible here via
/// `last_known_good_artifact`. After an overflowed compile that
/// follows a successful one this MUST return `false`.
pub fn compile_scheduler_last_known_good_artifact_present_for_tests(
    host: &crate::VerterHost,
    canonical_id: &str,
    profile: &crate::types::CompileProfile,
) -> bool {
    let profile_hash = crate::hash::compile_profile_hash(profile);
    host.scheduler
        .try_get_last_known_good(canonical_id, profile_hash)
        .is_some()
}

/// Criterion-only projection primitive harness. The module is compiled
/// solely by the explicit `test-support` feature used by the checked-in
/// projection safety benchmark; default production builds do not expose
/// it. Re-exported at the crate root as `verter_session::projection_bench_support`
/// (see `lib.rs`) so `benches/projection_safety_bench.rs` keeps a stable path.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod projection_bench_support {
    pub use crate::project_semantic_dispatch::locator_view::{
        ProjectionBenchCase, ProjectionBenchHarness,
    };
}

// ── Private flow-solve completeness-proof surface (hermetic) ────────────
// The fixtures' content keys stay `pub(crate)`; construction/planning go
// through the wrapper below.
/// The store-retained structural plan carrier the demand planner consumes:
/// the slice identity minted over the bound graph plus the selection it
/// covers. Tests pass it around as an opaque provenance token.
pub use crate::cache_runtime::flow_slice_node::PlannedFlowSlice;
pub use crate::project_semantic_dispatch::dispatch_txn::flow_obligation_state::*;
pub use crate::project_semantic_dispatch::dispatch_txn::ObligationRuntime;
pub use crate::project_semantic_dispatch::flow_solve::*;

/// One hermetic flow-solve fixture: `source`'s first function declaration,
/// parsed and indexed ONCE — the store-minted bound graph (skeleton +
/// graph sealed to the content key by `FunctionFlowGraphStore`) plus the
/// frame's real binding inventory from the `FunctionProgramIndex`.
pub struct FlowGraphFixtureForTests {
    bound: crate::cache_runtime::flow_slice_node::BoundFlowGraph,
    inventory: FlowBindingInventory,
}

#[rustfmt::skip]
impl FlowGraphFixtureForTests {
    /// The ONE structural plan the production hash node would retain for
    /// this demand over this fixture's bound graph: planned once by graph
    /// reachability and sealed with the slice identity minted over the
    /// SAME graph — the exact `PlannedFlowSlice` shape the production
    /// hash node's compute retains on its published outcome.
    pub fn retained_plan(&self, request: &FlowDemandRequest) -> Result<PlannedFlowSlice, FlowDemandPlanError> {
        use verter_semantic::analysis::flow::hashing::compute_flow_slice_hash;
        use verter_semantic::analysis::flow::peeker::{ReturnPathPeeker, SliceDemand};
        let subject = crate::project_semantic_dispatch::flow_solve::derive_demand_subject(&request.query)?;
        let bundle = self.bound.bundle();
        let demand = SliceDemand::for_return_projection(&bundle.skeleton, &subject.projection_path);
        let selection = ReturnPathPeeker::new(&bundle.graph)
            .plan(&demand, &request.resources.slice_budget)
            .map_err(FlowDemandPlanError::SliceBudget)?;
        let hash = compute_flow_slice_hash(&selection, &bundle.graph, &bundle.skeleton);
        Ok(PlannedFlowSlice::new(hash, selection))
    }

    /// Plan `request` over this fixture's store-bound graph and binding
    /// inventory. The request carries no graph axis and no subject axis:
    /// the basis takes the body identity from the bound graph's key and
    /// the subject from the query's own demand payload. The fixture runs
    /// the ONE structural plan the production hash node would retain for
    /// this demand and hands it to the demand planner, which assembles
    /// obligations without re-planning.
    pub fn build_plan(&self, request: FlowDemandRequest) -> Result<FlowDemandPlan, FlowDemandPlanError> {
        let retained = self.retained_plan(&request)?;
        self.build_plan_with_retained(request, &retained)
    }

    /// Plan `request` over this fixture's bound graph against an EXPLICIT
    /// retained selection — the adversarial seam: a selection retained
    /// for another graph or another demand must be a typed planning
    /// error, never a proof plan.
    pub fn build_plan_with_retained(&self, request: FlowDemandRequest, retained: &PlannedFlowSlice) -> Result<FlowDemandPlan, FlowDemandPlanError> {
        crate::project_semantic_dispatch::flow_solve::build_flow_demand_plan(request, &self.bound, retained, &self.inventory)
    }
}

/// Parse `source`, build the skeleton + graph of its first function
/// declaration through the store's minting path (sealed to the pinned
/// content key, body hashes tagged `body_hash_tag`), derive the source's
/// REAL parse identity (exact parse key + language row), and resolve the
/// frame's binding inventory through the real `FunctionProgramIndex`.
#[rustfmt::skip]
pub fn flow_graph_fixture_for_tests(source: &str, body_hash_tag: u8) -> FlowGraphFixtureForTests {
    flow_graph_fixture(source, body_hash_tag, verter_language::FileLanguage::script(verter_language::ScriptSourceType::Ts))
}

/// The same fixture under a different runtime-authoritative language row:
/// the parse identity is derived for REAL from the same source under that
/// language (never a tagged stand-in), so the two fixtures' keys differ in
/// exactly the source-identity axes.
#[rustfmt::skip]
pub fn flow_graph_fixture_for_tests_with_language(source: &str, body_hash_tag: u8, file_language: verter_language::FileLanguage) -> FlowGraphFixtureForTests {
    flow_graph_fixture(source, body_hash_tag, file_language)
}

#[rustfmt::skip]
fn flow_graph_fixture(source: &str, body_hash_tag: u8, file_language: verter_language::FileLanguage) -> FlowGraphFixtureForTests {
    use verter_semantic::analysis::flow::{FunctionBodySource, build_function_body_skeleton};
    use verter_semantic::analysis::function_program::{build_function_program_index, FunctionDeclarationRef, FunctionProgramKey};
    use verter_semantic::analysis::top_level_owners::TopLevelOwnerTable;
    let oxc_source_type = match &file_language {
        verter_language::FileLanguage::Script { source_type, .. } => match source_type {
            verter_language::ScriptSourceType::Ts => oxc_span::SourceType::ts(),
            verter_language::ScriptSourceType::Tsx => oxc_span::SourceType::tsx(),
            other => panic!("the hermetic flow fixture serves script languages ts/tsx, got {other:?}"),
        },
        other => panic!("the hermetic flow fixture serves script languages, got {other:?}"),
    };
    // The parse identity is DERIVED from the real source under the real
    // language row — the same derivation the production
    // `FileArtifactKey::for_source_identity` performs — never a constant.
    let (_, parse_key) = verter_language::default_parse_identity_for(source, &file_language)
        .expect("the hermetic fixture's script language derives a parse identity");
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_source_type).parse();
    assert!(parsed.errors.is_empty(), "fixture must parse");
    let function = parsed.program.body.iter().find_map(|s| match s { oxc_ast::ast::Statement::FunctionDeclaration(f) => Some(f), _ => None }).expect("fixture must contain a function declaration");
    let name = function.id.as_ref().expect("named function").name.as_str();
    let canonical_id: std::sync::Arc<str> = std::sync::Arc::from("/flow_solve_fixture.ts");
    let skeleton = build_function_body_skeleton(&FunctionBodySource::from_function(function).expect("bodied function"));
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index = build_function_program_index(&parsed.program, source, &owners, canonical_id.clone());
    let declaration = FunctionDeclarationRef { owner: verter_type_expr::TopLevelOwnerId::ordinary_file(), name: std::sync::Arc::from(name), space: verter_semantic::facts::SymbolSpace::Value };
    let function = FunctionProgramKey { declaration, part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody, overload_ordinal: 0 };
    let inventory = FlowBindingInventory {
        bindings: std::sync::Arc::clone(&index.get(&function).expect("the fixture function is indexed").entry().bindings),
    };
    let key = crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey {
        canonical_id, function, parse_env_hash: [0u8; 16],
        flow_body_stable_hash: [body_hash_tag; 16], flow_body_exact_hash: [body_hash_tag; 16],
        parse_key, file_language,
        build_toolchain_fingerprint: crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
    };
    let store = crate::cache_runtime::flow_slice_node::FunctionFlowGraphStore::new();
    let bound = store.mint_bound_flow_graph(key, skeleton);
    FlowGraphFixtureForTests { bound, inventory }
}

/// Mint the hermetic value payload: a clean flow-return result over `return_type`.
#[rustfmt::skip]
pub fn flow_return_result_for_tests(graph: &SemanticGraphStore, return_type: crate::semantic_query::SemanticNodeId) -> crate::semantic_query::FlowReturnResult {
    crate::semantic_query::FlowReturnResult::new(graph, return_type, false, None)
}

/// Mint a DEGRADED hermetic value payload over `return_type` — the
/// completion seal must refuse it.
#[rustfmt::skip]
pub fn degraded_flow_return_result_for_tests(graph: &SemanticGraphStore, return_type: crate::semantic_query::SemanticNodeId) -> crate::semantic_query::FlowReturnResult {
    crate::semantic_query::FlowReturnResult::new(graph, return_type, false, Some(crate::semantic_query::FlowReturnDegradation::NonCallableBinding))
}

/// Probe the no-flow allocation contract through the REAL production
/// dispatch: execute `key` through the one canonical
/// `ProjectSemanticDispatch` (the same choke point
/// [`dispatch_execute_type_node_for_tests`] forwards to) and report the
/// dispatch's flow-demand ledger footprint afterwards — `(installed
/// demand count, reserved demand storage capacity)`. An ordinary query
/// and every pending typed-gap root install zero demands and reserve
/// zero capacity.
#[rustfmt::skip]
pub fn dispatch_flow_demand_footprint_for_tests(
    host: &crate::VerterHost,
    key: crate::semantic_query::SemanticQueryKey,
) -> (usize, usize) {
    use crate::semantic_query::SemanticQueryApi;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    let _ = dispatch.execute(key);
    dispatch.flow_demand_footprint_for_tests()
}

/// Mint a raw composite-payload FIXTURE for integration tests that need
/// a pre-canonical or deliberately nested `Union` / `Intersection` graph
/// shape (display-projection guards, structural-hash discriminators).
/// Routes through the sealed `TestFixture` carrier category, which is
/// compiled solely under `cfg(any(test, feature = "test-support"))` —
/// an ordinary production build has neither this function nor the
/// category variant, so the production mint surface stays unforgeable
/// from outside the crate.
#[rustfmt::skip]
pub fn composite_fixture_for_tests<K: crate::semantic_query::composite::CompositeKind>(members: std::sync::Arc<[crate::semantic_query::SemanticNodeId]>) -> crate::semantic_query::composite::CompositeList<K> {
    crate::semantic_query::composite::CompositeList::test_fixture(members)
}
