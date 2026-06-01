//! Re-export shim for integration tests in
//! `crates/verter_session/tests/*.rs`. Integration tests build the lib
//! WITHOUT `cfg(test)` set, so they cannot reach `pub(crate)` items
//! through the normal `cfg(test)` test-helper pattern. This module is
//! gated `cfg(any(test, debug_assertions))` at its declaration site in
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
/// `OwnerImportSurface` / `RefCycleEntry` / `MemoEntry`.
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
/// in `tests/block_1_i_discriminators.rs`. The Failed variant is
/// part of the codex three-variant contract for
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
    crate::fact_signature_helpers::install_fact_tracer(host, f)
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

/// Read the current overflow-at-install counter value.
///
/// Integration tests use this to verify that `FactSignatureOverflow`
/// telemetry fires and the counter increments on overflow.
pub fn read_signature_overflow_at_install() -> u64 {
    crate::fact_signature_helpers::read_signature_overflow_at_install()
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
/// `crates/verter_session/tests/family_bcd_*_overflow*.rs` use this
/// to discriminate the
/// `MaterializeOutcome::Value` vs `Tainted` distinction on
/// admission refusal.
pub fn materialize_force_overflow_observations_for_tests(
    n: usize,
) -> crate::component_meta_materialize::MaterializeForceOverflowGuard {
    crate::component_meta_materialize::MaterializeForceOverflowGuard::arm(n)
}

/// Arm the compile-tier's test-only fact-injection knob with `n`
/// synthetic `FileWholeHash` observations per cold-compute call. When
/// `n > FACT_SIGNATURE_CAP` (1024), the cold compute's installed fact
/// tracer finalises with `Overflow`, exercising the
/// refuse-publish-on-overflow contract on `CompileSlot` without
/// requiring a workspace fixture that organically emits thousands of
/// facts. The returned guard zeroes the knob on drop.
pub fn compile_force_overflow_observations_for_tests(n: usize) -> CompileForceOverflowGuard {
    CompileForceOverflowGuard::arm(n)
}

#[doc(hidden)]
pub use crate::host_resolve::CompileForceOverflowGuard;

/// Reset the compile-tier prefetch invocation counter to zero. Call
/// immediately before a cold compute so the post-compute read counts
/// only that compute. The cold-compute path installs the prefetch ONLY
/// for `Session` cache mode (it pre-populates the compile-tier fact
/// tracer, which is itself `Session`-only), so a routing test arms this,
/// runs one cold compute per requested mode, and asserts the counter
/// stays `0` for `Content` / `Stateless` and increments for `Session`.
pub fn reset_compile_tier_prefetch_invocations_for_tests() {
    crate::host_resolve::reset_compile_tier_prefetch_invocations();
}

/// Read the compile-tier prefetch invocation counter. Pair with
/// [`reset_compile_tier_prefetch_invocations_for_tests`] around a single
/// cold compute for a deterministic observation of the Session-only
/// prefetch gate.
pub fn compile_tier_prefetch_invocations_for_tests() -> usize {
    crate::host_resolve::compile_tier_prefetch_invocations()
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

/// Drive [`crate::project_semantic_dispatch::ProjectSemanticDispatch::execute`]
/// from integration tests. Constructs a `ProjectSemanticDispatch`
/// from the `host` (the standard internal pattern) and forwards
/// the call. Used by tests that need to exercise the dispatch's
/// `install_fact_tracer` wrapper directly — the dispatch is
/// `pub(crate)` so integration test crates cannot reach it
/// otherwise.
pub fn dispatch_execute_for_tests(
    host: &crate::VerterHost,
    key: crate::semantic_query::SemanticQueryKey,
) -> crate::semantic_query::QueryResult<crate::semantic_query::SemanticNodeId> {
    use crate::semantic_query::SemanticQueryApi;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
    dispatch.execute(key)
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
/// without reaching through the full query pipeline.
pub fn dispatch_evaluate_deferred_for_tests(
    host: &crate::VerterHost,
    node: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> crate::semantic_query::SemanticNodeId {
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
/// `tests/family_bcd_*.rs` can drive it end-to-end.
pub fn app_config_no_override_proof_get_or_compute_for_tests(
    host: &crate::VerterHost,
    key: &crate::app_config_proof_db::AppConfigNoOverrideProofKey,
) -> Option<std::sync::Arc<crate::app_config_proof_db::AppConfigNoOverrideProofEntry>> {
    crate::component_meta_caches::app_config_no_override_proof_get_or_compute(host, key)
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
/// `tests/component_meta_signature_is_tracer_owned.rs` and
/// `tests/component_meta_family_producers_observe_cross_file_deps.rs`
/// use it to inspect the tracer-owned `facts` rail of the
/// published signature.
pub fn component_meta_result_signature_for_owner(
    host: &crate::VerterHost,
    owner_canonical: &str,
) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
    let whole_hash = host
        .ensure_indexed_ready(owner_canonical)
        .map(|ir| ir.whole_hash)?;
    let key = crate::component_meta_result_db::ComponentMetaResultKey {
        owner_canonical: std::sync::Arc::from(owner_canonical),
        options_fingerprint: crate::host_manage::component_meta_options_fingerprint(
            &crate::host_manage::ComponentMetaOptions::default(),
        ),
    };
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
/// tracer read set (codex item 3 — tracer-owned signature).
pub fn component_meta_cold_traced_read_set_for_tests(
    host: &crate::VerterHost,
    owner_canonical: &str,
) -> Option<crate::resolver_core::FactReadSetFinalise> {
    let canonical = host.resolve_alias_or_canonical(owner_canonical);
    let (resolved_opt, read_set) = host.with_fact_tracer(|| {
        crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
            host.resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
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

/// Discriminating probes for the dispatch DepSignature→fact
/// bridges (`accumulate_dispatch_dep_signature` /
/// `observe_fence_entry`). Used by
/// `tests/dispatch_bridges_convert_project_generation.rs`.
pub use crate::tests::dispatch_bridges::{
    accumulate_dispatch_dep_signature_for_tests, observe_fence_entry_for_tests,
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
