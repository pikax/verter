//! The soak runner proved on any worker: the smoke manifest (one tranche
//! per mode) passes, the full manifests match their frozen digests, and every
//! planted defect fails the run for its own reason and no other.
//!
//! The retention account is process-wide, so runs in one process are
//! serialised: a concurrent run's charges would move another run's drain.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use verter_audit::soak::SoakCounts;
use verter_bench::soak::controls::{self, Control};
use verter_bench::soak::gates::{self, GateEvidence};
use verter_bench::soak::isolation::{Answer, AnswerClass, IsolationEvidence};
use verter_bench::soak::manifest::{
    surge_limits_of, Action, LifecycleClass, Manifest, Mode, SurgeLimits, Target,
    BOUNDED_SURGE_LIMITS, BOUNDED_SURGE_MODULES, FULL_SURGE_LIMITS, FULL_TRANCHES, SURGE_MODULES,
    SURGE_PERIOD,
};
use verter_bench::soak::refusal::{Demand, DemandRole, RefusalEvidence, RefusalVariant};
use verter_bench::soak::run::{
    run, ActionRecord, ActiveEvidence, Charges, Outcome, OwnerStep, OwnerStepKind,
    PressureEvidence, RunOptions, Sample, SoakSummary, SUMMARY_FILE,
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
        LifecycleClass::required(mode).into_iter().collect(),
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
        host.release_completion()
            .wait()
            .expect("the parse workers apply the close's releases");
        let closed = counts(&host);
        let leaks = verter_audit::soak::drain_violations(
            &baseline,
            &closed,
            verter_audit::soak::DrainPoint::Closed,
        );
        assert!(leaks.is_empty(), "close left {leaks:#?}");
    }
}

/// The registered evidence-run manifest.
fn registration() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/benchmark/evidence-runs/rev11-l1-soak.json");
    let text = std::fs::read_to_string(&path).expect("the registered manifest");
    serde_json::from_str(&text).expect("JSON")
}

/// The registered manifest freezes both full workloads by digest, the
/// runner's sampling cadence and envelope, and the support-feature build the
/// runner executes on; a change to the generator, the template or any
/// published fixture moves the digests.
#[test]
fn full_manifests_match_the_frozen_registration() {
    use verter_bench::soak::run::SAMPLE_EVERY;
    use verter_bench::soak::verify::ENVELOPE_ACTIONS;
    let registered = registration();
    let soak = &registered["soak"];
    assert_eq!(soak["tranches"], FULL_TRANCHES);
    assert_eq!(soak["sampleEvery"], SAMPLE_EVERY);
    assert_eq!(soak["envelopeActions"], ENVELOPE_ACTIONS);
    assert_eq!(
        soak["build"],
        serde_json::json!({
            "package": "verter_bench",
            "bin": "verter-soak",
            "profile": "release",
            "features": ["soak-support"],
        })
    );
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
        let classes: BTreeSet<_> = manifest.actions.iter().map(|a| a.class).collect();
        assert_eq!(
            classes,
            LifecycleClass::required(mode).into_iter().collect(),
            "{mode:?}: the full workload drives every lifecycle class"
        );
        assert_eq!(
            Manifest::generate(mode, 1).digest(),
            soak["smoke"][mode.as_str()]["digest"]
                .as_str()
                .unwrap_or_default(),
            "{mode:?} smoke manifest digest"
        );
    }
    // The separately declared surge bounds are frozen in the registration
    // and ride in every workload digest above: each shape's request, its
    // populations' absolute maxima and its finite retained bound.
    for (name, frozen) in [
        ("bounded", BOUNDED_SURGE_LIMITS),
        ("full", FULL_SURGE_LIMITS),
    ] {
        let shape: SurgeLimits = serde_json::from_value(soak["surgeLimits"][name].clone())
            .expect("the registered surge shape");
        assert_eq!(shape, frozen, "{name} surge limits");
    }
    // The shapes' input byte maxima are what the frozen fixtures publish:
    // a fixture that grows a component past its registered bound fails
    // here, not only in a run.
    let manifest = Manifest::generate(Mode::Pressure, FULL_TRANCHES);
    let mut input_bytes = BTreeMap::new();
    for action in &manifest.actions {
        let Target::Surge {
            modules,
            props,
            oversized_props,
        } = &action.target
        else {
            continue;
        };
        for ordinal in 0..*modules {
            let count = if ordinal == 0 {
                *oversized_props
            } else {
                *props
            };
            let len = verter_bench::soak::fixtures::surge_source(action.tranche, ordinal, count)
                .len() as u64;
            input_bytes.insert(
                modules,
                len.max(input_bytes.get(modules).copied().unwrap_or(0)),
            );
        }
    }
    assert_eq!(
        input_bytes.get(&BOUNDED_SURGE_LIMITS.modules),
        Some(&BOUNDED_SURGE_LIMITS.input_bytes_max),
        "bounded surge input bytes"
    );
    assert_eq!(
        input_bytes.get(&FULL_SURGE_LIMITS.modules),
        Some(&FULL_SURGE_LIMITS.input_bytes_max),
        "full surge input bytes"
    );
}

/// The frozen identity covers every byte every action publishes: altering
/// the publication of any one action — in the first tranche, a middle one or
/// the last, a surge component, an intermediate refusal edit or the setup —
/// moves the digest, and every publishing action has publications to bind.
#[test]
fn the_manifest_digest_binds_every_published_fixture() {
    use verter_bench::soak::fixtures::{expected_publications, setup_publications};
    // Every template position, a bounded surge and a full surge.
    let manifest = Manifest::generate(Mode::Pressure, SURGE_PERIOD + 1);
    let frozen = manifest.digest();
    let setup = setup_publications();
    let published: Vec<Vec<String>> = manifest
        .actions
        .iter()
        .map(|a| expected_publications(Mode::Pressure, a))
        .collect();
    let altered = |index: u32| {
        manifest.digest_over(&setup, |action| {
            let mut out = published[action.index as usize].clone();
            if action.index == index {
                out.push("altered".into());
            }
            out
        })
    };
    let last = |pick: &dyn Fn(&Action) -> bool| {
        manifest
            .actions
            .iter()
            .rev()
            .find(|a| pick(a))
            .expect("the manifest has such an action")
            .index
    };
    let picks: [(&str, u32); 6] = [
        ("the first action", 0),
        ("a middle tranche's edit", {
            manifest
                .actions
                .iter()
                .find(|a| a.tranche == SURGE_PERIOD / 2 && a.class == LifecycleClass::Edit)
                .expect("an edit")
                .index
        }),
        (
            "the last tranche's churn",
            last(&|a| a.class == LifecycleClass::CandidateChurn),
        ),
        (
            "the last full surge",
            last(
                &|a| matches!(a.target, Target::Surge { modules, .. } if modules == SURGE_MODULES),
            ),
        ),
        (
            "the last depth edit",
            last(&|a| {
                matches!(
                    a.target,
                    Target::Refusal {
                        variant: RefusalVariant::DepthEdited,
                        ..
                    }
                )
            }),
        ),
        (
            "the last configuration restore",
            last(&|a| a.class == LifecycleClass::ConfigRestore),
        ),
    ];
    for (what, index) in picks {
        assert_ne!(altered(index), frozen, "{what} is not bound by the digest");
    }
    let mut setup_altered = setup.clone();
    setup_altered[0].push('x');
    assert_ne!(
        manifest.digest_over(&setup_altered, |a| published[a.index as usize].clone()),
        frozen,
        "the setup is not bound by the digest"
    );
    for mode in Mode::ALL {
        let manifest = Manifest::generate(mode, SURGE_PERIOD + 1);
        for action in &manifest.actions {
            let publishes = matches!(
                action.class,
                LifecycleClass::Open
                    | LifecycleClass::Edit
                    | LifecycleClass::RevertSource
                    | LifecycleClass::ReleaseCopyThenOriginal
                    | LifecycleClass::ReleaseOriginalThenCopy
                    | LifecycleClass::ConfigChange
                    | LifecycleClass::ConfigRestore
                    | LifecycleClass::UniqueOwnerChurn
                    | LifecycleClass::CandidateChurn
                    | LifecycleClass::PressureSurge
                    | LifecycleClass::MetaIsolation
                    | LifecycleClass::RefusalRepeat
                    | LifecycleClass::ProfileHealing
            );
            assert_eq!(
                !expected_publications(mode, action).is_empty(),
                publishes,
                "{mode:?} action {} ({:?})",
                action.index,
                action.class
            );
        }
    }
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

#[test]
fn control_unfrozen_fixture_fails_the_publication_check() {
    assert_control_bites(&control("unfrozen-fixture"));
}

/// Every control the runner ships is exercised by a test above, and the
/// registration names exactly the shipped controls.
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
            "unfrozen-fixture",
        ])
    );
    let registered: BTreeSet<String> = registration()["soak"]["controls"]["names"]
        .as_array()
        .expect("registered control names")
        .iter()
        .map(|n| n.as_str().expect("a name").to_string())
        .collect();
    assert_eq!(
        registered,
        names
            .iter()
            .map(|n| (*n).to_string())
            .collect::<BTreeSet<_>>()
    );
}

fn answer(digest: &str, class: AnswerClass) -> Answer {
    Answer {
        class,
        digest: digest.into(),
    }
}

fn evidence() -> IsolationEvidence {
    IsolationEvidence {
        callback_ran: true,
        ops_before: 36,
        same_budget: true,
        spent: answer("spent", AnswerClass::OperationProfile),
        isolated: answer("isolated", AnswerClass::Complete),
        repeat: answer("isolated", AnswerClass::Complete),
        spent_recomputes: 1,
        isolated_recomputes: 1,
        repeat_recomputes: 0,
        repeat_producer_runs: 0,
        other_context: answer("spent", AnswerClass::OperationProfile),
        other_context_recomputes: 1,
        oracle_other_context: answer("spent", AnswerClass::OperationProfile),
        oracle_spent: answer("spent", AnswerClass::OperationProfile),
        oracle_isolated: answer("isolated", AnswerClass::Complete),
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
    let cases: [(&str, Break); 17] = [
        // A different paid context must compute, match its own cold oracle
        // and be a refusal, not reuse the first context's retained refusal
        // or a complete answer.
        ("without computing", |e| e.other_context_recomputes = 0),
        ("other paid context's answer differs", |e| {
            e.other_context = answer("spent", AnswerClass::OperationProfile);
            e.oracle_other_context = answer("other", AnswerClass::OperationProfile);
        }),
        ("other paid context did not deliver", |e| {
            e.other_context = answer("isolated", AnswerClass::Complete);
            e.oracle_other_context = e.other_context.clone();
        }),
        ("never ran", |e| e.callback_ran = false),
        ("spent no operations", |e| e.ops_before = 0),
        ("batch's budget", |e| e.same_budget = false),
        ("operation-profile refusal", |e| {
            e.spent = answer("spent", AnswerClass::Complete);
            e.oracle_spent = e.spent.clone();
        }),
        ("operation-profile refusal", |e| {
            e.spent = Answer::absent();
            e.oracle_spent = Answer::absent();
        }),
        // A generic incomplete answer is not the typed refusal.
        ("operation-profile refusal", |e| {
            e.spent = answer("spent", AnswerClass::OtherIncomplete);
            e.oracle_spent = e.spent.clone();
        }),
        ("operation-profile refusal", |e| {
            e.spent = answer("spent", AnswerClass::ConnectedDepth);
            e.oracle_spent = e.spent.clone();
        }),
        ("complete answer", |e| {
            e.isolated = Answer::absent();
            e.oracle_isolated = Answer::absent();
            e.repeat = Answer::absent();
        }),
        ("spent-context answer differs", |e| {
            e.oracle_spent = answer("other", AnswerClass::OperationProfile)
        }),
        ("fresh-request answer differs", |e| {
            e.oracle_isolated = answer("other", AnswerClass::Complete)
        }),
        ("repeat differs", |e| {
            e.repeat = answer("other", AnswerClass::Complete)
        }),
        ("answered the fresh request's answer", |e| {
            e.spent = e.isolated.clone()
        }),
        ("answered without computing", |e| e.spent_recomputes = 0),
        ("retained result", |e| e.repeat_producer_runs = 1),
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

fn demand(role: DemandRole, answer: Answer, work: u64) -> Demand {
    Demand {
        role,
        answer,
        recomputes: work,
        producer_runs: work,
    }
}

/// A sound record of each refusal variant.
fn refusal(variant: RefusalVariant) -> RefusalEvidence {
    use AnswerClass::{Complete, ConnectedDepth, MissingInput, OperationProfile};
    use DemandRole::*;
    let sealed = answer("sealed", OperationProfile);
    let deep = answer("deep", ConnectedDepth);
    let sibling = answer("sibling", Complete);
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
                let refused = answer(&format!("edit{edit}"), OperationProfile);
                out.push(demand(Edit, refused.clone(), 3));
                out.push(demand(EditRepeat, refused, 0));
            }
            out
        }
        RefusalVariant::Healing => vec![
            demand(Sealed, sealed.clone(), 3),
            demand(Healed, answer("healed", Complete), 5),
            demand(TightAfterHealed, answer("healed", Complete), 0),
        ],
        RefusalVariant::Concurrent => vec![
            demand(Sealed, sealed.clone(), 3),
            demand(ConcurrentTight, sealed.clone(), 0),
            demand(ConcurrentLarge, answer("healed", Complete), 0),
            demand(ConcurrentTight, sealed, 0),
            demand(ConcurrentLarge, answer("healed", Complete), 0),
        ],
        RefusalVariant::DepthSealed => vec![
            demand(Sealed, deep.clone(), 1),
            demand(Repeat, deep.clone(), 0),
            demand(Repeat, deep.clone(), 0),
            demand(Sibling, sibling, 1),
            demand(Oracle, deep, 1),
        ],
        RefusalVariant::DepthEdited => vec![
            demand(Sealed, deep.clone(), 1),
            demand(Edit, deep.clone(), 1),
            demand(EditRepeat, deep, 0),
            demand(ShallowHealed, answer("shallow", Complete), 1),
            demand(HealedRepeat, answer("shallow", Complete), 0),
        ],
        RefusalVariant::DepthControls => vec![
            demand(Sealed, deep.clone(), 1),
            demand(NotIsolated, deep.clone(), 1),
            demand(NotIsolated, deep.clone(), 1),
            demand(OtherProfile, deep.clone(), 1),
            demand(Sibling, sibling, 1),
            demand(Repeat, deep, 0),
        ],
        RefusalVariant::DepthMissingInput => vec![
            demand(Missing, answer("mixed", MissingInput), 1),
            demand(Missing, answer("mixed", MissingInput), 1),
            demand(Supplied, deep.clone(), 1),
            demand(SuppliedRepeat, deep, 0),
        ],
        RefusalVariant::MissingInput => vec![
            demand(Missing, answer("missing", MissingInput), 1),
            demand(Missing, answer("missing", MissingInput), 1),
            demand(Supplied, answer("supplied", Complete), 1),
            demand(SuppliedRepeat, answer("supplied", Complete), 0),
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

fn broken(variant: RefusalVariant, plant: impl FnOnce(&mut RefusalEvidence)) -> Vec<String> {
    let mut evidence = refusal(variant);
    plant(&mut evidence);
    refusal_details(&evidence)
}

#[test]
fn refusal_rules_fail_each_broken_property() {
    use AnswerClass::*;
    for variant in RefusalVariant::ALL {
        assert!(
            refusal_details(&refusal(variant)).is_empty(),
            "{variant:?} baseline: {:?}",
            refusal_details(&refusal(variant))
        );
    }
    let fails = |details: Vec<String>, what: &str| assert!(!details.is_empty(), "{what}");
    // An absent symbol or a generic incomplete answer never stands in for a
    // typed refusal, and neither class stands in for the other.
    for (variant, class) in [
        (RefusalVariant::Sealed, Absent),
        (RefusalVariant::Sealed, OtherIncomplete),
        (RefusalVariant::Sealed, ConnectedDepth),
        (RefusalVariant::Sealed, MissingInput),
        (RefusalVariant::DepthSealed, OperationProfile),
        (RefusalVariant::DepthSealed, OtherIncomplete),
        (RefusalVariant::DepthSealed, MissingInput),
    ] {
        fails(
            broken(variant, |e| {
                for d in &mut e.demands {
                    if d.answer.class != Complete {
                        d.answer.class = class;
                    }
                }
            }),
            &format!("{variant:?} sealed as {class:?}"),
        );
    }
    // A replay that recomputed, by either counter, is not a retained refusal.
    for variant in [RefusalVariant::Sealed, RefusalVariant::DepthSealed] {
        fails(
            broken(variant, |e| e.demands[1].recomputes = 2),
            "a replay that resolved again",
        );
        fails(
            broken(variant, |e| e.demands[1].producer_runs = 1),
            "a replay that ran a semantic producer",
        );
    }
    // A repeat that completes is not the refusal.
    fails(
        broken(RefusalVariant::Sealed, |e| {
            e.demands[2].answer = answer("sealed", Complete)
        }),
        "a completed repeat",
    );
    // A larger profile that does not heal, or replays the tight refusal.
    fails(
        broken(RefusalVariant::Healing, |e| {
            e.demands[1].answer = answer("sealed", OperationProfile)
        }),
        "an unhealed larger profile",
    );
    fails(
        broken(RefusalVariant::Healing, |e| {
            e.demands[1].answer = answer("sealed", OperationProfile);
            e.demands[1].recomputes = 0;
        }),
        "a larger profile replaying the tight refusal",
    );
    // An edit that served the old refusal.
    fails(
        broken(RefusalVariant::Edited, |e| {
            e.demands[1].answer = answer("sealed", OperationProfile)
        }),
        "a stale edit",
    );
    // Concurrent profile variants that disagree.
    fails(
        broken(RefusalVariant::Concurrent, |e| {
            e.demands[3].answer = answer("other", OperationProfile)
        }),
        "disagreeing concurrent profiles",
    );
    // A relevant edit that replayed the old depth refusal.
    fails(
        broken(RefusalVariant::DepthEdited, |e| e.demands[1].recomputes = 0),
        "an edit that did not invalidate the depth refusal",
    );
    fails(
        broken(RefusalVariant::DepthEdited, |e| {
            e.demands[3].answer = answer("deep", ConnectedDepth)
        }),
        "a shallow equivalent that did not heal",
    );
    // Ineligible requests that inherited the isolated refusal.
    for index in [1, 2, 3] {
        fails(
            broken(RefusalVariant::DepthControls, |e| {
                e.demands[index].recomputes = 0
            }),
            "an ineligible request replaying the refusal",
        );
    }
    fails(
        broken(RefusalVariant::DepthControls, |e| {
            e.demands[4].answer = answer("deep", ConnectedDepth)
        }),
        "a sibling inheriting the refusal",
    );
    // A missing input that was retained, or that does not heal.
    fails(
        broken(RefusalVariant::DepthMissingInput, |e| {
            e.demands[1].recomputes = 0
        }),
        "a retained missing input",
    );
    fails(
        broken(RefusalVariant::DepthMissingInput, |e| {
            e.demands[0].answer = answer("deep", ConnectedDepth)
        }),
        "a mixed cause delivered as the depth refusal",
    );
    fails(
        broken(RefusalVariant::MissingInput, |e| {
            e.demands[2].answer = answer("missing", MissingInput)
        }),
        "a missing input that did not heal",
    );
}

fn charges(retained: u64, pinned: u64) -> Charges {
    Charges {
        active: 0,
        retained,
        pinned,
        storage: 100,
    }
}

fn step(kind: OwnerStepKind, roots: u64, charges: Charges, projected: Option<&str>) -> OwnerStep {
    OwnerStep {
        step: kind,
        roots,
        charges,
        projected: projected.map(str::to_string),
    }
}

/// The allocation charge of one held answer at the magnitudes of a real
/// run: the readers' 384 bytes ride on about 126 KB of unrelated pins.
const HELD_ALLOCATION: u64 = 384;

fn release_steps() -> Vec<OwnerStep> {
    vec![
        step(
            OwnerStepKind::BeforeRelease,
            50,
            charges(900, 151_332),
            Some("held"),
        ),
        step(
            OwnerStepKind::AfterRetirement,
            5,
            charges(300, 126_500),
            Some("held"),
        ),
        step(
            OwnerStepKind::AfterFirstRelease,
            5,
            charges(300, 126_500),
            Some("held"),
        ),
        step(
            OwnerStepKind::AfterFinalRelease,
            4,
            charges(300, 126_116),
            None,
        ),
        step(
            OwnerStepKind::ProbeAcquired,
            5,
            charges(300, 126_500),
            Some("held"),
        ),
        step(OwnerStepKind::ProbeReleased, 4, charges(300, 126_116), None),
    ]
}

fn copy_steps() -> Vec<OwnerStep> {
    vec![
        step(
            OwnerStepKind::BeforeCopy,
            5,
            charges(300, 480),
            Some("held"),
        ),
        step(OwnerStepKind::AfterCopy, 5, charges(300, 480), Some("held")),
    ]
}

#[test]
fn ownership_rules_fail_early_destruction_wrong_charges_and_lost_release() {
    let held = "held".to_string();
    let copy = LifecycleClass::PublicCopy;
    for release in [
        LifecycleClass::ReleaseCopyThenOriginal,
        LifecycleClass::ReleaseOriginalThenCopy,
    ] {
        assert!(check_ownership(0, release, Some(&held), &release_steps()).is_empty());
        let fails = |plant: &dyn Fn(&mut Vec<OwnerStep>), what: &str| {
            let mut steps = release_steps();
            plant(&mut steps);
            assert!(
                !check_ownership(0, release, Some(&held), &steps).is_empty(),
                "{release:?}: {what}"
            );
        };
        fails(
            &|s| s[1].projected = Some("gone".into()),
            "retirement destroyed the answer",
        );
        fails(
            &|s| s[2].projected = Some("gone".into()),
            "the first release destroyed it",
        );
        fails(&|s| s[2].roots = 4, "the first release dropped a root");
        fails(&|s| s[3].roots = 5, "the final release gave no root back");
        // Correct roots with the wrong charges.
        fails(
            &|s| s[2].charges.pinned -= 64,
            "the first release freed the shared charge",
        );
        fails(
            &|s| s[2].charges.retained += 64,
            "the first release charged again",
        );
        fails(
            &|s| s[1].charges.retained = 1_000,
            "retirement grew the retained charge",
        );
        // The held allocation's own (content) charge: pinned bytes less the
        // stores' backing storage.
        fails(
            &|s| {
                for step in s.iter_mut().take(3) {
                    step.charges.pinned = step.charges.storage;
                }
            },
            "a live reader holds no charge",
        );
        fails(
            &|s| {
                s[1].charges.pinned = s[1].charges.storage;
                s[2].charges.pinned = s[2].charges.storage;
            },
            "retirement freed the charge the surviving readers hold",
        );
        fails(
            &|s| s[3].charges.pinned = s[2].charges.pinned,
            "the final release freed nothing",
        );
        fails(
            &|s| s[3].charges.pinned = s[2].charges.pinned + 64,
            "the final release grew the pinned charge",
        );
        fails(
            &|s| s[1].charges.pinned = s[0].charges.pinned + 64,
            "retirement grew the content charge",
        );
        fails(
            &|s| s[1].charges.storage = s[1].charges.pinned + 1,
            "storage past the pinned total is inconsistent accounting",
        );
        fails(
            &|s| s[3].charges.retained += 64,
            "the final release grew the retained charge",
        );
        fails(
            &|s| s[3].charges.active += 64,
            "the final release grew the active charge",
        );
        // Wrong amounts on the held allocation itself at real magnitudes:
        // the unrelated pins dwarf it, so only the exact give-back against
        // the transfer probe discriminates. Retirement re-charges the
        // readers at twice the allocation and the final release returns the
        // doubled charge.
        fails(
            &|s| {
                s[1].charges.pinned += HELD_ALLOCATION;
                s[2].charges.pinned += HELD_ALLOCATION;
                s[3].charges.pinned = s[2].charges.pinned - 2 * HELD_ALLOCATION;
            },
            "a doubled charge transfer through retirement",
        );
        // Retirement keeps only a fraction of the allocation and the final
        // release returns that fraction.
        fails(
            &|s| {
                s[1].charges.pinned -= HELD_ALLOCATION - 128;
                s[2].charges.pinned -= HELD_ALLOCATION - 128;
                s[3].charges.pinned = s[2].charges.pinned - 128;
            },
            "a partial charge transfer through retirement",
        );
        // The probe's own evidence is required and must measure the same
        // allocation the release returns.
        fails(&|s| s.truncate(4), "no transfer probe recorded");
        fails(
            &|s| s[5].charges.pinned = s[4].charges.pinned,
            "the transfer probe's release gave back nothing",
        );
        fails(
            &|s| s[5].charges.pinned = s[4].charges.pinned - 256,
            "the transfer probe measured a different allocation",
        );
    }
    assert!(check_ownership(0, copy, Some(&held), &copy_steps()).is_empty());
    let mut uncharged_copy = copy_steps();
    for step in &mut uncharged_copy {
        step.charges.pinned = step.charges.storage;
    }
    assert!(!check_ownership(0, copy, Some(&held), &uncharged_copy).is_empty());
    let mut doubled_root = copy_steps();
    doubled_root[1].roots = 6;
    assert!(!check_ownership(0, copy, Some(&held), &doubled_root).is_empty());
    // A copy with correct roots that charges a second time.
    let mut doubled_charge = copy_steps();
    doubled_charge[1].charges.retained += 64;
    assert!(!check_ownership(0, copy, Some(&held), &doubled_charge).is_empty());
}

fn active(attributed_peak: Option<u64>, peak: u64) -> ActiveEvidence {
    ActiveEvidence {
        active_at_open: 0,
        active_at_close: 0,
        peak_active_bytes: peak,
        transitions: 2,
        attributed_peak,
        rejection: attributed_peak
            .is_none()
            .then(|| "PreWindowCharge { transitions: 1 }".into()),
    }
}

fn surge(
    modules: u32,
    pressure: Option<u64>,
    oversized: Option<u64>,
    outcome: Outcome,
) -> ActionRecord {
    // The surge's populations sit exactly at their registered maxima: one
    // live artifact, source and lease per component.
    let shape = surge_limits_of(modules).expect("a registered surge shape");
    ActionRecord {
        outcome,
        digests: (0..modules).map(|i| format!("d{i}")).collect(),
        refusals_pressure: pressure,
        refusals_oversized: oversized,
        pressure: Some(PressureEvidence {
            memo_candidates_before: 0,
            memo_candidates_after: 0,
            memo_candidate_trace: Vec::new(),
            refusals_active: 0,
            retained_results: Some(u64::from(modules) / 2),
            surge_counts: Some(SoakCounts {
                live_artifacts: shape.live_artifacts_max,
                registered_sources: shape.registered_sources_max,
                snapshot_leases: shape.snapshot_leases_max,
                ..SoakCounts::default()
            }),
            active: Some(active(
                Some(FORMER_ACTIVE_TARGET_BYTES),
                FORMER_ACTIVE_TARGET_BYTES,
            )),
        }),
        ..ActionRecord::new(0, LifecycleClass::PressureSurge)
    }
}

/// The surge actions of a full pressure manifest, re-indexed in order.
fn surge_manifest() -> Vec<Action> {
    Manifest::generate(Mode::Pressure, SURGE_PERIOD)
        .actions
        .into_iter()
        .filter(|a| matches!(a.target, Target::Surge { .. }))
        .enumerate()
        .map(|(i, mut a)| {
            a.index = i as u32;
            a
        })
        .collect()
}

fn indexed(records: Vec<ActionRecord>) -> Vec<ActionRecord> {
    records
        .into_iter()
        .enumerate()
        .map(|(i, r)| ActionRecord {
            index: i as u32,
            ..r
        })
        .collect()
}

fn sound_surges() -> Vec<ActionRecord> {
    indexed(vec![
        surge(BOUNDED_SURGE_MODULES, Some(0), Some(1), Outcome::Value),
        surge(SURGE_MODULES, Some(3), Some(1), Outcome::Value),
    ])
}

fn pressure_details(records: &[ActionRecord]) -> Vec<String> {
    pressure_details_over(&surge_manifest(), records)
}

fn pressure_details_over(expected: &[Action], records: &[ActionRecord]) -> Vec<String> {
    let mut failures = Vec::new();
    check_pressure_outcomes(expected, records, &mut failures);
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
    assert_eq!(
        surge_manifest()
            .iter()
            .map(|a| match a.target {
                Target::Surge { modules, .. } => modules,
                _ => 0,
            })
            .collect::<Vec<_>>(),
        [BOUNDED_SURGE_MODULES, SURGE_MODULES],
        "the fixture mirrors the manifest's bounded and full surges"
    );
    assert_eq!(pressure_details(&sound_surges()), Vec::<String>::new());
    let full = |plant: fn(&mut ActionRecord)| {
        let mut records = sound_surges();
        plant(&mut records[1]);
        records
    };
    let bounded = |plant: fn(&mut ActionRecord)| {
        let mut records = sound_surges();
        plant(&mut records[0]);
        records
    };
    let cases = [
        // The surge host's own populations are judged while its results are
        // live: more live artifacts than requested components, a byte figure
        // over its absolute limit, or no recorded populations fail.
        (
            full(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .live_artifacts = u64::from(SURGE_MODULES) + 1
            }),
            "exceeded the absolute limits",
        ),
        (
            full(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .pinned_bytes = 1 << 40
            }),
            "exceeded the absolute limits",
        ),
        (
            full(|r| r.pressure.as_mut().unwrap().surge_counts = None),
            "no live populations",
        ),
        (vec![], "ran no surge"),
        (sound_surges()[..1].to_vec(), "full surges were recorded"),
        (
            full(|r| r.refusals_pressure = Some(0)),
            "aggregate pressure",
        ),
        (full(|r| r.refusals_pressure = None), "aggregate pressure"),
        (full(|r| r.refusals_oversized = Some(0)), "oversized"),
        (full(|r| r.refusals_oversized = None), "oversized"),
        (full(|r| r.outcome = Outcome::Miss), "complete"),
        (
            full(|r| r.pressure.as_mut().unwrap().retained_results = Some(SURGE_MODULES as u64)),
            "uncached",
        ),
        (
            full(|r| r.pressure.as_mut().unwrap().refusals_active = 1),
            "refused active work",
        ),
        // The registered figures are bounded by the shape's frozen maxima,
        // never by what the request asks for.
        (
            full(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .registered_sources = FULL_SURGE_LIMITS.registered_sources_max + 1
            }),
            "exceeded the absolute limits",
        ),
        (
            full(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .snapshot_leases = FULL_SURGE_LIMITS.snapshot_leases_max + 1
            }),
            "exceeded the absolute limits",
        ),
        // The retained-byte bound is finite and explicit: the surge fills
        // the account up to the ratified ceiling and not one byte past it.
        (
            full(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .retained_bytes = FULL_SURGE_LIMITS.retained_bytes_max + 1
            }),
            "exceeded the absolute limits",
        ),
        // The bounded surge's retained bytes stay under the ordinary
        // workload bound: its smaller shape earns no surge-wide exemption.
        (
            bounded(|r| {
                r.pressure
                    .as_mut()
                    .unwrap()
                    .surge_counts
                    .as_mut()
                    .unwrap()
                    .retained_bytes = BOUNDED_SURGE_LIMITS.retained_bytes_max + 1
            }),
            "exceeded the absolute limits",
        ),
    ];
    for (records, needle) in cases {
        let details = pressure_details(&records);
        assert!(
            details.iter().any(|d| d.contains(needle)),
            "{needle}: {details:?}"
        );
    }
    // At the registered maxima, the ratified ceiling included, the surge
    // passes: the bounds are the shape's own, not a tighter guess.
    assert_eq!(
        pressure_details(&full(|r| {
            r.pressure
                .as_mut()
                .unwrap()
                .surge_counts
                .as_mut()
                .unwrap()
                .retained_bytes = FULL_SURGE_LIMITS.retained_bytes_max
        })),
        Vec::<String>::new()
    );
    // A request no registered shape covers has no allowance at all: one
    // module past the frozen size, or a frozen size asked for with other
    // props, fails however many answers it delivers — increasing a request
    // never increases the maxima it is judged by.
    let reprops = |mutate: &dyn Fn(&mut u32, &mut u32, &mut u32)| {
        let mut expected = surge_manifest();
        for action in &mut expected {
            if let Target::Surge {
                modules,
                props,
                oversized_props,
            } = &mut action.target
            {
                if *modules == SURGE_MODULES {
                    mutate(modules, props, oversized_props);
                }
            }
        }
        expected
    };
    let unregistered = reprops(&|modules, _, _| *modules += 1);
    let mut grown = sound_surges();
    grown[1].digests.push("one more".into());
    let details = pressure_details_over(&unregistered, &grown);
    assert!(
        details
            .iter()
            .any(|d| d.contains("no registered surge shape")),
        "{details:?}"
    );
    let wrong_props = reprops(&|_, props, _| *props += 1);
    let details = pressure_details_over(&wrong_props, &sound_surges());
    assert!(
        details
            .iter()
            .any(|d| d.contains("no registered surge shape")),
        "{details:?}"
    );
    // A full surge whose own window stayed below the former active target
    // fails however large its total or pre-window peak was; a window that
    // saw foreign work attributes nothing; a missing window fails.
    let below = full(|r| {
        r.pressure.as_mut().unwrap().active = Some(active(Some(1), u64::MAX));
    });
    assert!(pressure_details(&below)
        .iter()
        .any(|d| d.contains("former active target")));
    let foreign = full(|r| {
        r.pressure.as_mut().unwrap().active = Some(active(None, FORMER_ACTIVE_TARGET_BYTES * 2));
    });
    assert!(pressure_details(&foreign)
        .iter()
        .any(|d| d.contains("no attributable ACTIVE window")));
    let unwatched = full(|r| r.pressure.as_mut().unwrap().active = None);
    assert!(!pressure_details(&unwatched).is_empty());
    // A dropped or undelivered surge answer fails.
    let mut dropped = sound_surges();
    dropped[1].digests.pop();
    assert!(!pressure_details(&dropped).is_empty());
    let mut undelivered = sound_surges();
    undelivered[1].digests[7] = ABSENT_DELIVERY.into();
    assert!(!pressure_details(&undelivered).is_empty());
    // A bounded surge is held to delivery and its window, not to the full
    // outcomes.
    let mut bounded_unwatched = sound_surges();
    bounded_unwatched[0].pressure.as_mut().unwrap().active = None;
    assert!(!pressure_details(&bounded_unwatched).is_empty());
}

/// Every pressure manifest drives a bounded surge in its first tranche (the
/// smoke runs every pressure action class); the full surges are frozen in
/// the full pressure manifest only.
#[test]
fn surge_tranches_are_frozen_by_size() {
    let surges = |mode, tranches| {
        Manifest::generate(mode, tranches)
            .actions
            .into_iter()
            .filter_map(|a| match a.target {
                Target::Surge { modules, .. } => Some((a.tranche, modules)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(surges(Mode::Pressure, 1), [(0, BOUNDED_SURGE_MODULES)]);
    assert_eq!(
        surges(Mode::Normal, FULL_TRANCHES),
        Vec::<(u32, u32)>::new()
    );
    let full = surges(Mode::Pressure, FULL_TRANCHES);
    let expected: Vec<(u32, u32)> = (0..FULL_TRANCHES)
        .filter_map(|t| {
            if t % SURGE_PERIOD == SURGE_PERIOD - 1 {
                Some((t, SURGE_MODULES))
            } else if t.is_multiple_of(SURGE_PERIOD) {
                Some((t, BOUNDED_SURGE_MODULES))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(full, expected);
    assert!(LifecycleClass::required(Mode::Pressure).contains(&LifecycleClass::PressureSurge));
    assert!(!LifecycleClass::required(Mode::Normal).contains(&LifecycleClass::PressureSurge));
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

fn record_of(s: &mut SoakSummary, class: LifecycleClass) -> &mut ActionRecord {
    s.records
        .iter_mut()
        .find(|r| r.class == class)
        .expect("a record of the class")
}

/// The judgments that read a finished run's summary fail closed on a doctored
/// copy of a real run: a churn that admitted nothing is no eviction, a retry
/// without a materialised answer is no answer, a timeline missing its
/// endpoints or opening samples is no timeline, and a retained-byte figure
/// over the workload bound fails.
#[test]
fn summary_judgments_fail_closed_on_doctored_evidence() {
    let (summary, failures) = smoke(Mode::Pressure);
    assert!(failures.is_empty(), "{failures:#?}");
    let judged = |edit: &dyn Fn(&mut SoakSummary)| {
        let mut doctored = summary.clone();
        edit(&mut doctored);
        verify_summary(Mode::Pressure, 1, &doctored)
    };
    let wrong_with = |failures: &[SoakFailure], needle: &str| {
        failures.iter().any(|f| match f {
            SoakFailure::WrongOutcome { detail, .. } => detail.contains(needle),
            _ => false,
        })
    };
    // A churn whose candidate count never grows admitted nothing; one that
    // never stops growing displaced nothing.
    for trace in [vec![3; 25], (0..25).collect::<Vec<u64>>()] {
        let failures = judged(&|s| {
            record_of(s, LifecycleClass::CandidateChurn)
                .pressure
                .as_mut()
                .unwrap()
                .memo_candidate_trace = trace.clone();
        });
        assert!(
            wrong_with(&failures, "displaced no admitted candidate"),
            "{trace:?}: {failures:#?}"
        );
    }
    for digest in [None, Some("unmaterialised".to_string())] {
        let failures = judged(&|s| {
            record_of(s, LifecycleClass::Cancellation).retry_digest = digest.clone();
        });
        assert!(
            wrong_with(&failures, "no materialised answer"),
            "{digest:?}: {failures:#?}"
        );
    }
    let gap = |failures: Vec<SoakFailure>| {
        failures
            .iter()
            .any(|f| matches!(f, SoakFailure::SamplingGap { .. }))
    };
    let total = summary.records.len() as u32;
    assert!(gap(judged(&|s| {
        for sample in &mut s.samples {
            sample.actions_done = 0;
        }
    })));
    assert!(gap(judged(&|s| {
        for sample in &mut s.samples {
            if sample.at == "action" {
                sample.at = "renamed".into();
            }
        }
    })));
    assert!(gap(judged(&|s| {
        s.samples
            .retain(|x| !(x.at == "action" && x.actions_done == 25));
    })));
    assert!(gap(judged(&|s| {
        s.samples.last_mut().unwrap().actions_done = total - 1;
    })));
    let over = judged(&|s| s.samples[2].counts.retained_bytes = 100 * 1024 * 1024);
    assert!(over.iter().any(|f| matches!(
        f,
        SoakFailure::AbsoluteLimitExceeded { violations, .. }
            if violations.iter().any(|v| v.figure == "retained_bytes")
    )));
}

/// The frozen oversized component is one retained result above the account's
/// per-entry cap: on a fresh public host it is refused retention as oversized
/// (not for aggregate pressure) and is still delivered complete.
#[test]
fn the_oversized_surge_component_exceeds_the_entry_cap_alone() {
    use std::sync::Arc;
    use verter_bench::soak::fixtures::surge_source;
    use verter_bench::soak::manifest::OVERSIZED_PROPS;
    use verter_session::{HostConfig, UpsertRequest, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};
    let _serial = serial();
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let before = host.retention_snapshot();
    let id = "/w/oversized.vue".to_string();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.clone()),
            input_id: id.clone(),
            source: Arc::from(surge_source(0, 0, OVERSIZED_PROPS).as_str()),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(&id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("an upsert");
    let delivery = host.get_component_meta(&id);
    let after = host.retention_snapshot();
    assert!(delivery.as_ref().is_some_and(|d| d.is_complete()));
    assert!(after.refusals_oversized > before.refusals_oversized);
    assert_eq!(after.refusals_pressure, before.refusals_pressure);
    drop(delivery);
    host.close();
    host.release_completion().wait().expect("release boundary");
}

/// Two deliveries that keep every prop name and cause but change one prop's
/// type differ in their digest; the answer class alone would not tell them
/// apart.
#[test]
fn delivery_digest_binds_recovery_content_not_just_names() {
    use verter_bench::soak::isolation::Rig;
    let _serial = serial();
    let ask = |ty: &str| {
        let rig = Rig::new(0);
        rig.publish(
            0,
            &[(
                "c.vue",
                format!("<script setup lang=\"ts\">\ndefineProps<{{ a: {ty} }}>()\n</script>\n"),
            )],
        );
        rig.demand_of(0, "c.vue")
    };
    let (string, number) = (ask("string"), ask("number"));
    assert_eq!(string.class, number.class);
    assert_ne!(string.digest, number.digest);
    assert_eq!(string, ask("string"));
}
