//! Priority-lane + weighted-credit readiness selector tests.
//!
//! These tests characterize the lane/credit selector that replaces
//! the linear scan + time-based aging. Every test is discriminating:
//! it fails against the pre-change scan/aging selector OR a naive
//! lanes-without-credit selector, and passes against the
//! smooth-weighted-credit lane selector.
//!
//! Authority split under test:
//! - credit governs which lane is CHOSEN,
//! - the typed CPU/IO ledger governs ADMISSION,
//! - a capacity-skip does NOT debit credit.

use super::*;
use crate::caller_kind::CallerKind;

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

/// Submit a CPU-class (Analysis) ready node at `prio`, return its token.
fn submit_cpu(dag: &mut SchedulerDag, name: &str, prio: Priority) -> SubmissionToken {
    dag.submit(
        file_stage(name, 1, FileStageKey::Analysis),
        WorkKind::Analysis,
        prio,
        Vec::new(),
        None,
    )
}

/// Submit an I/O-class (Source/Load) ready node at `prio`, return its token.
fn submit_io(dag: &mut SchedulerDag, name: &str, prio: Priority) -> SubmissionToken {
    dag.submit(
        file_stage(name, 1, FileStageKey::Source),
        WorkKind::Load,
        prio,
        Vec::new(),
        None,
    )
}

// ──────────────────────────────────────────────────────────────────
// Lane membership invariants
// ──────────────────────────────────────────────────────────────────

/// A freshly-submitted ready node enters the lane matching its
/// `(Priority, ResourceClass)`. A node with unresolved deps does NOT
/// enter any lane.
///
/// Fails pre-impl: there is no lane index, so
/// `ready_lane_membership_for_test` does not exist.
#[test]
fn submit_ready_node_enters_priority_class_lane() {
    let mut dag = SchedulerDag::new();
    let cpu = submit_cpu(&mut dag, "/a.ts", Priority::Interactive);
    let io = submit_io(&mut dag, "/b.vue", Priority::Critical);

    assert_eq!(
        dag.ready_lane_membership_for_test(cpu),
        Some((Priority::Interactive, ResourceClass::Cpu)),
    );
    assert_eq!(
        dag.ready_lane_membership_for_test(io),
        Some((Priority::Critical, ResourceClass::Io)),
    );

    // A gated node does not enter any lane.
    let dep = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep_tok = submit_cpu(&mut dag, "/dep.ts", Priority::Interactive);
    let gated = dag.submit(
        artifact("/c.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![DepKey::from_identity(&dep)],
        None,
    );
    assert_eq!(
        dag.ready_lane_membership_for_test(gated),
        None,
        "a node with unresolved deps must not be in any ready lane",
    );
}

/// Dispatch removes the token from its lane. Complete/cancel of an
/// already-dispatched node leaves no lane residue.
#[test]
fn dispatch_removes_token_from_lane() {
    let mut dag = SchedulerDag::new();
    let cpu = submit_cpu(&mut dag, "/a.ts", Priority::Interactive);
    assert!(dag.ready_lane_membership_for_test(cpu).is_some());
    let job = dag.next_ready().expect("ready");
    assert_eq!(job.token, cpu);
    assert_eq!(
        dag.ready_lane_membership_for_test(cpu),
        None,
        "dispatched node must leave its lane",
    );
}

/// `complete` on a dep moves the gated waiter INTO its lane (the
/// newly-ready transition is the lane-insert hook).
#[test]
fn complete_dep_moves_waiter_into_lane() {
    let mut dag = SchedulerDag::new();
    let dep = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep_tok = submit_cpu(&mut dag, "/dep.ts", Priority::Interactive);
    let waiter = dag.submit(
        artifact("/a.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Background,
        vec![DepKey::from_identity(&dep)],
        None,
    );
    assert_eq!(dag.ready_lane_membership_for_test(waiter), None);
    // Dispatch + complete the dep.
    let _ = dag.next_ready().expect("dep ready");
    let newly = dag.complete(&dep);
    assert_eq!(newly, vec![waiter]);
    assert_eq!(
        dag.ready_lane_membership_for_test(waiter),
        Some((Priority::Background, ResourceClass::Cpu)),
        "completing the dep must place the waiter in its lane",
    );
}

/// Pre-dispatch dedup that ADDS a new dep removes a previously-ready
/// token from its lane (it is no longer dep-ready).
///
/// Fails pre-impl: no lane to be removed from; with naive lanes the
/// dedup arm that mutates `deps_remaining` would forget to un-enqueue.
#[test]
fn pre_dispatch_dedup_adding_dep_removes_ready_token_from_lane() {
    let mut dag = SchedulerDag::new();
    let target = file_stage("/a.vue", 1, FileStageKey::Source);
    let t1 = dag.submit(
        target.clone(),
        WorkKind::Load,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    assert_eq!(
        dag.ready_lane_membership_for_test(t1),
        Some((Priority::Interactive, ResourceClass::Io)),
        "node is ready (no deps) so it must start in a lane",
    );
    // A second submit of the same identity that carries a fresh dep
    // merges the dep into deps_remaining — the node is no longer ready.
    let new_dep = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep_tok = submit_cpu(&mut dag, "/dep.ts", Priority::Interactive);
    let t2 = dag.submit(
        target.clone(),
        WorkKind::Load,
        Priority::Interactive,
        vec![DepKey::from_identity(&new_dep)],
        None,
    );
    assert_eq!(t1, t2, "dedup returns the same token");
    assert_eq!(
        dag.ready_lane_membership_for_test(t1),
        None,
        "a pre-dispatch dedup that adds a dep must remove the token from its lane",
    );
}

/// Priority upgrade of an already-ready, pending node migrates it to
/// the higher-priority lane (lane migration).
///
/// Fails pre-impl: lanes don't exist; with naive lanes the dedup
/// upgrade only changes `base_priority` and the token stays in the
/// stale lane.
#[test]
fn priority_upgrade_migrates_ready_token_to_higher_lane() {
    let mut dag = SchedulerDag::new();
    let id = file_stage("/a.ts", 1, FileStageKey::Analysis);
    let t = dag.submit(
        id.clone(),
        WorkKind::Analysis,
        Priority::Background,
        Vec::new(),
        None,
    );
    assert_eq!(
        dag.ready_lane_membership_for_test(t),
        Some((Priority::Background, ResourceClass::Cpu)),
    );
    // Re-submit at a higher priority (dedup upgrade).
    let t2 = dag.submit(
        id.clone(),
        WorkKind::Analysis,
        Priority::Critical,
        Vec::new(),
        None,
    );
    assert_eq!(t, t2);
    assert_eq!(
        dag.ready_lane_membership_for_test(t),
        Some((Priority::Critical, ResourceClass::Cpu)),
        "an already-ready token must migrate to the higher lane on priority upgrade",
    );
    // And the upgraded priority is what dispatch reports.
    let job = dag.next_ready().expect("ready");
    assert_eq!(job.token, t);
    assert_eq!(job.priority, Priority::Critical);
}

/// `upgrade_priority` (the explicit entry, not dedup) also migrates a
/// ready token's lane.
#[test]
fn explicit_upgrade_priority_migrates_ready_token_lane() {
    let mut dag = SchedulerDag::new();
    let id = file_stage("/a.ts", 1, FileStageKey::Analysis);
    let t = dag.submit(
        id.clone(),
        WorkKind::Analysis,
        Priority::Background,
        Vec::new(),
        None,
    );
    assert_eq!(
        dag.ready_lane_membership_for_test(t),
        Some((Priority::Background, ResourceClass::Cpu)),
    );
    let upgraded = dag.upgrade_priority(&id, Priority::Interactive);
    assert_eq!(upgraded, Some(Priority::Interactive));
    assert_eq!(
        dag.ready_lane_membership_for_test(t),
        Some((Priority::Interactive, ResourceClass::Cpu)),
    );
}

/// A node made ready through the terminal-failure fan-out (it carries
/// a `FailedDepRecord`) must enter its lane so the stranded work
/// actually dispatches. Discriminator: if the fan-out path forgot the
/// lane-insert hook, the failed waiter would never dispatch and
/// `next_ready` would return `None`.
#[test]
fn terminal_failure_fanout_makes_stranded_waiter_dispatchable() {
    let mut dag = SchedulerDag::new();
    let dep = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep_tok = submit_cpu(&mut dag, "/dep.ts", Priority::Interactive);
    let waiter = dag.submit(
        artifact("/a.vue", 1, 7),
        WorkKind::Artifact,
        Priority::Interactive,
        vec![DepKey::from_identity(&dep)],
        None,
    );
    assert_eq!(dag.ready_lane_membership_for_test(waiter), None);
    // Dispatch the dep so it is in-flight, then terminalize its
    // Analysis. The fan-out strands the waiter (deps cleared) and
    // records a FailedDepRecord on it.
    let _ = dag.next_ready().expect("dep ready");
    let stranded = dag.fanout_analysis_failure_to_waiters(
        &canonical("/dep.ts"),
        1,
        &crate::job::SchedulerError::StageFailed {
            file_id: "/dep.ts".into(),
            stage: "Analysis".into(),
            message: "synthetic".into(),
        },
    );
    assert_eq!(stranded, vec![waiter]);
    assert_eq!(
        dag.ready_lane_membership_for_test(waiter),
        Some((Priority::Interactive, ResourceClass::Cpu)),
        "a terminal-failure-stranded waiter must enter its lane",
    );
    // It must dispatch (then short-circuit with DependencyFailed at execute time).
    let job = dag.next_ready().expect("stranded waiter must dispatch");
    assert_eq!(job.token, waiter);
    assert!(
        !job.failed_blocker_deps.is_empty(),
        "the dispatched stranded waiter must carry its FailedDepRecord",
    );
}

/// `clear` empties every lane.
#[test]
fn clear_empties_all_lanes() {
    let mut dag = SchedulerDag::new();
    let a = submit_cpu(&mut dag, "/a.ts", Priority::Critical);
    let b = submit_io(&mut dag, "/b.vue", Priority::Maintenance);
    assert!(dag.ready_lane_membership_for_test(a).is_some());
    assert!(dag.ready_lane_membership_for_test(b).is_some());
    dag.clear();
    assert_eq!(dag.ready_lane_membership_for_test(a), None);
    assert_eq!(dag.ready_lane_membership_for_test(b), None);
    assert_eq!(dag.in_flight_permits(), 0);
}

// ──────────────────────────────────────────────────────────────────
// Anti-starvation (weighted credit)
// ──────────────────────────────────────────────────────────────────

/// No-starvation WITHOUT aging: a single Maintenance token behind a
/// sustained stream of freshly-arriving Critical tokens dispatches
/// within 15 successful selections, using NO `thread::sleep`.
///
/// Fails against a strict-priority no-credit selector: Critical work
/// always wins, so the Maintenance token never dispatches (the loop
/// would exhaust its 15-iteration budget). Fails against the aging
/// selector under this test because aging is time-based and there are
/// no sleeps (Maintenance never promotes within the budget). Passes
/// only with selection-count weighted credit.
#[test]
fn maintenance_dispatches_within_fifteen_selections_no_sleeps() {
    let mut dag = SchedulerDag::new();
    // One lone Maintenance token.
    let maint = submit_cpu(&mut dag, "/maint.ts", Priority::Maintenance);

    let mut maint_seen = false;
    for i in 0..15usize {
        // Keep the Critical lane perpetually fed: each round there is
        // always at least one fresh Critical token ready.
        let crit_name = format!("/crit-{i}.ts");
        submit_cpu(&mut dag, &crit_name, Priority::Critical);
        let job = dag.next_ready().expect("a job must be ready every round");
        // Complete it so its permit returns (cpu budget default 8).
        let _ = dag.complete(&job.identity);
        if job.token == maint {
            maint_seen = true;
            break;
        }
    }
    assert!(
        maint_seen,
        "the lone Maintenance token must dispatch within 15 successful selections \
         despite a sustained Critical stream (weighted credit, no aging, no sleeps)",
    );
}

/// Background dispatches within 8 successful selections behind a
/// sustained Critical+Interactive stream (Background weight 2 vs
/// eligible-sum bound). Tightens the bound below Maintenance's 15.
#[test]
fn background_dispatches_within_eight_selections_no_sleeps() {
    let mut dag = SchedulerDag::new();
    let bg = submit_cpu(&mut dag, "/bg.ts", Priority::Background);
    let mut seen = false;
    for i in 0..8usize {
        submit_cpu(&mut dag, &format!("/crit-{i}.ts"), Priority::Critical);
        let job = dag.next_ready().expect("ready");
        let _ = dag.complete(&job.identity);
        if job.token == bg {
            seen = true;
            break;
        }
    }
    assert!(
        seen,
        "Background must dispatch within 8 successful selections behind a Critical stream",
    );
}

/// With all four lanes continuously eligible, the long-run service
/// distribution stays within the weighted-credit bounds (Critical
/// served most, Maintenance least, monotone by weight). Fairness
/// distribution check (no sleeps).
#[test]
fn long_run_service_distribution_is_weight_monotone() {
    let mut dag = SchedulerDag::new();
    let mut counts = std::collections::HashMap::<Priority, usize>::new();
    let rounds = 300usize;
    for i in 0..rounds {
        // Keep all four lanes fed every round.
        submit_cpu(&mut dag, &format!("/crit-{i}.ts"), Priority::Critical);
        submit_cpu(&mut dag, &format!("/int-{i}.ts"), Priority::Interactive);
        submit_cpu(&mut dag, &format!("/bg-{i}.ts"), Priority::Background);
        submit_cpu(&mut dag, &format!("/maint-{i}.ts"), Priority::Maintenance);
        let job = dag.next_ready().expect("ready");
        *counts.entry(job.priority).or_default() += 1;
        let _ = dag.complete(&job.identity);
    }
    let c = *counts.get(&Priority::Critical).unwrap_or(&0);
    let it = *counts.get(&Priority::Interactive).unwrap_or(&0);
    let bg = *counts.get(&Priority::Background).unwrap_or(&0);
    let mt = *counts.get(&Priority::Maintenance).unwrap_or(&0);
    // Weights 8:4:2:1 — service must be monotone by weight and every
    // lane must get nonzero service (no starvation).
    assert!(
        c >= it && it >= bg && bg >= mt,
        "service must be weight-monotone: {c} {it} {bg} {mt}"
    );
    assert!(
        mt > 0,
        "Maintenance must receive nonzero long-run service: {mt}"
    );
    assert!(
        c > mt,
        "Critical must outrank Maintenance in service: {c} vs {mt}"
    );
}

// ──────────────────────────────────────────────────────────────────
// Lane ↔ ledger split (capacity governs admission, not credit)
// ──────────────────────────────────────────────────────────────────

/// A capacity-saturated high-priority CPU lane does NOT burn credit:
/// the lower-priority eligible CPU work still dispatches, AND once the
/// saturated lane frees up it has not been starved by lost credit.
///
/// Discriminator: if a capacity-skip wrongly debited credit, the
/// high-priority lane would lose its accrued credit while skipped and
/// the lower lane would be over-served. Here we prove the saturated
/// lane resumes promptly once capacity frees, and that a second-class
/// (IO) lane never blocked by the CPU saturation dispatches.
#[test]
fn capacity_skip_does_not_debit_credit_and_other_class_dispatches() {
    // CPU budget 1, IO budget 1.
    let budget = DagCapacityBudget { cpu: 1, io: 1 };
    let mut dag = SchedulerDag::with_budget(budget);

    // Two Critical CPU jobs (only 1 CPU permit) + one Background IO job.
    let cpu_a = submit_cpu(&mut dag, "/a.ts", Priority::Critical);
    let _cpu_b = submit_cpu(&mut dag, "/b.ts", Priority::Critical);
    let io = submit_io(&mut dag, "/c.vue", Priority::Background);

    // First selection: highest credit is Critical-CPU. Dispatch cpu_a
    // (consumes the only CPU permit).
    let j1 = dag.next_ready().expect("first");
    assert_eq!(j1.token, cpu_a);
    assert_eq!(dag.in_flight_cpu_permits(), 1);

    // Second selection: Critical-CPU lane is capacity-blocked (cpu
    // saturated, Driver caller → no loan). The selector must fall
    // through to the IO class and dispatch the Background IO job
    // rather than busy-returning None.
    let j2 = dag
        .next_ready()
        .expect("io must dispatch when cpu saturated");
    assert_eq!(
        j2.token, io,
        "the eligible IO job must dispatch despite CPU saturation"
    );
    assert_eq!(dag.in_flight_io_permits(), 1);

    // Third selection: nothing admittable (cpu still saturated, no IO
    // left) → None. The blocked-class state is local to the call.
    assert!(
        dag.next_ready().is_none(),
        "with CPU saturated and IO drained, next_ready returns None (no busy-spin)",
    );

    // Free the CPU permit; the deferred Critical CPU job dispatches —
    // proving its credit was NOT burned while it was capacity-skipped.
    let _ = dag.complete(&j1.identity);
    assert_eq!(dag.in_flight_cpu_permits(), 0);
    let j3 = dag.next_ready().expect("deferred cpu job resumes");
    assert_eq!(j3.kind, WorkKind::Analysis);
}

/// Capacity returns to exactly zero after a mixed CPU/IO drain with a
/// cancel in the middle.
#[test]
fn capacity_returns_to_zero_after_mixed_drain_and_cancel() {
    let budget = DagCapacityBudget { cpu: 2, io: 2 };
    let mut dag = SchedulerDag::with_budget(budget);
    let a = submit_cpu(&mut dag, "/a.ts", Priority::Interactive);
    let b = submit_cpu(&mut dag, "/b.ts", Priority::Background);
    let c = submit_io(&mut dag, "/c.vue", Priority::Interactive);
    let d = submit_io(&mut dag, "/d.vue", Priority::Maintenance);

    // Dispatch all four.
    let mut dispatched = Vec::new();
    while let Some(job) = dag.next_ready() {
        dispatched.push(job);
    }
    assert_eq!(dispatched.len(), 4);
    assert_eq!(dag.in_flight_cpu_permits(), 2);
    assert_eq!(dag.in_flight_io_permits(), 2);

    // Complete two, cancel two — permits must all return.
    let by_tok = |t: SubmissionToken| {
        dispatched
            .iter()
            .find(|j| j.token == t)
            .unwrap()
            .identity
            .clone()
    };
    let _ = dag.complete(&by_tok(a));
    let _ = dag.cancel(&by_tok(b));
    let _ = dag.complete(&by_tok(c));
    let _ = dag.cancel(&by_tok(d));

    assert_eq!(
        dag.in_flight_cpu_permits(),
        0,
        "cpu permits must return to zero"
    );
    assert_eq!(
        dag.in_flight_io_permits(),
        0,
        "io permits must return to zero"
    );
    assert_eq!(dag.in_flight_permits(), 0, "aggregate must return to zero");
}

// ──────────────────────────────────────────────────────────────────
// Capacity-loan (deadlock avoidance) preserved through the lane selector
// ──────────────────────────────────────────────────────────────────

/// THE critical deadlock test, extended through the lane selector: a
/// parked CpuWorker (active_path non-empty) must over-admit its own
/// CPU class to inline-run a transitive CPU dep, EVEN when the CPU
/// lane's class is saturated.
///
/// Setup: CPU budget 1, fully consumed by the parked worker's own
/// permit. A ready CPU dep sits in the Critical lane. A Driver caller
/// would get None (saturated, no loan). A parked CpuWorker MUST get
/// the dep via a loan.
///
/// Discriminator: a lane gate that refuses the saturated CPU lane
/// before checking loan eligibility returns None for the CpuWorker
/// and the single-worker dependency chain DEADLOCKS.
#[test]
fn parked_cpu_worker_loans_over_saturated_cpu_lane_no_deadlock() {
    let budget = DagCapacityBudget { cpu: 1, io: 8 };
    let mut dag = SchedulerDag::with_budget(budget);

    // The parked worker's own in-flight job consumes the only CPU permit.
    let parked_id = file_stage("/parked.ts", 1, FileStageKey::Analysis);
    let _parked = dag.submit(
        parked_id.clone(),
        WorkKind::Analysis,
        Priority::Interactive,
        Vec::new(),
        None,
    );
    let parked_job = dag.next_ready().expect("parked job dispatches");
    assert_eq!(parked_job.identity, parked_id);
    assert_eq!(dag.in_flight_cpu_permits(), 1, "CPU budget now saturated");

    // A transitive CPU dependency becomes ready.
    let dep_id = file_stage("/dep.ts", 1, FileStageKey::Analysis);
    let _dep = dag.submit(
        dep_id.clone(),
        WorkKind::Analysis,
        Priority::Critical,
        Vec::new(),
        None,
    );

    // A Driver caller (no loan) cannot admit the dep — CPU saturated.
    assert!(
        dag.next_ready_for_pump(CallerKind::Driver, &[]).is_none(),
        "Driver caller must NOT over-admit the saturated CPU lane",
    );

    // The parked CpuWorker (active_path = its own job) MUST loan over
    // the saturated CPU lane and receive the dep — else deadlock.
    let active_path = [parked_id.clone()];
    let loaned = dag
        .next_ready_for_pump(CallerKind::CpuWorker, &active_path)
        .expect("parked CpuWorker must loan over a saturated CPU lane to run the dep inline");
    assert_eq!(loaned.identity, dep_id);
    assert_eq!(
        dag.in_flight_cpu_permits(),
        2,
        "the loan bumps the CPU counter past the cap for the inline execute",
    );

    // Both reservations release exactly once.
    let _ = dag.complete(&dep_id);
    assert_eq!(dag.in_flight_cpu_permits(), 1);
    let _ = dag.complete(&parked_id);
    assert_eq!(dag.in_flight_cpu_permits(), 0);
}

/// The loan does NOT fire for a non-matching caller/class combination
/// even when parked: an IoWorker parked on an IO job must NOT loan a
/// CPU permit for a saturated CPU lane.
#[test]
fn loan_does_not_fire_for_cross_class_parked_worker() {
    let budget = DagCapacityBudget { cpu: 1, io: 8 };
    let mut dag = SchedulerDag::with_budget(budget);
    // Saturate CPU.
    let cpu_busy = submit_cpu(&mut dag, "/busy.ts", Priority::Interactive);
    let busy_job = dag.next_ready().expect("busy dispatches");
    assert_eq!(busy_job.token, cpu_busy);
    assert_eq!(dag.in_flight_cpu_permits(), 1);

    // A ready CPU dep.
    let _dep = submit_cpu(&mut dag, "/dep.ts", Priority::Critical);

    // An IoWorker parked on some IO identity must NOT loan a CPU permit.
    let io_path = [file_stage("/io.vue", 1, FileStageKey::Source)];
    assert!(
        dag.next_ready_for_pump(CallerKind::IoWorker, &io_path)
            .is_none(),
        "IoWorker must not loan a CPU permit for a CPU lane (cross-class)",
    );
    assert_eq!(
        dag.in_flight_cpu_permits(),
        1,
        "no cross-class loan was taken"
    );
}

// ──────────────────────────────────────────────────────────────────
// Active-path skip + caller-class preference (preserved)
// ──────────────────────────────────────────────────────────────────

/// Active-path skip: the selector skips only active identities and
/// dispatches the next eligible token.
#[test]
fn active_path_skip_dispatches_next_eligible_token() {
    let mut dag = SchedulerDag::new();
    let active = file_stage("/active.ts", 1, FileStageKey::Analysis);
    let _a = dag.submit(
        active.clone(),
        WorkKind::Analysis,
        Priority::Critical,
        Vec::new(),
        None,
    );
    let other = submit_cpu(&mut dag, "/other.ts", Priority::Background);

    // With the active identity in the path, the higher-priority active
    // token is skipped and the lower-priority other token dispatches.
    let job = dag
        .next_ready_for_pump(CallerKind::CpuWorker, std::slice::from_ref(&active))
        .expect("the non-active token must dispatch");
    assert_eq!(
        job.token, other,
        "active-path skip must skip the active identity and dispatch the next eligible token",
    );
}

/// Caller-class preference still overrides priority: with a CPU and an
/// IO candidate at the SAME priority, a CpuWorker receives CPU first
/// and an IoWorker receives IO first.
#[test]
fn caller_class_preference_overrides_within_lane() {
    // CpuWorker prefers CPU.
    {
        let mut dag = SchedulerDag::with_budget(DagCapacityBudget { cpu: 1, io: 1 });
        let cpu = submit_cpu(&mut dag, "/a.ts", Priority::Interactive);
        let _io = submit_io(&mut dag, "/b.vue", Priority::Interactive);
        let job = dag
            .next_ready_for_pump(CallerKind::CpuWorker, &[])
            .expect("ready");
        assert_eq!(
            job.token, cpu,
            "CpuWorker must receive the CPU candidate first"
        );
    }
    // IoWorker prefers IO.
    {
        let mut dag = SchedulerDag::with_budget(DagCapacityBudget { cpu: 1, io: 1 });
        let _cpu = submit_cpu(&mut dag, "/a.ts", Priority::Interactive);
        let io = submit_io(&mut dag, "/b.vue", Priority::Interactive);
        let job = dag
            .next_ready_for_pump(CallerKind::IoWorker, &[])
            .expect("ready");
        assert_eq!(
            job.token, io,
            "IoWorker must receive the IO candidate first"
        );
    }
}

/// Class preference must NOT cross a priority boundary downward: a
/// CpuWorker with a Critical IO candidate and a Background CPU
/// candidate still respects that the preference operates within the
/// chosen lane, but the class-preference contract (from the legacy
/// tests) floats the worker's own class first across equal-priority
/// candidates. Here the higher-priority IO must still win when the
/// CPU work is strictly lower priority and the IO lane is the only
/// Critical lane — i.e. preference is a within-eligibility bias, not a
/// license to run stale low-priority work.
///
/// This pins that the class bias does not invert priority across
/// lanes for the credit selector: Critical IO outranks Background CPU
/// even for a CpuWorker.
#[test]
fn class_preference_does_not_invert_priority_across_lanes() {
    let mut dag = SchedulerDag::with_budget(DagCapacityBudget { cpu: 8, io: 8 });
    let _bg_cpu = submit_cpu(&mut dag, "/a.ts", Priority::Background);
    let crit_io = submit_io(&mut dag, "/b.vue", Priority::Critical);
    let job = dag
        .next_ready_for_pump(CallerKind::CpuWorker, &[])
        .expect("ready");
    assert_eq!(
        job.token, crit_io,
        "a Critical IO job must outrank a Background CPU job even for a CpuWorker",
    );
}

// ──────────────────────────────────────────────────────────────────
// Strict-priority preservation when no credit promotion fires
// ──────────────────────────────────────────────────────────────────

/// When three nodes of distinct priority are submitted and drained
/// back-to-back (each lane eligible once at selection time), the
/// dispatch order is strict priority: Critical, Interactive,
/// Background. This preserves `next_ready_returns_highest_priority_first`
/// under the credit selector.
#[test]
fn distinct_priorities_dispatch_in_priority_order() {
    let mut dag = SchedulerDag::new();
    let low = submit_io(&mut dag, "/low.vue", Priority::Background);
    let hi = submit_io(&mut dag, "/hi.vue", Priority::Critical);
    let mid = submit_io(&mut dag, "/mid.vue", Priority::Interactive);
    let r1 = dag.next_ready().expect("hi");
    let r2 = dag.next_ready().expect("mid");
    let r3 = dag.next_ready().expect("low");
    assert_eq!(r1.token, hi);
    assert_eq!(r2.token, mid);
    assert_eq!(r3.token, low);
}

// ──────────────────────────────────────────────────────────────────
// P1 — seeded randomized DAG lifecycle model
// ──────────────────────────────────────────────────────────────────

/// Tiny deterministic xorshift PRNG (no external dep) for the model.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn prio_of(i: u64) -> Priority {
    match i % 4 {
        0 => Priority::Critical,
        1 => Priority::Interactive,
        2 => Priority::Background,
        _ => Priority::Maintenance,
    }
}

/// Seeded randomized DAG-lifecycle model: after every operation the
/// set of tokens present in the ready lanes must EXACTLY equal the
/// model's notion of readiness (`!cancelled && !dispatched &&
/// deps_remaining empty`). Exercises submit, dedup, priority upgrade,
/// dep complete, cancel, dispatch/complete with a large capacity so
/// capacity never gates the membership invariant.
#[test]
fn seeded_lifecycle_lane_membership_matches_model() {
    for seed in [1u64, 7, 42, 1337, 99991] {
        run_lifecycle_model(
            seed,
            400,
            DagCapacityBudget {
                cpu: 1024,
                io: 1024,
            },
        );
    }
}

/// Capacity-limited stress variant of the model with active-path
/// cooperative callers. Capacity gates admission but NOT lane
/// membership; the invariant is identical.
#[test]
fn seeded_lifecycle_with_capacity_limits_and_cooperative_callers() {
    for seed in [3u64, 11, 271, 65537] {
        run_lifecycle_model(seed, 250, DagCapacityBudget { cpu: 2, io: 2 });
    }
}

fn run_lifecycle_model(seed: u64, ops: usize, budget: DagCapacityBudget) {
    let mut dag = SchedulerDag::with_budget(budget);
    let mut rng = Rng::new(seed);

    // Model: identity-name -> (token, kind, priority, deps_remaining set, dispatched, present)
    // We use a small universe of file names so dedup / dep edges actually collide.
    let names = ["/m0.ts", "/m1.ts", "/m2.ts", "/m3.ts", "/m4.ts", "/m5.ts"];

    // Track submitted-but-live identities for dep/complete/cancel ops.
    let mut live: Vec<WorkNodeIdentity> = Vec::new();

    for _ in 0..ops {
        let op = rng.below(6);
        match op {
            // submit a (possibly new, possibly dedup) Analysis node,
            // sometimes gated on another live Analysis identity.
            0 | 1 => {
                let name = names[rng.below(names.len() as u64) as usize];
                let id = file_stage(name, 1, FileStageKey::Analysis);
                let prio = prio_of(rng.next_u64());
                // 40% chance to gate on a random live dep (not self).
                let deps = if !live.is_empty() && rng.below(10) < 4 {
                    let dep = live[rng.below(live.len() as u64) as usize].clone();
                    if dep != id {
                        vec![DepKey::from_identity(&dep)]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let _ = dag.submit(id.clone(), WorkKind::Analysis, prio, deps, None);
                if !live.contains(&id) {
                    live.push(id);
                }
            }
            // upgrade priority of a live identity.
            2 => {
                if !live.is_empty() {
                    let id = live[rng.below(live.len() as u64) as usize].clone();
                    let _ = dag.upgrade_priority(&id, prio_of(rng.next_u64()));
                }
            }
            // dispatch one ready job (cooperative caller varies).
            3 => {
                let caller = match rng.below(3) {
                    0 => CallerKind::Driver,
                    1 => CallerKind::CpuWorker,
                    _ => CallerKind::IoWorker,
                };
                // Sometimes pass an active_path frame.
                let path: Vec<WorkNodeIdentity> = if !live.is_empty() && rng.below(10) < 3 {
                    vec![live[rng.below(live.len() as u64) as usize].clone()]
                } else {
                    Vec::new()
                };
                let _ = dag.next_ready_for_pump(caller, &path);
            }
            // complete a live identity.
            4 => {
                if !live.is_empty() {
                    let idx = rng.below(live.len() as u64) as usize;
                    let id = live.remove(idx);
                    let _ = dag.complete(&id);
                }
            }
            // cancel a live identity.
            _ => {
                if !live.is_empty() {
                    let idx = rng.below(live.len() as u64) as usize;
                    let id = live.remove(idx);
                    let _ = dag.cancel(&id);
                }
            }
        }

        // INVARIANT: lane membership == model readiness.
        dag.assert_lane_membership_matches_nodes();
    }
}
