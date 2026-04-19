//! Solver-routing tests for the generic-navigation track (plan §6.2 +
//! §3 Phase D).
//!
//! These tests lock the architectural invariants that govern how the
//! solver hands off to the shared dispatch layer via `SemanticQueryApi`.
//! They cover:
//!
//! - Dispatch handoff: `resolve_prepared_ref` must construct
//!   `SemanticQueryKey::Instantiate { base, args }` with substitution
//!   baked into the arg node ids — not threaded through the key.
//! - Per-request arena: solver scratch nodes must NOT publish as
//!   reusable identity. Only the shared semantic graph holds warm
//!   reusable results.
//! - Single in-flight authority: the solver must never self-await or
//!   block cross-thread; those responsibilities belong exclusively to
//!   the shared dispatch layer.
//!
//! Most tests in this file are `#[ignore = "pending D1 / D2 / ..."]`
//! because the solver→dispatch cutover is a multi-commit architectural
//! change coordinated across Phase D. Un-ignore happens in the enabling
//! commit per F1's convention.
//!
//! **Cross-reference.** Plan §6.2 "Single-in-flight-authority
//! assertions" names each test; this file is the canonical location.

#![cfg(test)]

/// After D1 lands, `resolve_prepared_ref` constructs `SemanticQueryKey::Instantiate { base, args }`
/// with substitution baked into the `args` node ids before the dispatch
/// call. The key itself carries no `mode` field and no `SubstitutionEnv`
/// reference — both are forbidden per plan §7.14.
#[test]
#[ignore = "pending D1 solver handoff (requires solver NodeId ↔ SemanticNodeId translator)"]
fn solver_handoff_does_not_reference_mode_or_substitution_env() {
    // Architecturally enforced by the signature:
    //   fn build_instantiate(&self, base: SemanticNodeId, args: &Arc<[SemanticNodeId]>)
    //
    // No mode, no subst env. The D1 cutover verifies no call site
    // outside the dispatch layer synthesises an `Instantiate` key with
    // an environment attached (there is no such field on the key).
    //
    // When un-ignored: a runtime grep over solve.rs + lower.rs + relate.rs
    // asserts no reference to `Instantiate` carries a mode or env token.
}

/// Two separate `TypeQueryEngine` instances resolving the same generic
/// dedup via the shared dispatch memo. The second engine sees a warm
/// hit rather than re-walking the body.
#[test]
#[ignore = "pending D1 solver handoff implementation"]
fn instantiation_via_dispatch_dedups_across_two_type_query_engines() {
    // Plan §3 D1: "the solver's `QueryArena` remains as a per-request
    // scratch space ... but reusable semantic results live in
    // `SemanticGraphStore`'s own node arena."
    //
    // After D1: two distinct engines (two requests) calling
    // `resolve_prepared_ref(Foo<string>)` issue the same
    // `SemanticQueryKey::Instantiate { base, [string] }` and the second
    // caller observes `SemanticGraphStats::hits` incremented.
}

/// The per-request solver arena must not publish reusable identity —
/// its NodeIds never surface outside the owning `TypeQueryEngine`.
#[test]
#[ignore = "pending D1 solver handoff implementation"]
fn per_request_arena_does_not_publish_reusable_identity() {
    // Enforced by the ownership rule in plan §7.1: `QueryArena` is a
    // permanent non-authoritative per-request scratch. This test walks
    // the arena's public interface and asserts no method exposes a
    // NodeId to a consumer outside the engine's lifetime.
}

/// Self-recursive generic like `type Rec<T> = { next: Rec<T> }` must not
/// self-await when the solver re-enters the same semantic subquery. The
/// dispatcher's recursion sentinel returns immediately.
#[test]
#[ignore = "pending D1 single-in-flight-authority enforcement"]
fn solver_never_self_awaits_returns_sentinel_instead() {
    // Plan §7.7: "Exactly one in-flight authority for reusable semantic
    // work: `ProjectSemanticDispatch` / `SemanticGraphStore`." The
    // solver's own RecursionTracker demotes to a scratch marker.
}

/// Two threads calling `SemanticQueryApi::execute` on the same cold
/// entry wait in the shared Condvar — solver-side wait counters do not
/// increment.
#[test]
#[ignore = "pending D1 single-in-flight-authority enforcement"]
fn cross_thread_joiners_wait_only_in_dispatch_layer() {
    // Plan §7.7 stress: assert `SemanticGraphStats::waits_ms > 0` while
    // solver-scratch wait counters stay at 0.
}

/// N threads running full `get_component_meta` on a deeply recursive
/// generic complete under a generous timeout without deadlock.
#[test]
#[ignore = "pending D1 single-in-flight-authority enforcement"]
fn stress_parallel_requests_same_decl_no_deadlock() {
    // Plan §6.2 stress (N=16 typical): every thread returns an equal
    // top-level `SemanticNodeId`; `SemanticGraphStats::same_path_sentinel_returns >= 1`;
    // no thread shows a wait in a solver-local lock held across a
    // dispatch call.
}

/// Mutually-recursive generics `type A<T> = { b: B<T> }` /
/// `type B<T> = { a: A<T> }` — 8 parallel instantiations with distinct
/// Ts through the mutual SCC. All complete, dedup correctly, no solver
/// wait counters increment.
#[test]
#[ignore = "pending D1 single-in-flight-authority enforcement"]
fn mutually_recursive_generics_sccid_single_in_flight_authority() {}

/// Dev-time enforcement wrapper `LockAuditingGraphStore` panics if
/// `Condvar::wait` runs while the current thread holds any tracked
/// solver-local lock. Exercised by every stress test.
#[test]
#[ignore = "pending D1 lock-audit wrapper (cfg(debug_assertions))"]
fn dispatch_drop_locks_before_wait() {}

/// Maintained static list `SOLVER_DISPATCH_CALL_SITES` covers every
/// `SemanticQueryApi::execute` invocation anywhere under
/// `crates/verter_semantic/src/analysis/type_solver/**/*.rs`. The test
/// greps the source and asserts set-equality with the list.
#[test]
#[ignore = "pending D1 SOLVER_DISPATCH_CALL_SITES registry"]
fn solver_lock_audit_call_site_list_is_current() {}

// ---------------------------------------------------------------------------
// D2 — solver routes Conditional through dispatch
// ---------------------------------------------------------------------------

/// `resolve_conditional` substitutes bindings into the four node ids
/// (check / extends / true / false) eagerly, then hands off to
/// `SemanticQueryApi::execute(SemanticQueryKey::Conditional { .. })`.
/// No substitution environment crosses the key boundary.
#[test]
#[ignore = "pending D2 conditional solver handoff"]
fn solver_conditional_handoff_has_substitution_baked_into_node_ids() {}

/// A cached `Conditional` result is reused on the second solver call
/// rather than re-evaluated.
#[test]
#[ignore = "pending D2 conditional solver handoff"]
fn solver_conditional_dispatch_reuses_cached_result() {}

/// Deferred open-conditional survives scope exit (ported + tightened
/// from the existing `deferred_imported_alias_resolution_keeps_declaring_file_scope`
/// test, to be un-ignored with the D2 implementation).
#[test]
#[ignore = "pending D2 conditional solver handoff"]
fn deferred_open_conditional_survives_scope_exit() {}

// ---------------------------------------------------------------------------
// D3 — projection-authority collapse
// ---------------------------------------------------------------------------

/// Every current call site of `TypeQueryEngine::project_member` routes
/// through `SemanticQueryApi::execute(ProjectPath { .. })`.
#[test]
#[ignore = "pending D3 projection-authority cutover"]
fn project_member_call_site_migrated_to_dispatch() {}

#[test]
#[ignore = "pending D3 projection-authority cutover"]
fn project_surface_call_site_migrated_to_dispatch() {}

#[test]
#[ignore = "pending D3 projection-authority cutover"]
fn project_keyspace_call_site_migrated_to_dispatch() {}

#[test]
#[ignore = "pending D3 projection-authority cutover"]
fn type_surface_db_deleted_or_internalised() {}

#[test]
#[ignore = "pending D3 projection-authority cutover"]
fn component_meta_projection_goes_through_dispatch_not_private_api() {}

// ---------------------------------------------------------------------------
// D4 — retire coarse open-generic symbolic stop
// ---------------------------------------------------------------------------

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn open_generic_expansion_no_longer_short_circuits_to_applied_stub() {}

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn path_projection_through_open_applied_does_not_short_circuit_to_symbolic_indexed_access() {}

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn indexed_access_open_skips_counter_retired() {}

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn budget_exceeded_returns_structured_failure_not_applied_stub() {}

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn budget_domain_solver_resolve_steps_trips_cleanly() {}

#[test]
#[ignore = "pending D4 symbolic-stop retirement"]
fn budget_domain_solver_arena_nodes_trips_cleanly() {}

// ---------------------------------------------------------------------------
// D5 — solver caches + request-scoped identity retired
// ---------------------------------------------------------------------------

#[test]
#[ignore = "pending D5 solver-cache retirement"]
fn solver_trace_summary_does_not_double_count_dispatch_metrics() {}

#[test]
#[ignore = "pending D5 solver-cache retirement"]
fn subject_key_and_op_cache_types_deleted() {}

#[test]
#[ignore = "pending D5 solver-cache retirement"]
fn projection_cache_and_active_projection_keys_deleted() {}

#[test]
#[ignore = "pending D5 solver-cache retirement"]
fn solver_caches_member_and_keyspace_deleted() {}

#[test]
#[ignore = "pending D5 solver-cache retirement"]
fn solver_caches_relation_rebuilt_per_dispatch_builder_invocation() {}
