use super::*;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(target_arch = "wasm32")]
use web_time::Duration;

fn canonical(s: &str) -> Arc<str> {
    Arc::from(s)
}

fn file_stage(s: &str, gen: u64, stage: FileStageKey) -> WorkNodeIdentity {
    WorkNodeIdentity::FileStage {
        canonical: canonical(s),
        generation: gen,
        stage,
    }
}

fn artifact(s: &str, gen: u64, profile: u64) -> WorkNodeIdentity {
    WorkNodeIdentity::Artifact {
        canonical: canonical(s),
        generation: gen,
        profile_hash: profile_hash_to_bytes(profile),
        content_hash: [0u8; 16],
    }
}

fn cache_node(cache: u64, key: u64, epoch: u64, pin: u64) -> WorkNodeIdentity {
    let mut kh = [0u8; 16];
    kh[..8].copy_from_slice(&key.to_le_bytes());
    WorkNodeIdentity::CacheNode {
        cache_id: SchedulerCacheId(cache),
        key_hash: kh,
        view_epoch: epoch,
        snapshot_pin_id: PinId(pin),
    }
}

fn fast_aging() -> DagAgingConfig {
    DagAgingConfig {
        background_to_interactive: Duration::from_millis(50),
        maintenance_to_background: Duration::from_millis(50),
    }
}

/// Discriminating fact: WorkNodeIdentity has exactly three
/// variants. Adding a fourth would fail compilation here
/// because the exhaustive match below must cover it.
#[test]
fn work_node_identity_has_exactly_three_variants() {
    let id_file = file_stage("/a.vue", 1, FileStageKey::Source);
    let id_art = artifact("/a.vue", 1, 42);
    let id_cache = cache_node(7, 99, 1, 1);

    for id in [id_file, id_art, id_cache] {
        match id {
            WorkNodeIdentity::FileStage { .. } => {}
            WorkNodeIdentity::Artifact { .. } => {}
            WorkNodeIdentity::CacheNode { .. } => {}
        }
    }
}

/// Discriminating fact: WorkKind has exactly five variants.
/// A sixth would force this match arm to grow.
#[test]
fn work_kind_has_exactly_five_variants() {
    for k in [
        WorkKind::Load,
        WorkKind::Parse,
        WorkKind::Analysis,
        WorkKind::Artifact,
        WorkKind::CacheNode,
    ] {
        match k {
            WorkKind::Load => {}
            WorkKind::Parse => {}
            WorkKind::Analysis => {}
            WorkKind::Artifact => {}
            WorkKind::CacheNode => {}
        }
    }
}

/// Highest-priority ready node dispatches first.
#[test]
fn next_ready_returns_highest_priority_first() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let t_low = dag.submit(
        file_stage("/low.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Background,
        Vec::new(),
        None,
    );
    let t_hi = dag.submit(
        file_stage("/hi.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Critical,
        Vec::new(),
        None,
    );
    let t_mid = dag.submit(
        file_stage("/mid.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    // Pre-change behaviour (no DAG existing): `next_ready` would
    // return None — every assertion below would fail.
    let r1 = dag.next_ready().expect("hi expected");
    let r2 = dag.next_ready().expect("mid expected");
    let r3 = dag.next_ready().expect("low expected");
    assert_eq!(r1.token, t_hi);
    assert_eq!(r2.token, t_mid);
    assert_eq!(r3.token, t_low);
}

/// Identical identity merges into the same token with priority
/// taking the min (higher priority).
#[test]
fn submit_dedup_merges_identity_and_upgrades_priority() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let t1 = dag.submit(
        file_stage("/a.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Background,
        Vec::new(),
        None,
    );
    let t2 = dag.submit(
        file_stage("/a.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Critical,
        Vec::new(),
        None,
    );
    assert_eq!(t1, t2, "dedup must return the same token");
    let r = dag.next_ready().expect("ready");
    assert_eq!(r.token, t1);
    assert_eq!(r.effective_priority, Priority::Critical);
    assert!(
        dag.next_ready().is_none(),
        "only one node should have been dispatched after dedup",
    );
}

/// Newer generation supersedes older — the cancel path drops the
/// older node so it never dispatches.
#[test]
fn cancel_older_generation_drops_stale_node() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let _old = dag.submit(
        file_stage("/a.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let new = dag.submit(
        file_stage("/a.vue", 2, FileStageKey::Source),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    // Higher layer must explicitly cancel the older identity.
    dag.cancel(&file_stage("/a.vue", 1, FileStageKey::Source));
    let r = dag.next_ready().expect("ready");
    assert_eq!(r.token, new);
    assert!(dag.next_ready().is_none());
}

/// Dependency gating: a node with unresolved deps does not appear
/// in `next_ready`. After `complete` on the gating identity, the
/// waiter becomes dispatchable.
#[test]
fn dependency_gating_holds_back_node_until_dep_completes() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let dep_id = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep_tok = dag.submit(
        dep_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let waiter = dag.submit(
        artifact("/a.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![DepKey::from_identity(&dep_id)],
        None,
    );

    // First ready is the dep (the waiter is blocked).
    let r1 = dag.next_ready().expect("dep ready");
    assert_eq!(r1.identity, dep_id);
    assert!(
        dag.next_ready().is_none(),
        "waiter must not be ready while dep is unresolved",
    );

    // Resolve the dep — waiter becomes ready.
    let newly_ready = dag.complete(&dep_id);
    assert_eq!(newly_ready, vec![waiter]);
    let r2 = dag.next_ready().expect("waiter ready");
    assert_eq!(r2.token, waiter);
}

/// Dynamic-import barrier: a cache-node-typed dep gates an artifact
/// across kinds, proving the variant disjointness in practice.
#[test]
fn dynamic_import_barrier_uses_cache_node_dep_to_gate_artifact() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let barrier = cache_node(1, 42, 7, 99);
    let _bt = dag.submit(
        barrier.clone(),
        WorkKind::CacheNode,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let consumer = dag.submit(
        artifact("/a.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![DepKey::from_identity(&barrier)],
        None,
    );
    let first = dag.next_ready().expect("barrier first");
    assert_eq!(first.identity, barrier);
    assert!(dag.next_ready().is_none());
    let newly = dag.complete(&barrier);
    assert_eq!(newly, vec![consumer]);
}

/// Cancellation releases capacity exactly once via Drop.
#[test]
fn cancellation_releases_capacity_permits_exactly_once_on_drop() {
    let dag = SchedulerDag::new(DagAgingConfig::default());
    assert_eq!(dag.in_flight_permits(), 0);
    {
        let _r = dag.reserve_capacity(3);
        assert_eq!(dag.in_flight_permits(), 3);
    }
    assert_eq!(dag.in_flight_permits(), 0);
}

/// Explicit `release` also returns permits exactly once; the
/// double-release scenario is statically impossible because
/// `release(self)` consumes the reservation.
#[test]
fn capacity_release_returns_permits_and_makes_double_release_unrepresentable() {
    let dag = SchedulerDag::new(DagAgingConfig::default());
    let r = dag.reserve_capacity(2);
    assert_eq!(dag.in_flight_permits(), 2);
    r.release();
    assert_eq!(dag.in_flight_permits(), 0);
    // r is moved — calling release again would not compile.
    // A second reservation must be a fresh one:
    let r2 = dag.reserve_capacity(5);
    assert_eq!(dag.in_flight_permits(), 5);
    drop(r2);
    assert_eq!(dag.in_flight_permits(), 0);
}

/// Aging promotes Background → Interactive after the configured
/// threshold.
#[test]
fn aging_promotes_background_to_interactive_after_threshold() {
    let mut dag = SchedulerDag::new(fast_aging());
    let _t = dag.submit(
        file_stage("/old.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Background,
        Vec::new(),
        None,
    );
    // Wait past the aging threshold.
    std::thread::sleep(Duration::from_millis(80));
    let r = dag.next_ready().expect("ready after aging");
    assert_eq!(r.effective_priority, Priority::Interactive);
}

/// Cancel returns stranded waiters whose only remaining dep was
/// the cancelled node, so the driver can fail them.
#[test]
fn cancel_returns_stranded_waiters_for_failure_propagation() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let dep_id = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep = dag.submit(
        dep_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let waiter = dag.submit(
        artifact("/a.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![DepKey::from_identity(&dep_id)],
        None,
    );
    let stranded = dag.cancel(&dep_id);
    assert_eq!(stranded, vec![waiter], "stranded waiter must be reported");
}

/// Pre-dispatch dedup merges incoming deps into the existing node's
/// `deps_remaining`.
///
/// Without dep merging on dedup the second `submit` would ignore
/// its deps, so the merged node could dispatch before C completed.
/// With dep merging, `deps_remaining = {A, B, C}` — C gates the
/// same dispatch.
#[test]
fn submit_dedup_merges_incoming_deps_into_existing_node() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let dep_a = file_stage("/a.ts", 1, FileStageKey::Analysis);
    let dep_b = file_stage("/b.ts", 1, FileStageKey::Analysis);
    let dep_c = file_stage("/c.ts", 1, FileStageKey::Analysis);
    let target = file_stage("/target.vue", 1, FileStageKey::Analysis);

    // Admit deps so the gating identities exist.
    for d in [&dep_a, &dep_b, &dep_c] {
        dag.submit(
            d.clone(),
            WorkKind::Analysis,
            Priority::Background,
            Vec::new(),
            None,
        );
    }

    // First submit with {A, B}.
    let t1 = dag.submit(
        target.clone(),
        WorkKind::Analysis,
        Priority::Background,
        vec![DepKey::from_identity(&dep_a), DepKey::from_identity(&dep_b)],
        None,
    );
    // Second submit with {C} — before A or B completes.
    let t2 = dag.submit(
        target.clone(),
        WorkKind::Analysis,
        Priority::Background,
        vec![DepKey::from_identity(&dep_c)],
        None,
    );
    assert_eq!(t1, t2, "dedup must collapse onto the same token");

    // Complete A and B — target must still NOT be ready because
    // C remains as a merged gating dep.
    dag.complete(&dep_a);
    dag.complete(&dep_b);
    assert!(
        dag.has_pending_deps(&target),
        "merged dep C must still gate the target — without dep-merging this assert would fail",
    );

    // Complete C — now the target should be free of deps.
    dag.complete(&dep_c);
    assert!(
        !dag.has_pending_deps(&target),
        "all merged deps satisfied — target is now dispatchable",
    );
}

/// Re-submitting a dispatched identity with empty deps is a
/// legitimate in-flight dedup — a second caller is joining the
/// in-flight work without adding new gating. Without the joiner
/// allowance, the dispatched arm of `submit` panicked via
/// `debug_assert!(false)`. With the allowance, the same call is a
/// no-op on `deps_remaining` and returns the existing token so the
/// joiner shares the in-flight result.
#[test]
fn in_flight_dedup_no_panic_no_deps_change() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let id = file_stage("/target.vue", 1, FileStageKey::Source);

    // Admit and dispatch target first.
    let t1 = dag.submit(
        id.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let ready = dag.next_ready().expect("ready");
    assert_eq!(ready.token, t1, "target dispatched first");

    // id is now dispatched. A second submit with empty deps is a
    // legitimate joiner. Without the joiner allowance this would
    // `debug_assert!(false)`.
    let t2 = dag.submit(
        id.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    assert_eq!(t1, t2, "dedup must return the in-flight token");

    // The in-flight node's deps_remaining is unchanged (empty).
    let deps = dag
        .deps_remaining_for_test(t1)
        .expect("dispatched node still present");
    assert!(
        deps.is_empty(),
        "empty-deps re-submit must not modify deps_remaining",
    );
}

/// Re-submitting an already-dispatched identity with NEW incoming
/// deps must NOT mutate the dispatched node's `deps_remaining`. A
/// dispatched node's prerequisite set is closed: the worker is
/// already running under the dep set fixed at dispatch time, so
/// appending to that set after the fact would let the result publish
/// under a prerequisite the work never observed. Priority upgrades
/// continue to apply.
#[test]
fn dispatched_dedup_with_deps_does_not_mutate_incoming_edges() {
    use crate::request_context::OpaqueRequestContext;
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let target_id = file_stage("/target.vue", 1, FileStageKey::Source);

    // Admit target with an initial dep A AND a real request_context
    // tagged by request_id=42. The dedup join on the dispatched
    // path must NOT overwrite this context with the joiner's None.
    let dep_a = file_stage("/a.ts", 1, FileStageKey::Analysis);
    let _t_a = dag.submit(
        dep_a.clone(),
        WorkKind::Analysis,
        Priority::Background,
        Vec::new(),
        None,
    );
    let first_ctx = OpaqueRequestContext::test_only(42);
    let t1 = dag.submit(
        target_id.clone(),
        WorkKind::Load,
        Priority::Background,
        vec![DepKey::from_identity(&dep_a)],
        Some(first_ctx.clone()),
    );
    // Dispatch A first, then complete it so target's gate clears.
    let ready_a = dag.next_ready().expect("a ready");
    assert_eq!(ready_a.token, _t_a, "a dispatched first");
    let _ = dag.complete(&dep_a);
    // Now target is ready; dispatch it.
    let ready_t = dag.next_ready().expect("target ready");
    assert_eq!(ready_t.token, t1, "target dispatched after a clears");

    // Sanity: dispatched target has no remaining deps.
    let pre_deps = dag
        .deps_remaining_for_test(t1)
        .expect("dispatched target present");
    assert!(
        pre_deps.is_empty(),
        "pre-condition: dispatched target's deps_remaining is empty",
    );

    // Admit a NEW blocker B and re-submit target with deps={B} +
    // higher priority + None context (the joiner's view). The
    // closed-prereq invariant says deps are ignored on the
    // dispatched dedup branch, the priority upgrade still applies,
    // AND the first-arrived submitter's request_context survives
    // (the joiner's None must not clobber it).
    let dep_b = file_stage("/b.ts", 1, FileStageKey::Analysis);
    let _t_b = dag.submit(
        dep_b.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let t2 = dag.submit(
        target_id.clone(),
        WorkKind::Load,
        Priority::Critical,
        vec![DepKey::from_identity(&dep_b)],
        None,
    );
    assert_eq!(t1, t2, "dedup must return the in-flight token");

    // Closed prereq invariant: dispatched node's incoming edges are
    // immutable, so the new dep B was NOT appended. Pre-strip code
    // appended B to `deps_remaining`; post-strip it stays empty.
    let target_tok = dag.token_for(&target_id).expect("target still in DAG");
    let post_deps = dag
        .deps_remaining_for_test(target_tok)
        .expect("target node deps_remaining");
    assert!(
        post_deps.is_empty(),
        "dispatched dedup must NOT mutate deps_remaining: \
         a dispatched node's incoming edges are immutable",
    );
    assert!(
        !post_deps.contains(&DepKey::from_identity(&dep_b)),
        "late dep B must NOT be present in deps_remaining after \
         re-submit on a dispatched identity",
    );

    // Priority upgrade survives the dispatched dedup branch.
    let prio = dag
        .base_priority_for_test(target_tok)
        .expect("target priority");
    assert_eq!(
        prio,
        Priority::Critical,
        "priority upgrade must still apply on dispatched dedup",
    );

    // Winner-context survival on the dispatched dedup branch. The
    // first-arrived submitter's context (request_id=42) is the
    // "winner" — the joiner's None re-submit must NOT overwrite it.
    // A `Some(None)` return would mean the node still exists but
    // the context was cleared; `None` would mean the node is gone.
    // Neither indicates the winner's context survived — only
    // `Some(42)` proves it.
    let ctx_id = dag
        .request_context_id_for_test(target_tok)
        .expect("target node present");
    assert_eq!(
        ctx_id, 42,
        "dispatched dedup must preserve the first-arrived \
         submitter's request_context — without the preserve guard \
         the joiner's None could have overwritten it",
    );

    // The reverse-index must not have grown a stale entry for the
    // dispatched target under dep B's key. Complete B and assert the
    // dispatched target is NOT among the newly-ready tokens (it was
    // never waiting on B).
    let newly_ready = dag.complete(&dep_b);
    assert!(
        !newly_ready.contains(&target_tok),
        "dispatched target must not appear in B's waiter fan-out: \
         dispatched-dedup must not add the target token to B's waiters list",
    );
}

/// Priority upgrade on a re-submit must apply even when the node is
/// already dispatched (in-flight dedup). Without the priority upgrade
/// arm the dispatched submit returned early via debug_assert; in
/// release the upgrade was silently dropped.
#[test]
fn priority_upgrade_survives_in_flight_dedup() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let id = file_stage("/target.vue", 1, FileStageKey::Source);
    let t1 = dag.submit(
        id.clone(),
        WorkKind::Load,
        Priority::Background,
        Vec::new(),
        None,
    );
    let r = dag.next_ready().expect("ready");
    assert_eq!(r.token, t1);
    // id is now dispatched. Re-submit with Critical — without the
    // priority-upgrade arm this would panic in debug or drop the
    // upgrade in release.
    let t2 = dag.submit(
        id.clone(),
        WorkKind::Load,
        Priority::Critical,
        Vec::new(),
        None,
    );
    assert_eq!(t1, t2, "in-flight dedup returns the same token");
    let target_tok = dag.token_for(&id).expect("target");
    let prio = dag
        .base_priority_for_test(target_tok)
        .expect("target node priority");
    assert_eq!(
        prio,
        Priority::Critical,
        "priority upgrade must take effect even on the dispatched node",
    );
}

/// Supersede also cancels stale-generation DAG nodes, not just
/// waiter groups. Without this, a stale Source node could still
/// dispatch after the canonical bumped to a higher generation.
#[test]
fn supersede_old_file_generations_cancels_stale_dag_nodes() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let stale = file_stage("/a.vue", 1, FileStageKey::Source);
    let _stale_tok = dag.submit(
        stale.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    assert!(dag.token_for(&stale).is_some(), "stale node pre-supersede");

    // Bump to gen=2 — supersede must cancel the gen=1 node.
    let canonical: Arc<str> = Arc::from("/a.vue");
    dag.supersede_old_file_generations(&canonical, 2);

    assert!(
        dag.token_for(&stale).is_none(),
        "stale gen=1 node must be cancelled and dropped from by_identity",
    );
    // It must also be gone from `nodes` (cancel removes the
    // entry after releasing waiters).
    assert_eq!(
        dag.total_active(),
        0,
        "no active nodes remain after stale-generation cancel",
    );
    // Without the DAG-node cancel arm, the stale node would still
    // be dispatchable because supersede only touched waiter groups;
    // with the cancel arm, nothing remains to dispatch.
    assert!(
        dag.next_ready().is_none(),
        "stale node must not dispatch after supersede",
    );
}

/// next_ready defers a CPU-bound job when the cpu class budget is
/// saturated; completing a dispatched CPU job returns the permit
/// and lets the next deferred job dispatch. This wires the per-class
/// hybrid budget through the DAG's next_ready path.
#[test]
fn next_ready_defers_when_cpu_class_saturated_and_resumes_on_complete() {
    let budget = DagCapacityBudget { cpu: 1, io: 1 };
    let mut dag = SchedulerDag::with_budget(DagAgingConfig::default(), budget);

    // Submit two CPU-bound jobs at the same priority.
    let id_a = file_stage("/a.ts", 1, FileStageKey::Analysis);
    let id_b = file_stage("/b.ts", 1, FileStageKey::Analysis);
    let _t_a = dag.submit(
        id_a.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let _t_b = dag.submit(
        id_b.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );

    // First dispatch consumes the single cpu permit.
    let first = dag.next_ready().expect("first cpu job");
    assert_eq!(dag.in_flight_cpu_permits(), 1);

    // Second dispatch is deferred — cpu budget full.
    // Without reservation gating, both jobs would dispatch
    // back-to-back and the assertion below would fail.
    assert!(
        dag.next_ready().is_none(),
        "cpu_work saturated — next_ready must defer the second job",
    );

    // Completing the first job releases the permit; the second
    // job dispatches.
    let _newly = dag.complete(&first.identity);
    assert_eq!(
        dag.in_flight_cpu_permits(),
        0,
        "permit returned by complete()",
    );
    let second = dag.next_ready().expect("second cpu job after release");
    assert_ne!(second.identity, first.identity);
}

/// CPU and IO budgets are independent: a saturated CPU pool does
/// NOT prevent an IO-bound job from dispatching.
#[test]
fn next_ready_admits_io_when_cpu_full_and_vice_versa() {
    let budget = DagCapacityBudget { cpu: 1, io: 1 };
    let mut dag = SchedulerDag::with_budget(DagAgingConfig::default(), budget);

    let cpu_id = file_stage("/cpu.ts", 1, FileStageKey::Analysis);
    let io_id = file_stage("/io.vue", 1, FileStageKey::Source);
    let _ = dag.submit(
        cpu_id,
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let _ = dag.submit(
        io_id,
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );

    // Both classes have 1 permit — both jobs must dispatch.
    let _r1 = dag.next_ready().expect("first ready");
    let _r2 = dag.next_ready().expect("second ready");
    assert_eq!(dag.in_flight_cpu_permits(), 1);
    assert_eq!(dag.in_flight_io_permits(), 1);
}

/// `submit_count` discrimination: a fresh dedup-merge does NOT
/// produce a second token — `total_active` stays at 1 after two
/// `submit` calls with the same identity.
#[test]
fn submit_count_stays_one_under_dedup() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    dag.submit(
        file_stage("/a.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Background,
        Vec::new(),
        None,
    );
    dag.submit(
        file_stage("/a.vue", 1, FileStageKey::Source),
        WorkKind::Load,
        Priority::Critical,
        Vec::new(),
        None,
    );
    assert_eq!(
        dag.total_active(),
        1,
        "dedup must collapse two `submit` calls into one node",
    );
}

// ──────────────────────────────────────────────────────────────
// Direct unit tests for the macro-cycle filter primitives and
// the cooperative pump's caller-aware `next_ready_for_pump`.
// ──────────────────────────────────────────────────────────────

/// `has_dep_on` true case: a node whose `deps_remaining` contains
/// the target `DepKey` reports true. The adjacency check is the
/// direct-mutual-cycle path of the unified macro-cycle filter;
/// the bounded BFS on top handles transitive cycles. The
/// adjacency case must still hold.
#[test]
fn has_dep_on_returns_true_when_dep_is_in_deps_remaining() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let dep_id = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let owner_id = artifact("/a.vue", 1, 7);
    let dep_key = DepKey::from_identity(&dep_id);
    dag.submit(
        dep_id,
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    dag.submit(
        owner_id.clone(),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![dep_key.clone()],
        None,
    );
    assert!(
        dag.has_dep_on(&owner_id, &dep_key),
        "owner's deps_remaining must contain dep",
    );
}

/// `has_dep_on` false case: after the dep completes, the owner's
/// `deps_remaining` no longer contains it.
#[test]
fn has_dep_on_returns_false_when_dep_completed() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let dep_id = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let owner_id = artifact("/a.vue", 1, 7);
    let dep_key = DepKey::from_identity(&dep_id);
    dag.submit(
        dep_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    dag.submit(
        owner_id.clone(),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![dep_key.clone()],
        None,
    );
    // Complete the dep — the owner's deps_remaining should be
    // emptied.
    dag.complete(&dep_id);
    assert!(
        !dag.has_dep_on(&owner_id, &dep_key),
        "completed dep must NOT remain in owner's deps_remaining",
    );
}

/// `dep_reaches_owner` transitive BFS: A→B→C→A. The walk
/// starts at A's dep (B), follows B's Analysis-stage dep edges
/// to C, follows C's edges to A, and reports true.
#[test]
fn dep_reaches_owner_returns_true_for_three_node_cycle() {
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let a = canonical("/a.vue");
    let b = canonical("/b.vue");
    let c = canonical("/c.vue");

    let b_id = WorkNodeIdentity::FileStage {
        canonical: Arc::clone(&b),
        generation: 1,
        stage: FileStageKey::Analysis,
    };
    let c_id = WorkNodeIdentity::FileStage {
        canonical: Arc::clone(&c),
        generation: 1,
        stage: FileStageKey::Analysis,
    };
    let c_dep = DepKey::FileStage {
        canonical: Arc::clone(&c),
        generation: 1,
        stage: FileStageKey::Analysis,
    };
    let a_dep = DepKey::FileStage {
        canonical: Arc::clone(&a),
        generation: 1,
        stage: FileStageKey::Analysis,
    };
    // B gates on C; C gates on A. The closing edge from A to B
    // would close the cycle.
    dag.submit(
        b_id,
        WorkKind::Analysis,
        Priority::Background,
        vec![c_dep],
        None,
    );
    dag.submit(
        c_id,
        WorkKind::Analysis,
        Priority::Background,
        vec![a_dep],
        None,
    );
    assert!(
        dag.dep_reaches_owner(&a, 1, &b, 1),
        "bounded BFS must report A→B→C→A as a cycle",
    );
}

/// `dep_reaches_owner` must detect a cycle even when the chain
/// length exceeds any plausible fixed-hop cap. The visited-set
/// bound terminates the walk only when no unvisited reachable
/// nodes remain.
///
/// Setup: A → B0 → B1 → ... → B299 → A (300-node linear chain
/// closing on A). A fixed-hop cap of 256 would return false at
/// hop 256, missing the cycle and admitting a mutually-blocking
/// dep edge that would deadlock at runtime. The visited-set
/// bound walks all 300 hops and reports the cycle.
#[test]
fn dep_reaches_owner_detects_cycle_past_256_hops() {
    const CHAIN_LEN: usize = 300;
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let a = canonical("/a.vue");
    let chain: Vec<Arc<str>> = (0..CHAIN_LEN)
        .map(|i| canonical(&format!("/b{i}.vue")))
        .collect();

    // Each Bi gates on Bi+1; B(CHAIN_LEN-1) gates on A. Walking
    // from B0 reaches A through CHAIN_LEN hops.
    for (i, current) in chain.iter().enumerate() {
        let current_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(current),
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let next_canonical = if i + 1 < CHAIN_LEN {
            Arc::clone(&chain[i + 1])
        } else {
            Arc::clone(&a)
        };
        let next_dep = DepKey::FileStage {
            canonical: next_canonical,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        dag.submit(
            current_id,
            WorkKind::Analysis,
            Priority::Background,
            vec![next_dep],
            None,
        );
    }

    assert!(
        dag.dep_reaches_owner(&a, 1, &chain[0], 1),
        "visited-set BFS must detect cycle through {CHAIN_LEN}-node chain (a fixed 256-hop cap would miss this and admit a deadlocking dep)",
    );
}

/// `dep_reaches_owner` BFS must visit each reachable node exactly
/// once and keep the frontier bounded by O(V), independent of edge
/// count. The enqueue-time visited rule is the invariant: every
/// `frontier.push_back` is gated by `visited.insert(..)` returning
/// `true`, so the same node never enters the frontier twice.
///
/// Setup: a complete bipartite-like layered graph with
/// `LAYERS` × `WIDTH` Analysis-stage nodes. Each node in layer L
/// has `WIDTH` dep edges into every node in layer L+1.
///
/// The BFS starts at `layers[0][0]` (one node in layer 0) and
/// fans out to every node in layers 1..LAYERS, so the reachable
/// set is `1 + (LAYERS - 1) * WIDTH = 81` nodes (layer-0 siblings
/// of the start are unreachable — edges flow forward only).
/// Total edges in the reachable subgraph =
/// `1 * WIDTH + (LAYERS - 2) * WIDTH * WIDTH = 16 + 4*256 = 1040`.
///
/// Under enqueue-time visited:
///   enqueue_count == reachable_count       (81)
///   max_frontier_len <= reachable_count    (16 in steady state)
///
/// Under pop-time visited (the regression class):
///   enqueue_count grows toward O(E) = 1040+
///   max_frontier_len grows toward O(WIDTH * WIDTH) = 256 per layer
///
/// Discriminator: the test reads the BFS metrics directly via the
/// instrumented variant and asserts the O(V) bound. The wall-clock
/// fallback (2s) catches any timeout pathology, but the equality
/// and inequality on the metric counters are the load-bearing
/// assertions — they discriminate enqueue-time visited from
/// pop-time visited without relying on timing.
#[test]
fn dep_reaches_owner_frontier_bounded_on_dense_graph() {
    const LAYERS: usize = 6;
    const WIDTH: usize = 16;
    // BFS seeds `layers[0][0]` only — layer-0 siblings are not
    // reachable through forward edges. So the reachable set is
    // the start (1) plus every node in layers 1..LAYERS.
    const REACHABLE: usize = 1 + (LAYERS - 1) * WIDTH;
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let owner = canonical("/owner.vue");

    // Build a layered DAG with no cycle. Each node in layer L
    // depends on every node in layer L+1.
    let mut layers: Vec<Vec<Arc<str>>> = Vec::with_capacity(LAYERS);
    for li in 0..LAYERS {
        let mut row = Vec::with_capacity(WIDTH);
        for wi in 0..WIDTH {
            row.push(canonical(&format!("/layer{li}_node{wi}.vue")));
        }
        layers.push(row);
    }

    for li in 0..LAYERS {
        let next_layer = if li + 1 < LAYERS {
            Some(&layers[li + 1])
        } else {
            None
        };
        for node_canonical in layers[li].iter() {
            let node_id = WorkNodeIdentity::FileStage {
                canonical: Arc::clone(node_canonical),
                generation: 1,
                stage: FileStageKey::Analysis,
            };
            let deps: Vec<DepKey> = if let Some(next) = next_layer {
                next.iter()
                    .map(|c| DepKey::FileStage {
                        canonical: Arc::clone(c),
                        generation: 1,
                        stage: FileStageKey::Analysis,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            dag.submit(
                node_id,
                WorkKind::Analysis,
                Priority::Background,
                deps,
                None,
            );
        }
    }

    // Owner is not part of the graph — no cycle. Probe starts at
    // layer 0's first node; the BFS must exhaust the reachable
    // subgraph and return false.
    let start = std::time::Instant::now();
    let (reachable, metrics) = dag.dep_reaches_owner_with_metrics(&owner, 1, &layers[0][0], 1);
    let elapsed = start.elapsed();
    assert!(!reachable, "no cycle exists; BFS should return false");

    // Primary discriminating assertions on the BFS metrics.
    //
    // enqueue_count: every reachable node entered the frontier
    // exactly once. Pop-time visited (the regression class) would
    // push the layer-L+1 nodes `WIDTH` times each before the first
    // pop dedup, so enqueue_count would be approximately
    // `WIDTH + (LAYERS - 2) * WIDTH * WIDTH = 1040` — well above
    // `REACHABLE = 81`.
    assert_eq!(
        metrics.enqueue_count, REACHABLE,
        "enqueue-time visited rule violated: each reachable node MUST be \
         enqueued exactly once. observed enqueue_count = {} (expected {}). \
         pop-time visited inflates this toward O(E) on dense graphs.",
        metrics.enqueue_count, REACHABLE,
    );

    // max_frontier_len: bounded by REACHABLE (in fact much smaller
    // — at steady state the frontier holds one layer = WIDTH = 16
    // nodes). Pop-time visited would let the layer L+1 frontier
    // hold up to `WIDTH * WIDTH = 256` duplicate entries before
    // any pop dedup could fire, exceeding REACHABLE.
    assert!(
        metrics.max_frontier_len <= REACHABLE,
        "frontier-length bound violated: max_frontier_len = {} > REACHABLE = {}. \
         enqueue-time visited keeps the frontier at O(V); pop-time visited \
         lets it grow toward O(edges).",
        metrics.max_frontier_len,
        REACHABLE,
    );

    // Fallback wall-clock bound: well above any O(V) walk but far
    // below the O(E) blow-up the pop-time variant produces.
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "dense-graph BFS must complete in bounded O(V) time; elapsed = {elapsed:?}",
    );
}

/// `next_ready_for_pump` active-path filter: a node whose identity
/// matches an entry in the caller's active path is NOT returned
/// (the calling worker is itself executing that work).
#[test]
fn next_ready_for_pump_skips_active_path_identities() {
    use crate::caller_kind::CallerKind;
    let mut dag = SchedulerDag::new(DagAgingConfig::default());
    let id = file_stage("/a.vue", 1, FileStageKey::Analysis);
    dag.submit(
        id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let active_path = [id.clone()];
    let ready = dag.next_ready_for_pump(CallerKind::CpuWorker, &active_path);
    assert!(
        ready.is_none(),
        "active-path identity must be filtered out: got {ready:?}",
    );
    // Without the filter the same call returns the identity.
    let ready_unfiltered = dag.next_ready_for_pump(CallerKind::CpuWorker, &[]);
    let ready_unfiltered = ready_unfiltered.expect("unfiltered call must return the candidate");
    assert_eq!(ready_unfiltered.identity, id);
}

/// `next_ready_for_pump` resource-class preference (CPU caller).
/// With one CPU and one I/O candidate both ready, a `CpuWorker`
/// caller MUST receive the CPU candidate first so an inline-
/// execute path can run on the same thread.
#[test]
fn next_ready_for_pump_prefers_cpu_class_for_cpu_worker_caller() {
    use crate::caller_kind::CallerKind;
    let mut dag = SchedulerDag::with_budget(
        DagAgingConfig {
            background_to_interactive: Duration::from_secs(60),
            maintenance_to_background: Duration::from_secs(60),
        },
        DagCapacityBudget { cpu: 1, io: 1 },
    );

    // CPU candidate (Analysis).
    let cpu_id = file_stage("/a.vue", 1, FileStageKey::Analysis);
    dag.submit(
        cpu_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    // I/O candidate (Source).
    let io_id = file_stage("/b.vue", 1, FileStageKey::Source);
    dag.submit(
        io_id.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );

    // CpuWorker caller prefers CPU class.
    let first = dag
        .next_ready_for_pump(CallerKind::CpuWorker, &[])
        .expect("first ready");
    assert_eq!(
        first.identity, cpu_id,
        "CpuWorker caller must receive CPU candidate first, got {:?}",
        first.identity,
    );
}

/// `next_ready_for_pump` resource-class preference (I/O caller).
/// Symmetric to the CPU case: with one CPU and one I/O candidate
/// both ready, an `IoWorker` caller MUST receive the I/O
/// candidate first.
#[test]
fn next_ready_for_pump_prefers_io_class_for_io_worker_caller() {
    use crate::caller_kind::CallerKind;
    let mut dag = SchedulerDag::with_budget(
        DagAgingConfig {
            background_to_interactive: Duration::from_secs(60),
            maintenance_to_background: Duration::from_secs(60),
        },
        DagCapacityBudget { cpu: 1, io: 1 },
    );

    let cpu_id = file_stage("/a.vue", 1, FileStageKey::Analysis);
    dag.submit(
        cpu_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let io_id = file_stage("/b.vue", 1, FileStageKey::Source);
    dag.submit(
        io_id.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );

    let first = dag
        .next_ready_for_pump(CallerKind::IoWorker, &[])
        .expect("first ready");
    assert_eq!(
        first.identity, io_id,
        "IoWorker caller must receive I/O candidate first, got {:?}",
        first.identity,
    );
}

/// I/O capacity loan bumps the I/O counter past the configured
/// cap for the duration of the inline execute, and releases on
/// Drop. Discriminator: an I/O loan must succeed when the budget
/// is exhausted (symmetric with the CPU-loan path).
#[test]
fn loan_capacity_for_class_io_bumps_past_cap_and_releases_on_drop() {
    let dag = SchedulerDag::with_budget(
        DagAgingConfig::default(),
        DagCapacityBudget { cpu: 4, io: 1 },
    );
    // First reservation consumes the only I/O slot.
    let _r1 = dag
        .try_reserve_for_class(ResourceClass::Io)
        .expect("first I/O reservation must succeed");
    assert!(
        dag.try_reserve_for_class(ResourceClass::Io).is_none(),
        "second I/O reservation must fail (budget exhausted)",
    );
    let pre_io = dag.in_flight_io_permits();
    // Loan path: returns Some even though the budget is full.
    let loan = dag
        .loan_capacity_for_class(ResourceClass::Io)
        .expect("I/O loan must succeed when budget is full");
    assert_eq!(
        dag.in_flight_io_permits(),
        pre_io + 1,
        "loan must bump the per-class I/O counter past the cap",
    );
    drop(loan);
    assert_eq!(
        dag.in_flight_io_permits(),
        pre_io,
        "loan Drop must release the I/O counter back to its pre-loan level",
    );
}
