//! The soak runner proved on any worker: the smoke manifest (one tranche
//! per mode) passes, the full manifests match their frozen digests, and every
//! planted defect fails the run for its own reason and no other.
//!
//! The retention account is process-wide, so runs in one process are
//! serialised: a concurrent run's charges would move another run's drain.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use verter_audit::soak::SoakCounts;
use verter_bench::soak::controls::{self, Control};
use verter_bench::soak::gates::{self, GateEvidence};
use verter_bench::soak::isolation::{Answer, IsolationEvidence};
use verter_bench::soak::manifest::{
    LifecycleClass, Manifest, Mode, Target, FULL_TRANCHES, SURGE_PERIOD,
};
use verter_bench::soak::refusal::{Demand, DemandRole, RefusalEvidence, RefusalVariant};
use verter_bench::soak::run::{
    run, ActionRecord, Outcome, OwnerStep, OwnerStepKind, PressureEvidence, RunOptions, Sample,
    SoakSummary, SUMMARY_FILE,
};
use verter_bench::soak::verify::{
    check_isolation, check_ownership, check_pressure_outcomes, check_refusal, verify,
    verify_summary, SoakFailure, ABSENT_DELIVERY, FORMER_ACTIVE_TARGET_BYTES,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn smoke(mode: Mode) -> (SoakSummary, Vec<SoakFailure>) {
    let _serial = serial();
    let dir = tempfile::tempdir().expect("a temp dir");
    let summary = run(&RunOptions {
        mode,
        tranches: 1,
        out_dir: dir.path().to_path_buf(),
        mutation: None,
    })
    .expect("the soak runs");
    assert!(
        dir.path().join(SUMMARY_FILE).exists(),
        "a run writes its summary"
    );
    let failures = verify(mode, 1, dir.path());
    (summary, failures)
}

fn assert_smoke_passes(mode: Mode) {
    let (summary, failures) = smoke(mode);
    assert!(failures.is_empty(), "{mode:?} smoke failed: {failures:#?}");
    assert_eq!(summary.records.len(), 100);
    let classes: BTreeSet<_> = summary.records.iter().map(|r| r.class).collect();
    assert_eq!(
        classes,
        LifecycleClass::required(mode, 1).into_iter().collect(),
        "the smoke tranche drives exactly its mode's lifecycle classes"
    );
    // Capture is off: the run is the shipped configuration, and the workload
    // retains something between baseline and teardown.
    let baseline = &summary.samples.first().expect("a baseline").counts;
    let last = &summary.samples.last().expect("a final sample").counts;
    assert!(
        summary
            .samples
            .iter()
            .any(|s| s.counts.retained_bytes > baseline.retained_bytes),
        "the workload retains something between baseline and teardown"
    );
    assert!(last.retained_bytes <= baseline.retained_bytes);
}

#[test]
fn smoke_normal_passes_and_drains_to_baseline() {
    assert_smoke_passes(Mode::Normal);
}

#[test]
fn smoke_pressure_passes_and_drains_to_baseline() {
    assert_smoke_passes(Mode::Pressure);
}

/// One cold demand, then repeated demand/close cycles on a live host, return
/// every drained figure to the baseline while the host stays reusable.
#[test]
fn repeated_demand_close_cycles_return_to_baseline_on_a_live_host() {
    use std::sync::Arc;
    use verter_bench::soak::run::counts;
    use verter_session::{HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};
    let _serial = serial();
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let baseline = counts(&host);
    for _ in 0..3 {
        let id = "/soak/cycle.ts".to_string();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.clone()),
                input_id: id.clone(),
                source: Arc::from("export type A = { readonly k: \"a\" };\n"),
                file_language: verter_session::LanguageRegistry::global()
                    .classify_static(&id)
                    .static_resolution(),
                aliases: Vec::new(),
            })
            .expect("an upsert");
        let answer = host.resolve_named_symbol(&id, "A", None);
        assert!(answer.is_some(), "the cold demand answers");
        drop(answer);
        host.evict(&id);
        host.remove(&id).expect("the module was open");
        host.close();
        host.reclaim_retired_artifacts();
        let closed = counts(&host);
        let leaks = verter_audit::soak::drain_violations(
            &baseline,
            &closed,
            verter_audit::soak::DrainPoint::Closed,
        );
        assert!(leaks.is_empty(), "close left {leaks:#?}");
    }
}

/// The registered manifest freezes both full workloads by digest; a change
/// to the generator, the template or the fixtures moves it.
#[test]
fn full_manifests_match_the_frozen_registration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/benchmark/evidence-runs/rev11-l1-soak.json");
    let text = std::fs::read_to_string(&path).expect("the registered manifest");
    let registered: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let soak = &registered["soak"];
    assert_eq!(soak["tranches"], FULL_TRANCHES);
    for mode in Mode::ALL {
        let manifest = Manifest::generate(mode, FULL_TRANCHES);
        let frozen = &soak["workloads"][mode.as_str()];
        assert_eq!(manifest.actions.len(), 10_000);
        assert_eq!(frozen["actions"], 10_000);
        assert_eq!(
            frozen["digest"].as_str(),
            Some(manifest.digest().as_str()),
            "{mode:?} full manifest digest"
        );
        let min = soak["minDistinctQueries"].as_u64().expect("a minimum");
        assert!(manifest.distinct_queries() as u64 >= min);
        assert_eq!(frozen["distinctQueries"], manifest.distinct_queries());
        assert_eq!(
            Manifest::generate(mode, 1).digest(),
            soak["smoke"][mode.as_str()]["digest"]
                .as_str()
                .unwrap_or_default(),
            "{mode:?} smoke manifest digest"
        );
    }
}

/// The frozen identity covers the fixtures the actions publish: the same
/// actions over different source bytes are a different workload.
#[test]
fn the_manifest_digest_binds_fixture_contents() {
    use verter_bench::soak::fixtures::content_digest;
    assert_eq!(
        content_digest(Mode::Pressure, FULL_TRANCHES),
        content_digest(Mode::Pressure, FULL_TRANCHES),
        "unchanged generation is stable"
    );
    assert_ne!(
        content_digest(Mode::Pressure, FULL_TRANCHES),
        content_digest(Mode::Normal, FULL_TRANCHES),
        "the pressure fixtures differ from the normal ones"
    );
    // Dropping a fixture from the digest's inputs would not move it; the
    // digest of a manifest therefore changes with the content digest.
    let manifest = Manifest::generate(Mode::Normal, 1);
    let mut bytes = serde_json::to_vec(&manifest.actions).expect("actions serialise");
    let actions_only = verter_bench::soak::manifest::sha256_hex(&bytes);
    assert_ne!(manifest.digest(), actions_only);
    bytes.extend_from_slice(b"other fixtures");
    assert_ne!(
        verter_bench::soak::manifest::sha256_hex(&bytes),
        manifest.digest()
    );
}

/// Run `control`: it must be applied and fail the run for its own reason.
fn assert_control_bites(control: &Control) {
    let _serial = serial();
    let dir = tempfile::tempdir().expect("a temp dir");
    let report = controls::execute(control, dir.path()).expect("the control runs");
    assert!(
        report.applied,
        "{} was not applied: {report:#?}",
        control.name
    );
    assert_eq!(report.kinds, report.expected_kinds, "{}", control.name);
    assert!(report.leak_figure_found, "{}: {report:#?}", control.name);
    assert!(report.bit);
}

fn control(name: &str) -> Control {
    controls::all()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no control {name}"))
}

#[test]
fn control_retained_key_leak_fails_the_drain() {
    assert_control_bites(&control("retained-key-leak"));
}

#[test]
fn control_omitted_workload_class_fails_the_inventory() {
    assert_control_bites(&control("omitted-workload-class"));
}

#[test]
fn control_omitted_cancellation_fails_the_cancellation_rule() {
    assert_control_bites(&control("omitted-cancellation"));
}

#[test]
fn control_fake_overlap_fails_the_overlap_witness() {
    assert_control_bites(&control("fake-overlap"));
}

#[test]
fn control_wrong_project_reversion_fails_the_sentinel() {
    assert_control_bites(&control("wrong-project-reversion"));
}

#[test]
fn control_missing_summary_fails_the_verdict() {
    assert_control_bites(&control("missing-summary"));
}

#[test]
fn control_unexpected_skip_fails_the_verdict() {
    assert_control_bites(&control("unexpected-skip"));
}

/// Every control the runner ships is exercised by a test above.
#[test]
fn every_control_has_a_test() {
    let names: BTreeSet<_> = controls::all().iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "retained-key-leak",
            "omitted-workload-class",
            "omitted-cancellation",
            "fake-overlap",
            "wrong-project-reversion",
            "missing-summary",
            "unexpected-skip",
        ])
    );
}

fn answer(digest: &str, complete: bool) -> Answer {
    Answer {
        delivered: true,
        complete,
        digest: digest.into(),
    }
}

fn evidence() -> IsolationEvidence {
    IsolationEvidence {
        callback_ran: true,
        ops_before: 36,
        same_budget: true,
        spent: answer("spent", false),
        isolated: answer("isolated", true),
        repeat: answer("isolated", true),
        spent_recomputes: 1,
        isolated_recomputes: 1,
        repeat_recomputes: 0,
        oracle_spent: answer("spent", false),
        oracle_isolated: answer("isolated", true),
    }
}

fn isolation_details(evidence: &IsolationEvidence) -> Vec<String> {
    check_isolation(0, evidence)
        .into_iter()
        .map(|f| match f {
            SoakFailure::IsolationBroken { detail, .. } => detail,
            other => panic!("unexpected failure {other:?}"),
        })
        .collect()
}

#[test]
fn isolation_rules_fail_each_broken_property() {
    assert!(isolation_details(&evidence()).is_empty());
    type Break = fn(&mut IsolationEvidence);
    let cases: [(&str, Break); 12] = [
        ("never ran", |e| e.callback_ran = false),
        ("spent no operations", |e| e.ops_before = 0),
        ("batch's budget", |e| e.same_budget = false),
        ("incomplete budget refusal", |e| {
            e.spent = answer("spent", true);
            e.oracle_spent = e.spent.clone();
        }),
        ("incomplete budget refusal", |e| {
            e.spent = Answer::absent();
            e.oracle_spent = Answer::absent();
        }),
        ("complete answer", |e| {
            e.isolated = Answer::absent();
            e.oracle_isolated = Answer::absent();
            e.repeat = Answer::absent();
        }),
        ("spent-context answer differs", |e| {
            e.oracle_spent = answer("other", false)
        }),
        ("fresh-request answer differs", |e| {
            e.oracle_isolated = answer("other", true)
        }),
        ("repeat differs", |e| e.repeat = answer("other", true)),
        ("answered the fresh request's answer", |e| {
            e.spent = e.isolated.clone()
        }),
        ("answered without computing", |e| e.spent_recomputes = 0),
        ("retained result", |e| e.repeat_recomputes = 1),
    ];
    for (needle, plant) in cases {
        let mut broken = evidence();
        plant(&mut broken);
        let details = isolation_details(&broken);
        assert!(
            details.iter().any(|d| d.contains(needle)),
            "{needle}: {details:?}"
        );
    }
}

fn demand(role: DemandRole, answer: Answer, recomputes: u64) -> Demand {
    Demand {
        role,
        answer,
        recomputes,
    }
}

fn refusal(variant: RefusalVariant) -> RefusalEvidence {
    let sealed = answer("sealed", false);
    use DemandRole::*;
    let demands = match variant {
        RefusalVariant::Sealed => vec![
            demand(Sealed, sealed.clone(), 3),
            demand(Repeat, sealed.clone(), 0),
            demand(Repeat, sealed.clone(), 0),
            demand(Oracle, sealed, 3),
        ],
        RefusalVariant::Edited => {
            let mut out = vec![demand(Sealed, sealed, 3)];
            for edit in 1..=3 {
                let refused = answer(&format!("edit{edit}"), false);
                out.push(demand(Edit, refused.clone(), 3));
                out.push(demand(EditRepeat, refused, 0));
            }
            out
        }
        RefusalVariant::Healing => vec![
            demand(Sealed, sealed.clone(), 3),
            demand(Healed, answer("healed", true), 5),
            demand(TightAfterHealed, answer("healed", true), 0),
        ],
        RefusalVariant::Concurrent => vec![
            demand(Sealed, sealed.clone(), 3),
            demand(ConcurrentTight, sealed.clone(), 0),
            demand(ConcurrentLarge, answer("healed", true), 0),
            demand(ConcurrentTight, sealed, 0),
            demand(ConcurrentLarge, answer("healed", true), 0),
        ],
    };
    RefusalEvidence { variant, demands }
}

fn refusal_details(evidence: &RefusalEvidence) -> Vec<String> {
    check_refusal(0, evidence)
        .into_iter()
        .map(|f| match f {
            SoakFailure::RefusalBroken { detail, .. } => detail,
            other => panic!("unexpected failure {other:?}"),
        })
        .collect()
}

#[test]
fn refusal_rules_fail_each_broken_property() {
    for variant in [
        RefusalVariant::Sealed,
        RefusalVariant::Edited,
        RefusalVariant::Healing,
        RefusalVariant::Concurrent,
    ] {
        assert!(
            refusal_details(&refusal(variant)).is_empty(),
            "{variant:?} baseline"
        );
    }
    // A missing symbol is delivered nothing, never a sealed refusal.
    let mut missing = refusal(RefusalVariant::Sealed);
    missing.demands[0].answer = Answer::absent();
    assert!(!refusal_details(&missing).is_empty());
    // A repeat that resolves again is not a retained refusal.
    let mut recomputed = refusal(RefusalVariant::Sealed);
    recomputed.demands[1].recomputes = 2;
    assert!(!refusal_details(&recomputed).is_empty());
    // A repeat that completes is not the refusal.
    let mut completed = refusal(RefusalVariant::Sealed);
    completed.demands[2].answer = answer("sealed", true);
    assert!(!refusal_details(&completed).is_empty());
    // A larger profile that does not heal.
    let mut unhealed = refusal(RefusalVariant::Healing);
    unhealed.demands[1].answer = answer("sealed", false);
    assert!(!refusal_details(&unhealed).is_empty());
    // An edit that served the old refusal.
    let mut stale = refusal(RefusalVariant::Edited);
    stale.demands[1].answer = answer("sealed", false);
    assert!(!refusal_details(&stale).is_empty());
    // A larger profile that replays the tight profile's retained refusal.
    let mut replayed = refusal(RefusalVariant::Healing);
    replayed.demands[1].answer = answer("sealed", false);
    replayed.demands[1].recomputes = 0;
    assert!(!refusal_details(&replayed).is_empty());
    // Concurrent profile variants that disagree.
    let mut mixed = refusal(RefusalVariant::Concurrent);
    mixed.demands[3].answer = answer("other", false);
    assert!(!refusal_details(&mixed).is_empty());
}

fn step(kind: OwnerStepKind, roots: u64, projected: Option<&str>) -> OwnerStep {
    OwnerStep {
        step: kind,
        roots,
        projected: projected.map(str::to_string),
    }
}

fn release_steps() -> Vec<OwnerStep> {
    vec![
        step(OwnerStepKind::BeforeRelease, 5, Some("held")),
        step(OwnerStepKind::AfterFirstRelease, 5, Some("held")),
        step(OwnerStepKind::AfterFinalRelease, 4, None),
    ]
}

#[test]
fn ownership_rules_fail_early_destruction_double_charge_and_lost_release() {
    let held = "held".to_string();
    let release = LifecycleClass::ReleaseCopyThenOriginal;
    assert!(check_ownership(0, release, Some(&held), &release_steps()).is_empty());
    let copy = vec![
        step(OwnerStepKind::BeforeCopy, 5, Some("held")),
        step(OwnerStepKind::AfterCopy, 5, Some("held")),
    ];
    assert!(check_ownership(0, LifecycleClass::PublicCopy, Some(&held), &copy).is_empty());
    // The copy became a second independent root (a duplicated charge).
    let mut doubled = copy.clone();
    doubled[1].roots = 6;
    assert!(!check_ownership(0, LifecycleClass::PublicCopy, Some(&held), &doubled).is_empty());
    // The first release destroyed what the surviving handle reads.
    let mut early = release_steps();
    early[1].projected = Some("gone".into());
    assert!(!check_ownership(0, release, Some(&held), &early).is_empty());
    // The first release dropped a root while a handle survives.
    let mut dropped = release_steps();
    dropped[1].roots = 4;
    assert!(!check_ownership(0, release, Some(&held), &dropped).is_empty());
    // The final release gave nothing back.
    let mut kept = release_steps();
    kept[2].roots = 5;
    assert!(!check_ownership(0, release, Some(&held), &kept).is_empty());
}

fn surge(pressure: Option<u64>, oversized: Option<u64>, outcome: Outcome) -> ActionRecord {
    ActionRecord {
        outcome,
        digests: (0..300).map(|i| format!("d{i}")).collect(),
        refusals_pressure: pressure,
        refusals_oversized: oversized,
        pressure: Some(PressureEvidence {
            memo_candidates_before: 0,
            memo_candidates_after: 0,
            refusals_active: 0,
            peak_total_bytes: FORMER_ACTIVE_TARGET_BYTES + 1,
        }),
        ..ActionRecord::new(0, LifecycleClass::PressureSurge)
    }
}

fn surge_manifest() -> Vec<verter_bench::soak::manifest::Action> {
    Manifest::generate(Mode::Pressure, SURGE_PERIOD)
        .actions
        .into_iter()
        .filter(|a| matches!(a.target, Target::Surge { .. }))
        .map(|mut a| {
            a.index = 0;
            a
        })
        .collect()
}

fn pressure_details(records: &[ActionRecord]) -> Vec<String> {
    let mut failures = Vec::new();
    check_pressure_outcomes(&surge_manifest(), records, &mut failures);
    failures
        .into_iter()
        .map(|f| match f {
            SoakFailure::PressureNotExercised { detail } => detail,
            other => panic!("unexpected failure {other:?}"),
        })
        .collect()
}

#[test]
fn full_pressure_outcomes_fail_zero_missing_or_incomplete_evidence() {
    assert!(pressure_details(&[surge(Some(3), Some(1), Outcome::Value)]).is_empty());
    let cases = [
        (vec![], "ran no surge"),
        (
            vec![surge(Some(0), Some(1), Outcome::Value)],
            "aggregate pressure",
        ),
        (
            vec![surge(None, Some(1), Outcome::Value)],
            "aggregate pressure",
        ),
        (vec![surge(Some(3), Some(0), Outcome::Value)], "oversized"),
        (vec![surge(Some(3), None, Outcome::Value)], "oversized"),
        (vec![surge(Some(3), Some(1), Outcome::Miss)], "complete"),
    ];
    for (records, needle) in cases {
        let details = pressure_details(&records);
        assert!(
            details.iter().any(|d| d.contains(needle)),
            "{needle}: {details:?}"
        );
    }
    // A dropped result, a missing delivery, an active refusal or work that
    // never crossed the former active target each fail.
    let mut dropped = surge(Some(3), Some(1), Outcome::Value);
    dropped.digests.pop();
    assert!(!pressure_details(&[dropped]).is_empty());
    let mut undelivered = surge(Some(3), Some(1), Outcome::Value);
    undelivered.digests[7] = ABSENT_DELIVERY.into();
    assert!(!pressure_details(&[undelivered]).is_empty());
    let mut refused_active = surge(Some(3), Some(1), Outcome::Value);
    refused_active.pressure.as_mut().unwrap().refusals_active = 1;
    assert!(!pressure_details(&[refused_active]).is_empty());
    let mut shallow = surge(Some(3), Some(1), Outcome::Value);
    shallow.pressure.as_mut().unwrap().peak_total_bytes = 1;
    assert!(!pressure_details(&[shallow]).is_empty());
}

/// The surge runs only in the full pressure workload's surge tranches, and a
/// full manifest requires its class.
#[test]
fn surge_tranches_are_frozen_in_the_full_pressure_manifest_only() {
    let surges = |mode, tranches| {
        Manifest::generate(mode, tranches)
            .actions
            .into_iter()
            .filter(|a| matches!(a.target, Target::Surge { .. }))
            .map(|a| a.tranche)
            .collect::<Vec<_>>()
    };
    assert_eq!(surges(Mode::Pressure, 1), Vec::<u32>::new());
    assert_eq!(surges(Mode::Normal, FULL_TRANCHES), Vec::<u32>::new());
    let full = surges(Mode::Pressure, FULL_TRANCHES);
    assert_eq!(
        full,
        (SURGE_PERIOD - 1..FULL_TRANCHES)
            .step_by(SURGE_PERIOD as usize)
            .collect::<Vec<_>>()
    );
    assert!(LifecycleClass::required(Mode::Pressure, FULL_TRANCHES)
        .contains(&LifecycleClass::PressureSurge));
    assert!(!LifecycleClass::required(Mode::Pressure, 1).contains(&LifecycleClass::PressureSurge));
}

fn sample(at: &str, actions_done: u32, counts: SoakCounts) -> Sample {
    Sample {
        at: at.into(),
        actions_done,
        counts,
    }
}

/// A sample above an absolute workload limit fails, in the opening window as
/// well as after it, however large the envelope it would otherwise set.
#[test]
fn absolute_limits_fail_in_the_opening_window() {
    let _serial = serial();
    let dir = tempfile::tempdir().expect("a temp dir");
    let mut summary = run(&RunOptions {
        mode: Mode::Normal,
        tranches: 1,
        out_dir: dir.path().to_path_buf(),
        mutation: None,
    })
    .expect("the soak runs");
    assert!(verify_summary(Mode::Normal, 1, &summary).is_empty());
    let mut over = summary.samples[1].counts.clone();
    over.active_bytes = 600 * 1024 * 1024;
    over.live_artifacts = 1_000;
    over.freshness_exact_entries = 5_000;
    summary.samples.insert(2, sample("action", 25, over));
    let failures = verify_summary(Mode::Normal, 1, &summary);
    let limit = failures.iter().find_map(|f| match f {
        SoakFailure::AbsoluteLimitExceeded { violations, .. } => Some(violations),
        _ => None,
    });
    let figures: BTreeSet<_> = limit
        .expect("the oversized sample fails an absolute limit")
        .iter()
        .map(|v| v.figure.as_str())
        .collect();
    assert!(figures.contains("active_bytes") && figures.contains("live_artifacts"));
    assert!(figures.contains("freshness_exact_entries"));
}

/// Each population a predecessor retires is part of the judged sample: a
/// retained entry in any of them fails the drain.
#[test]
fn every_lifecycle_population_is_judged_for_drain() {
    use verter_audit::soak::{drain_violations, CountRole, DrainPoint};
    let judged: BTreeSet<&str> = SoakCounts::default()
        .figures()
        .into_iter()
        .filter(|(_, role, _)| *role == CountRole::Drains)
        .map(|(name, _, _)| name)
        .collect();
    for population in [
        "overlay_resolution_slots",
        "resolution_slots",
        "resolution_candidates",
        "resolution_derived_nodes",
        "freshness_view_leases",
        "publication_lanes",
        "carrier_candidates",
        "pending_destroyed_nodes",
        "snapshot_leases",
    ] {
        assert!(judged.contains(population), "{population} is not judged");
    }
    let baseline = SoakCounts::default();
    // Amortised history is not drained at close; an absolute bound holds it
    // instead.
    let retained = SoakCounts {
        resolution_slots: 1,
        freshness_view_leases: 1,
        freshness_exact_entries: 1,
        ..SoakCounts::default()
    };
    let figures: Vec<_> = drain_violations(&baseline, &retained, DrainPoint::Closed)
        .into_iter()
        .map(|v| v.figure)
        .collect();
    assert_eq!(figures, ["resolution_slots", "freshness_view_leases"]);
}

fn gate_details(evidence: &GateEvidence) -> Vec<String> {
    verter_bench::soak::verify::check_gates(0, evidence)
        .into_iter()
        .map(|f| match f {
            SoakFailure::WrongOutcome { detail, .. } => detail,
            other => panic!("unexpected failure {other:?}"),
        })
        .collect()
}

#[test]
fn gate_drive_through_the_reconciler_satisfies_every_gate_rule() {
    let _serial = serial();
    let observed = gates::run(0, 128);
    assert_eq!(
        gate_details(&observed),
        Vec::<String>::new(),
        "{observed:?}"
    );
}

#[test]
fn gate_rules_fail_each_broken_property() {
    let sound = GateEvidence {
        sources: 4,
        holders_entered: 4,
        live_with_holders: 4,
        waited_sources: 4,
        cancelled_left: 2,
        cancelled: 2,
        completed: 6,
        live_after: 0,
        participants_after: 0,
        capacity_peak: 128,
        capacity_after: 16,
    };
    assert!(gate_details(&sound).is_empty());
    type Break = fn(&mut GateEvidence);
    let cases: [(&str, Break); 8] = [
        // More cancellations than transitions is malformed evidence, a
        // failure to report and not an arithmetic panic.
        ("did not complete after the release", |e| e.cancelled = 9),
        ("while its holder was parked", |e| e.live_with_holders = 3),
        ("did not register behind", |e| e.waited_sources = 3),
        ("cancelled waiter did not leave", |e| e.cancelled_left = 1),
        ("did not complete after the release", |e| e.completed = 5),
        ("outlived its last transition", |e| e.live_after = 1),
        ("outlived its last transition", |e| e.participants_after = 1),
        ("peak backing capacity", |e| e.capacity_after = 128),
    ];
    for (needle, broken) in cases {
        let mut e = sound.clone();
        broken(&mut e);
        let details = gate_details(&e);
        assert!(
            details.iter().any(|d| d.contains(needle)),
            "{needle}: {details:?}"
        );
    }
}

/// A single demand whose answer was never materialised is not an answer: the
/// verdict names the action, however the rest of the run reads.
#[test]
fn single_demand_without_a_materialised_answer_fails() {
    let (mut summary, failures) = smoke(Mode::Normal);
    assert!(failures.is_empty(), "{failures:#?}");
    let index = summary
        .records
        .iter()
        .find(|r| r.class == LifecycleClass::ColdDemand)
        .expect("a cold demand")
        .index;
    for broken in [vec![], vec!["unmaterialised".to_string()]] {
        summary.records[index as usize].digests = broken;
        let failures = verify_summary(Mode::Normal, 1, &summary);
        assert!(
            failures.iter().any(|f| matches!(
                f,
                SoakFailure::WrongOutcome { index: i, .. } if *i == index
            )),
            "{failures:#?}"
        );
    }
}
