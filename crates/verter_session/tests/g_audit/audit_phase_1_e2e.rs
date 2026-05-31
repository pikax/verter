//! Phase-1 e2e — drives every Phase-1 audit helper through a fake
//! `RequestContext` and asserts the new counters and structured event
//! variants land in the audit record / accumulator. Plan §3.7.
//!
//! This test does NOT exercise the materialiser (Phase 8); it validates
//! the audit envelope only. Each helper is invoked with a context
//! installed (so the counter increments fire) and without a context
//! (so the no-op fast path is exercised).
//!
//! Discriminating contract: the test FAILS if any new counter remains
//! at zero after invocation, AND if any new event variant fails to
//! land in the accumulator's `structured_events` log.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_session::component_meta_audit::accumulator::RequestFootprintAccumulator;
use verter_session::component_meta_audit::{
    CacheOutcomeKind, MaterializationScopeAudit, MaterializeSkipReason, ProjectionModeAudit,
    StructuredAuditEvent,
};
use verter_session::host_manage::{
    push_structured_event, record_dep_signature_intern_hit, record_dep_signature_merge,
    record_family_map_lock_acquisition, record_materialize_structure_cache_hit,
    record_materialize_structure_call, record_node_arena_lock_acquisition,
};
use verter_session::request_context::{RequestContext, RequestContextGuard};

#[test]
fn phase1_counters_no_op_without_request_context() {
    // Nothing installed → every helper is a zero-op. We can only
    // assert by absence: the call returns; nothing panics; no global
    // state we can observe is touched. The assertion is that the
    // calls compile and return — exercising the fast-path branch.
    record_materialize_structure_call();
    record_materialize_structure_cache_hit();
    record_node_arena_lock_acquisition(std::time::Duration::ZERO);
    record_family_map_lock_acquisition(std::time::Duration::ZERO);
    record_dep_signature_merge();
    record_dep_signature_intern_hit();
    // If this point is reached, the no-op path works. Nothing more
    // to assert without a context.
}

#[test]
fn phase1_counters_increment_under_request_context() {
    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(101, Arc::from("/c.vue"), true, Some(Arc::clone(&acc)));
    let _g = RequestContextGuard::install(Arc::clone(&ctx));

    record_materialize_structure_call();
    record_materialize_structure_call();
    record_materialize_structure_cache_hit();
    record_node_arena_lock_acquisition(std::time::Duration::ZERO);
    record_family_map_lock_acquisition(std::time::Duration::ZERO);
    record_dep_signature_merge();
    record_dep_signature_merge();
    record_dep_signature_merge();
    record_dep_signature_intern_hit();

    assert_eq!(
        ctx.materialize_structure_calls.load(Ordering::Relaxed),
        2,
        "materialize_structure_calls must increment per call",
    );
    assert_eq!(
        ctx.materialize_structure_cache_hits.load(Ordering::Relaxed),
        1,
    );
    assert_eq!(ctx.node_arena_lock_acquisitions.load(Ordering::Relaxed), 1);
    assert_eq!(ctx.family_map_lock_acquisitions.load(Ordering::Relaxed), 1);
    assert_eq!(ctx.dep_signature_merges.load(Ordering::Relaxed), 3);
    assert_eq!(ctx.dep_signature_intern_hits.load(Ordering::Relaxed), 1);
}

#[test]
fn phase1_new_structured_events_appended_to_accumulator() {
    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(202, Arc::from("/d.vue"), true, Some(Arc::clone(&acc)));
    let _g = RequestContextGuard::install(Arc::clone(&ctx));

    push_structured_event(StructuredAuditEvent::MaterializeStructureEnter {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::TopLevel,
        mode: ProjectionModeAudit::Expanded,
        depth: 1,
    });
    push_structured_event(StructuredAuditEvent::MaterializeStructureExit {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::TopLevel,
        mode: ProjectionModeAudit::Expanded,
        outcome: CacheOutcomeKind::Hit,
        duration_ns: 1234,
    });
    push_structured_event(StructuredAuditEvent::MaterializeStructurePolicySkip {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        reason: MaterializeSkipReason::FunctionPropertyAtNested,
    });
    push_structured_event(StructuredAuditEvent::MaterializeStructureCycleDetected {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        mode: ProjectionModeAudit::Expanded,
        depth: 3,
    });
    push_structured_event(StructuredAuditEvent::MaterializeStructureDepthFuseTripped {
        base: Arc::from("Object#7"),
        scope_axis: MaterializationScopeAudit::Nested,
        mode: ProjectionModeAudit::Expanded,
        depth: 4096,
    });

    let state = acc.drain();
    assert_eq!(state.structured_events.len(), 5);

    // Discriminating: every new variant must be present (not just any 5).
    let mut saw_enter = false;
    let mut saw_exit = false;
    let mut saw_skip = false;
    let mut saw_cycle = false;
    let mut saw_fuse = false;
    for ev in &state.structured_events {
        match ev {
            StructuredAuditEvent::MaterializeStructureEnter { .. } => saw_enter = true,
            StructuredAuditEvent::MaterializeStructureExit { .. } => saw_exit = true,
            StructuredAuditEvent::MaterializeStructurePolicySkip { .. } => saw_skip = true,
            StructuredAuditEvent::MaterializeStructureCycleDetected { .. } => saw_cycle = true,
            StructuredAuditEvent::MaterializeStructureDepthFuseTripped { .. } => saw_fuse = true,
            _ => {}
        }
    }
    assert!(saw_enter && saw_exit && saw_skip && saw_cycle && saw_fuse,
        "every new structured-event variant must reach the accumulator (saw enter={saw_enter} exit={saw_exit} skip={saw_skip} cycle={saw_cycle} fuse={saw_fuse})",
    );
}

#[test]
fn phase1_cache_outcome_kind_tainted_serializes_round_trip() {
    let value = CacheOutcomeKind::Tainted;
    let json = serde_json::to_string(&value).expect("serialize");
    assert_eq!(json, "\"Tainted\"");
    let back: CacheOutcomeKind = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(back, CacheOutcomeKind::Tainted));
}

#[test]
fn phase1_pub_mirror_enums_have_expected_variants() {
    // Plan §3.4 — `MaterializationScopeAudit` and
    // `ProjectionModeAudit` are PUB (not pub(crate)) so this
    // integration test can construct them.
    let scopes = [
        MaterializationScopeAudit::TopLevel,
        MaterializationScopeAudit::Nested,
    ];
    let modes = [
        ProjectionModeAudit::Identity,
        ProjectionModeAudit::Navigate,
        ProjectionModeAudit::Shallow,
        ProjectionModeAudit::Expanded,
    ];
    assert_eq!(scopes.len(), 2);
    assert_eq!(modes.len(), 4);
    // Exhaustive serde round-trip.
    for s in scopes {
        let j = serde_json::to_string(&s).unwrap();
        let _: MaterializationScopeAudit = serde_json::from_str(&j).unwrap();
    }
    for m in modes {
        let j = serde_json::to_string(&m).unwrap();
        let _: ProjectionModeAudit = serde_json::from_str(&j).unwrap();
    }
}

#[test]
fn phase1_materialize_skip_reason_covers_all_arms() {
    let arms = [
        MaterializeSkipReason::FunctionPropertyAtNested,
        MaterializeSkipReason::GenericRefWithArgsTopLevel,
        MaterializeSkipReason::PackageRefTopLevel,
        MaterializeSkipReason::RegistryRouteNotInlineMaterialisable,
        MaterializeSkipReason::NonStructuralTopLevel,
    ];
    assert_eq!(arms.len(), 5);
    for r in arms {
        let j = serde_json::to_string(&r).unwrap();
        let _: MaterializeSkipReason = serde_json::from_str(&j).unwrap();
    }
}
