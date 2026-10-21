//! Judge a soak run from the summary it wrote, against an independently
//! regenerated manifest.
//!
//! The verifier never trusts the runner's own view of the workload: it reads
//! the summary back from disk, regenerates the expected manifest from the
//! summary's `(mode, tranches)`, and checks the action and result inventories,
//! the per-class answer rules, the sampling cadence, the opening-window
//! envelope and the final drain against the regenerated workload.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use verter_audit::soak::{
    drain_violations, envelope_of, envelope_violations, CountViolation, DrainPoint,
};

use super::gates::GateEvidence;
use super::isolation::{Answer, AnswerClass, IsolationEvidence};
use super::manifest::{
    surge_limits_of, Action, LifecycleClass, Manifest, Mode, SurgeLimits, Target, SURGE_MODULES,
};
use super::refusal::{Demand, DemandRole, RefusalEvidence, RefusalVariant};
use super::run::{
    ActionRecord, Outcome, OwnerStep, OwnerStepKind, SoakSummary, SAMPLE_EVERY, SUMMARY_FILE,
};
use crate::semantic_perf::disk;

/// Samples taken within the first this-many actions set the envelope later
/// samples are held to.
pub const ENVELOPE_ACTIONS: u32 = 2_000;

/// Why a run fails, one variant per independently checked property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum SoakFailure {
    /// No readable summary in the output directory.
    MissingSummary { detail: String },
    /// The summary is for a different workload than the one requested.
    WorkloadMismatch { detail: String },
    /// The executed actions differ from the regenerated manifest.
    InventoryMismatch { detail: String },
    /// An action (or the setup, `index: None`) published other bytes, or
    /// other paths, than the regenerated manifest freezes for it.
    PublicationMismatch { index: Option<u32>, detail: String },
    /// A class the mode requires never ran.
    MissingClass { class: LifecycleClass },
    /// An action was recorded without being executed.
    UnexpectedSkip { index: u32 },
    /// An action's outcome breaks its class's rule.
    WrongOutcome { index: u32, detail: String },
    /// A cancellation request was not answered cancelled.
    CancellationNotObserved { index: u32 },
    /// An overlapping demand's requests were not in flight together, or the
    /// host recorded no join onto an in-flight computation.
    FakeOverlap {
        index: u32,
        max_in_flight: u32,
        joined_waits: u64,
        producer_claims: u32,
        subscribers: u32,
    },
    /// The restored configuration does not answer the original sentinel.
    WrongProjectReversion { index: u32 },
    /// Two samples further apart than the sampling cadence allows.
    SamplingGap { from: u32, to: u32 },
    /// A sample after the opening window above its envelope.
    EnvelopeExceeded {
        actions_done: u32,
        violations: Vec<CountViolation>,
    },
    /// The full pressure workload did not drive an admission branch it
    /// freezes, or delivered an incomplete answer under refusal.
    PressureNotExercised { detail: String },
    /// A component-meta isolation action broke an isolation rule.
    IsolationBroken { index: u32, detail: String },
    /// A refusal or healing action broke a refusal rule.
    RefusalBroken { index: u32, detail: String },
    /// A copy or release action broke an ownership rule.
    OwnershipBroken { index: u32, detail: String },
    /// A sample above an absolute workload limit.
    AbsoluteLimitExceeded {
        actions_done: u32,
        violations: Vec<CountViolation>,
    },
    /// A drained figure above its baseline after teardown.
    RetainedLeak { violations: Vec<CountViolation> },
}

/// Read the summary `out_dir` holds and judge it as the `(mode, tranches)`
/// workload. An empty result is a pass.
#[must_use]
pub fn verify(mode: Mode, tranches: u32, out_dir: &Path) -> Vec<SoakFailure> {
    let path = out_dir.join(SUMMARY_FILE);
    let summary: SoakSummary = match disk::read_to_string(&path)
        .map_err(|err| err.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))
    {
        Ok(summary) => summary,
        Err(detail) => {
            return vec![SoakFailure::MissingSummary {
                detail: format!("{}: {detail}", path.display()),
            }]
        }
    };
    verify_summary(mode, tranches, &summary)
}

/// Judge `summary` as the `(mode, tranches)` workload.
#[must_use]
pub fn verify_summary(mode: Mode, tranches: u32, summary: &SoakSummary) -> Vec<SoakFailure> {
    let mut failures = Vec::new();
    let expected = Manifest::generate(mode, tranches);
    if summary.mode != mode || summary.tranches != tranches {
        failures.push(SoakFailure::WorkloadMismatch {
            detail: format!(
                "requested {} x{tranches}, summary is {} x{}",
                mode.as_str(),
                summary.mode.as_str(),
                summary.tranches
            ),
        });
        return failures;
    }
    if summary.manifest_digest != expected.digest() {
        failures.push(SoakFailure::WorkloadMismatch {
            detail: format!(
                "manifest digest {} is not the regenerated {}",
                summary.manifest_digest,
                expected.digest()
            ),
        });
    }
    check_inventory(&expected, summary, &mut failures);
    check_publications(&expected, summary, &mut failures);
    check_outcomes(&expected, summary, &mut failures);
    check_samples(summary, &mut failures);
    if mode == Mode::Pressure {
        check_pressure_outcomes(&expected.actions, &summary.records, &mut failures);
    }
    failures
}

/// The isolation rules of one component-meta batch isolation action: the
/// callback ran with operations already spent under the batch's own budget;
/// its demand delivered the budget refusal a cold computation under the same
/// spent context delivers, which is not the complete answer a fresh request
/// gets; the fresh request answers complete and repeats from its retained
/// result; both match their own cold oracles. An error or an absent delivery
/// is never an answer of either class.
pub fn check_isolation(index: u32, e: &IsolationEvidence) -> Vec<SoakFailure> {
    let mut out = Vec::new();
    let mut check = |holds: bool, detail: &str| {
        if !holds {
            out.push(SoakFailure::IsolationBroken {
                index,
                detail: detail.into(),
            });
        }
    };
    check(e.callback_ran, "the batch callback never ran");
    check(e.ops_before > 0, "the batch had spent no operations");
    check(
        e.same_budget,
        "the callback's demand did not run under the batch's budget",
    );
    check(
        e.spent.class == AnswerClass::OperationProfile,
        "the spent context did not deliver the operation-profile refusal",
    );
    check(
        e.isolated.is_complete(),
        "the fresh request did not deliver a complete answer",
    );
    check(
        e.spent == e.oracle_spent,
        "the spent-context answer differs from its cold oracle",
    );
    check(
        e.isolated == e.oracle_isolated,
        "the fresh-request answer differs from its cold oracle",
    );
    check(
        e.repeat == e.isolated,
        "the fresh-request repeat differs from its first answer",
    );
    check(
        e.spent != e.isolated,
        "the spent context answered the fresh request's answer",
    );
    check(
        e.spent_recomputes > 0 && e.isolated_recomputes > 0,
        "a cold demand was answered without computing",
    );
    check(
        e.repeat_recomputes == 0 && e.repeat_producer_runs == 0,
        "the fresh-request repeat was not served from the retained result",
    );
    // Same root, profile and entry allowance under a different paid set: the
    // refusal sealed in the first spent context must not be replayed.
    check(
        e.other_context_ops == e.ops_before && e.other_context_ops > 0,
        "the other paid context did not preserve equal entry allowance",
    );
    check(
        e.other_context_recomputes > 0,
        "a different paid context was answered without computing: it inherited another context's refusal",
    );
    check(
        e.other_context == e.oracle_other_context,
        "the other paid context's answer differs from its cold oracle",
    );
    check(
        e.other_context.class == AnswerClass::OperationProfile,
        "the other paid context did not deliver the operation-profile refusal",
    );
    out
}

/// The gate rules of one publication-gate action: every holder held its gate
/// while parked in the commit, every source then showed holder and waiter
/// together, each cancelled waiter left its source to the holder alone, every
/// transition that was not cancelled completed once released, and afterwards
/// no gate, no participant and no more backing capacity than the peak's
/// remained.
pub fn check_gates(index: u32, e: &GateEvidence) -> Vec<SoakFailure> {
    let mut out = Vec::new();
    let mut check = |holds: bool, detail: &str| {
        if !holds {
            out.push(SoakFailure::WrongOutcome {
                index,
                detail: detail.into(),
            });
        }
    };
    let n = e.sources;
    check(n > 0, "the gate action drove no sources");
    check(
        e.holders_entered == n && e.live_with_holders == n,
        "not every source held a live gate while its holder was parked",
    );
    check(
        e.waited_sources == n,
        "a waiter did not register behind its source's holder",
    );
    check(
        e.cancelled > 0 && e.cancelled_left == e.cancelled,
        "a cancelled waiter did not leave its source's gate",
    );
    check(
        (2 * n).checked_sub(e.cancelled) == Some(e.completed),
        "an uncancelled transition did not complete after the release",
    );
    check(
        e.live_after == 0 && e.participants_after == 0,
        "a gate or participant outlived its last transition",
    );
    check(
        e.capacity_after < e.capacity_peak,
        "the gate registry kept its peak backing capacity",
    );
    out
}

/// The refusal rules of one refusal or healing action.
///
/// A retained refusal is a delivered answer of its variant's typed refusal
/// class (operation profile, or the connected query-depth isolated root)
/// sealed by computing it; only then may a repeat be demanded to replay it,
/// with the same answer and neither a component-meta resolution nor a cold
/// semantic producer run. An absent symbol, a missing input or any other
/// incomplete answer is never a refusal. An edit seals a new refusal; a cold
/// twin seals the same one; a larger operation profile, asked of the same
/// host that holds the tight profile's refusal, computes a complete answer.
/// The depth bound has no larger profile: a shallow equivalent heals it, and
/// a request that spent before asking, a different operation profile, a
/// sibling and a missing input each compute rather than inherit a refusal.
pub fn check_refusal(index: u32, e: &RefusalEvidence) -> Vec<SoakFailure> {
    let mut out = Vec::new();
    let mut check = |holds: bool, detail: &str| {
        if !holds {
            out.push(SoakFailure::RefusalBroken {
                index,
                detail: detail.into(),
            });
        }
    };
    let class = if e.variant.is_depth() {
        AnswerClass::ConnectedDepth
    } else {
        AnswerClass::OperationProfile
    };
    let computed = |d: &Demand| d.recomputes.is_some_and(|n| n > 0);
    let replayed = |d: &Demand, want: &Answer| {
        d.answer == *want && d.recomputes == Some(0) && d.producer_runs == Some(0)
    };
    let all = |role: DemandRole, count: usize, rule: &dyn Fn(&Demand) -> bool| {
        let demands: Vec<_> = e.of(role).collect();
        demands.len() == count && demands.iter().all(|d| rule(d))
    };
    if matches!(
        e.variant,
        RefusalVariant::DepthMissingInput | RefusalVariant::MissingInput
    ) {
        let missing_input = |d: &Demand| d.answer.class == AnswerClass::MissingInput && computed(d);
        check(
            all(DemandRole::Missing, 2, &missing_input),
            "a missing input was not delivered as its own cause, computed every time",
        );
        let supplied = e.of(DemandRole::Supplied).next();
        let want = if e.variant == RefusalVariant::MissingInput {
            AnswerClass::Complete
        } else {
            AnswerClass::ConnectedDepth
        };
        check(
            supplied.is_some_and(|d| d.answer.class == want && computed(d)),
            "supplying the missing input did not compute the remaining answer",
        );
        if let Some(supplied) = supplied {
            check(
                all(DemandRole::SuppliedRepeat, 1, &|d| {
                    replayed(d, &supplied.answer)
                }),
                "the supplied answer was not retained and replayed without work",
            );
        }
        return out;
    }
    let sealed = e.of(DemandRole::Sealed).next();
    check(
        sealed.is_some_and(|d| d.answer.class == class && computed(d)),
        "the first demand did not seal a computed refusal of the variant's typed class",
    );
    let Some(sealed) = sealed.filter(|d| d.answer.class == class) else {
        return out;
    };
    let sibling_exact = |d: &Demand| d.answer.is_complete();
    match e.variant {
        RefusalVariant::Sealed | RefusalVariant::DepthSealed => {
            check(
                all(DemandRole::Repeat, 2, &|d| replayed(d, &sealed.answer)),
                "a repeated refusal was not answered again without resolving",
            );
            check(
                all(DemandRole::Oracle, 1, &|d| {
                    d.answer == sealed.answer && computed(d)
                }),
                "a cold twin did not seal the same refusal by computing it",
            );
            if e.variant == RefusalVariant::DepthSealed {
                check(
                    all(DemandRole::Sibling, 1, &sibling_exact),
                    "a sibling of the refusing component inherited an incomplete answer",
                );
            }
        }
        RefusalVariant::Edited => {
            let edits: Vec<_> = e.of(DemandRole::Edit).collect();
            let repeats: Vec<_> = e.of(DemandRole::EditRepeat).collect();
            check(
                edits.len() == 3 && repeats.len() == 3,
                "the edited refusal was not demanded and repeated for every edit",
            );
            let mut previous = &sealed.answer;
            for (edit, repeat) in edits.iter().zip(&repeats) {
                check(
                    edit.answer.class == class
                        && edit.answer != *previous
                        && computed(edit)
                        && replayed(repeat, &edit.answer),
                    "an edit did not seal a new refusal that repeats without resolving",
                );
                previous = &edit.answer;
            }
        }
        RefusalVariant::DepthEdited => {
            check(
                all(DemandRole::Edit, 1, &|d| {
                    d.answer.class == class && computed(d)
                }),
                "a relevant edit did not invalidate the depth refusal and seal it again",
            );
            if let Some(edit) = e.of(DemandRole::Edit).next() {
                check(
                    all(DemandRole::EditRepeat, 1, &|d| replayed(d, &edit.answer)),
                    "the edited depth refusal was not replayed without resolving",
                );
            }
            let healed = e.of(DemandRole::ShallowHealed).next();
            check(
                healed.is_some_and(|d| d.answer.is_complete() && computed(d)),
                "the shallow equivalent did not heal the depth refusal by computing",
            );
            if let Some(healed) = healed {
                check(
                    all(DemandRole::HealedRepeat, 1, &|d| {
                        replayed(d, &healed.answer)
                    }),
                    "the healed answer was not retained and replayed without work",
                );
            }
        }
        RefusalVariant::DepthControls => {
            let computes_class = |d: &Demand| d.answer.class == class && computed(d);
            check(
                all(DemandRole::NotIsolated, 2, &computes_class),
                "a request that spent before asking inherited the isolated refusal",
            );
            check(
                all(DemandRole::OtherProfile, 1, &computes_class),
                "a different operation profile inherited the isolated refusal",
            );
            check(
                all(DemandRole::Sibling, 1, &sibling_exact),
                "a sibling of the refusing component inherited an incomplete answer",
            );
            check(
                all(DemandRole::Repeat, 1, &|d| replayed(d, &sealed.answer)),
                "the isolated refusal did not survive the ineligible requests",
            );
        }
        RefusalVariant::Healing => {
            let healed: Vec<_> = e.of(DemandRole::Healed).collect();
            check(
                healed.len() == 1
                    && healed[0].answer.is_complete()
                    && healed[0].answer != sealed.answer
                    && computed(healed[0]),
                "the larger profile did not heal the refusal with a complete answer",
            );
            // Both profiles ask one host: the complete answer the larger
            // profile computed now serves the tight profile without any
            // resolution.
            if let Some(healed) = healed.first() {
                check(
                    all(DemandRole::TightAfterHealed, 1, &|d| {
                        replayed(d, &healed.answer)
                    }),
                    "the tight profile was not served the healed answer without resolving",
                );
            }
        }
        RefusalVariant::Concurrent => {
            let tight: Vec<_> = e.of(DemandRole::ConcurrentTight).collect();
            let large: Vec<_> = e.of(DemandRole::ConcurrentLarge).collect();
            check(
                tight.len() == 2
                    && large.first().is_some_and(|l| {
                        tight
                            .iter()
                            .all(|d| d.answer == sealed.answer || d.answer == l.answer)
                    }),
                "a concurrent tight-profile demand delivered neither the sealed refusal nor the healed answer",
            );
            check(
                large.len() == 2
                    && large[0].answer.is_complete()
                    && large[0].answer == large[1].answer
                    && large[0].answer != sealed.answer,
                "concurrent larger-profile demands did not all deliver one complete answer",
            );
        }
        RefusalVariant::DepthMissingInput | RefusalVariant::MissingInput => {}
    }
    out
}

/// The ownership rules of a held answer, read from its roots, the process
/// account's charges and what the surviving handle projects.
///
/// A public copy shares the answer and its one charge: it is neither a
/// second independent root nor a second charge. Retiring the cache entries
/// the answer came from releases the cache's share (retained bytes never
/// grow) and leaves both handles reading it. Releasing the first handle
/// changes neither the roots nor any charge while the other survives and
/// still reads the same answer. Releasing the last gives its root back,
/// grows neither the retained nor the active charge, and gives back exactly
/// the one allocation charge the transfer probe measures by demanding and
/// releasing the same answer again — a transfer that dropped, doubled or
/// short-charged the allocation fails, as does a release with no probe
/// evidence. Correct roots with the wrong charges fail, and so does a final
/// drain alone.
pub fn check_ownership(
    index: u32,
    class: LifecycleClass,
    held: Option<&String>,
    steps: &[OwnerStep],
) -> Vec<SoakFailure> {
    let mut out = Vec::new();
    let mut check = |holds: bool, detail: &str| {
        if !holds {
            out.push(SoakFailure::OwnershipBroken {
                index,
                detail: detail.into(),
            });
        }
    };
    let step = |kind: OwnerStepKind| steps.iter().find(|s| s.step == kind);
    let projects_held =
        |s: Option<&OwnerStep>| s.is_some_and(|s| s.projected.as_ref() == held && held.is_some());
    if class == LifecycleClass::PublicCopy {
        let (before, after) = (
            step(OwnerStepKind::BeforeCopy),
            step(OwnerStepKind::AfterCopy),
        );
        check(
            projects_held(before) && projects_held(after),
            "the copy does not project the held answer",
        );
        check(
            matches!((before, after), (Some(b), Some(a)) if a.roots == b.roots),
            "sharing a copy changed the independent roots",
        );
        check(
            matches!((before, after), (Some(b), Some(a)) if a.charges == b.charges),
            "sharing a copy changed the account's charges",
        );
        check(
            [before, after].iter().all(|s| {
                s.and_then(|s| s.charges.content_pinned())
                    .is_some_and(|c| c > 0)
            }),
            "the held answer carries no content charge",
        );
        return out;
    }
    let (before, retired, first, last) = (
        step(OwnerStepKind::BeforeRelease),
        step(OwnerStepKind::AfterRetirement),
        step(OwnerStepKind::AfterFirstRelease),
        step(OwnerStepKind::AfterFinalRelease),
    );
    check(
        projects_held(before) && projects_held(retired) && projects_held(first),
        "a surviving handle does not project the held answer after retirement or the first release",
    );
    check(
        matches!((retired, first), (Some(r), Some(f)) if f.roots == r.roots),
        "the first release changed the independent roots while a handle survives",
    );
    check(
        matches!((retired, first), (Some(r), Some(f)) if f.charges == r.charges),
        "the first release changed the account's charges while a handle survives",
    );
    check(
        matches!((first, last), (Some(f), Some(l)) if l.roots + 1 == f.roots),
        "the final release did not give back the root",
    );
    check(
        matches!((retired, first, last), (Some(r), Some(f), Some(l))
            if r.charges.retained <= before.map_or(0, |b| b.charges.retained)
                && l.charges.retained <= f.charges.retained
                && l.charges.active <= f.charges.active),
        "retirement or the final release grew the retained or active charges",
    );
    // The held allocation's own charge: pinned bytes less the stores' backing
    // storage. The surviving readers hold it through retirement and the
    // first release (one allocation charge, transferred from the cache to the
    // readers, never dropped or doubled), and the last reader's release
    // gives it back.
    let content = |s: Option<&OwnerStep>| s.and_then(|s| s.charges.content_pinned());
    check(
        [before, retired, first]
            .iter()
            .all(|s| content(*s).is_some_and(|c| c > 0)),
        "a live reader holds no content charge before the final release",
    );
    check(
        matches!((content(before), content(retired)), (Some(b), Some(r)) if r <= b),
        "retirement grew the content charge instead of transferring it to the readers",
    );
    check(
        matches!((content(first), content(last)), (Some(f), Some(l)) if l < f),
        "the final release did not give back the content charge",
    );
    // The give-back is exact: the transfer probe — the same answer demanded
    // and released once more after the final release — returns precisely one
    // allocation's charge, so the final release must return the same amount.
    // A transfer that doubled or short-charged the allocation, amounts the
    // unrelated pins around the release cannot expose, fails; so does a
    // release with no probe evidence.
    let (acquired, released) = (
        step(OwnerStepKind::ProbeAcquired),
        step(OwnerStepKind::ProbeReleased),
    );
    match (content(acquired), content(released)) {
        (Some(pa), Some(pr)) => {
            check(
                pa.checked_sub(pr).is_some_and(|give_back| give_back > 0),
                "the transfer probe's release gave back no charge",
            );
            check(
                matches!(
                    (content(first), content(last), pa.checked_sub(pr)),
                    (Some(f), Some(l), Some(probe)) if f.checked_sub(l) == Some(probe)
                ),
                "the final release did not give back exactly the charge the transfer probe measured",
            );
        }
        _ => check(
            false,
            "the release recorded no transfer probe to measure the allocation against",
        ),
    }
    out
}

/// The pressure workload's surge outcomes.
///
/// Every surge, bounded or full, delivers every requested answer complete,
/// refuses nothing of the active class, and is covered by an ACTIVE window
/// that attributes its peak to the surge alone.
///
/// The full surges (the manifest's [`SURGE_MODULES`]-component ones) hold
/// enough simultaneously live answers to cross the aggregate ceiling and
/// publish a retained answer above the entry cap, so the account refuses on
/// both branches, attributed to the surge actions rather than to lifetime
/// counters; an answer the account refused is still delivered complete but
/// not retained; and the surge's own ACTIVE window reaches the former
/// advisory active target. Total, lifetime or pre-window peaks never stand
/// in for that window. A manifest that freezes a full surge and a run that
/// recorded zero or missing evidence for it fail.
pub fn check_pressure_outcomes(
    expected: &[Action],
    records: &[ActionRecord],
    failures: &mut Vec<SoakFailure>,
) {
    let requested = |index: u32| {
        expected
            .iter()
            .find_map(|a| match (&a.target, a.index == index) {
                (
                    Target::Surge {
                        modules,
                        props,
                        oversized_props,
                    },
                    true,
                ) => Some((*modules, *props, *oversized_props)),
                _ => None,
            })
    };
    let mut fail = |detail: String| failures.push(SoakFailure::PressureNotExercised { detail });
    let expected_full = expected
        .iter()
        .filter(|a| matches!(a.target, Target::Surge { modules, .. } if modules == SURGE_MODULES))
        .count();
    let surges: Vec<_> = records
        .iter()
        .filter(|r| r.class == LifecycleClass::PressureSurge)
        .collect();
    if surges.is_empty() {
        fail("the pressure workload ran no surge".into());
    }
    let mut full = Vec::new();
    for surge in &surges {
        // The request must be exactly a registered shape — its size, props
        // and oversized props all matching one set of frozen bounds: a
        // request no registered shape covers has no allowance at all, and a
        // larger request never enlarges the maxima it is judged by.
        let shape = match requested(surge.index) {
            Some((modules, props, oversized_props)) => match surge_limits_of(modules) {
                Some(l) if l.props == props && l.oversized_props == oversized_props => Some(l),
                _ => {
                    fail(format!(
                        "surge {} requested {modules} modules x {props}/{oversized_props} props: no registered surge shape",
                        surge.index
                    ));
                    None
                }
            },
            None => None,
        };
        let modules = shape.map(|l| l.modules);
        let complete = surge.outcome == Outcome::Value
            && modules.map(|m| m as usize) == Some(surge.digests.len())
            && surge.digests.iter().all(|d| d != ABSENT_DELIVERY);
        if !complete {
            fail(format!(
                "surge {} did not deliver every requested answer complete: {:?} with {} of {:?}",
                surge.index,
                surge.outcome,
                surge.digests.len(),
                modules
            ));
        }
        match surge
            .pressure
            .as_ref()
            .and_then(|p| p.surge_counts.as_ref())
        {
            Some(live) => {
                let violations = absolute_violations_for(live, shape);
                if !violations.is_empty() {
                    fail(format!(
                        "surge {}'s host exceeded the absolute limits while its results were live: {violations:?}",
                        surge.index
                    ));
                }
            }
            None => fail(format!(
                "surge {} recorded no live populations of its host",
                surge.index
            )),
        }
        let pressure = surge.pressure.as_ref();
        let attributed = pressure
            .and_then(|p| p.active.as_ref())
            .and_then(|a| a.attributed_peak);
        if pressure.is_none_or(|p| p.refusals_active != 0) || attributed.is_none() {
            fail(format!(
                "surge {} refused active work or has no attributable ACTIVE window: {pressure:?}",
                surge.index
            ));
        }
        if modules == Some(SURGE_MODULES) {
            full.push((surge, attributed));
        }
    }
    if full.len() != expected_full {
        fail(format!(
            "{} of {expected_full} full surges were recorded",
            full.len()
        ));
    }
    if expected_full == 0 {
        return;
    }
    for (surge, attributed) in &full {
        if attributed.is_none_or(|peak| peak < FORMER_ACTIVE_TARGET_BYTES) {
            fail(format!(
                "full surge {}'s own ACTIVE window did not reach the former active target: {attributed:?}",
                surge.index
            ));
        }
        let retained = surge.pressure.as_ref().and_then(|p| p.retained_results);
        if retained.is_none_or(|kept| kept >= surge.digests.len() as u64) {
            fail(format!(
                "full surge {} retained every delivered answer: nothing was delivered complete but uncached ({retained:?})",
                surge.index
            ));
        }
    }
    let total = |pick: fn(&ActionRecord) -> Option<u64>| {
        full.iter().map(|(r, _)| pick(r).unwrap_or(0)).sum::<u64>()
    };
    if total(|r| r.refusals_pressure) == 0 {
        fail("no full surge was refused for aggregate pressure".into());
    }
    if total(|r| r.refusals_oversized) == 0 {
        fail("no full surge published an oversized retained answer".into());
    }
}

/// The digest a surge records for a component the host delivered nothing for.
pub const ABSENT_DELIVERY: &str = "absent";
/// The account's former advisory active target: admitted work is expected to
/// cross it without any refusal of the active class.
pub const FORMER_ACTIVE_TARGET_BYTES: u64 = 512 * 1024 * 1024;

/// The account byte limits and workload count limits every sample, the
/// opening window included, must satisfy. Counts are the structural bound of
/// the frozen workload: a tranche has at most this many documents, handles
/// and requests live at once.
pub const ABSOLUTE_LIMITS: &[(&str, u64)] = &[
    // History retired in amortised passes past the owner's trigger.
    ("freshness_exact_entries", 4096),
    ("freshness_subtree_entries", 4096),
    ("freshness_queued_entries", 4096),
    ("resolution_retired_decisions", 256),
    ("active_bytes", 512 * 1024 * 1024),
    ("pinned_bytes", 512 * 1024 * 1024),
    ("content_pinned_bytes", 512 * 1024 * 1024),
    // A workload bound well under the account's own ceiling (which can never
    // fail: the account refuses past it): a tranche's retained results peak
    // near a megabyte and fall back as it closes, so growth that the close
    // route later clears, per-entry byte growth with flat entry counts
    // included, crosses this long before the ceiling.
    ("retained_bytes", 64 * 1024 * 1024),
    ("live_artifacts", 16),
    ("registered_sources", 16),
    ("live_roots", 64),
    ("snapshot_leases", 64),
    ("resolution_flights", 8),
];
/// The refusable (active plus retained) occupancy the account may never admit
/// past: the ratified aggregate ceiling.
pub const REFUSABLE_CEILING_BYTES: u64 = 1024 * 1024 * 1024;

/// The populations a registered surge shape bounds at its own frozen
/// maxima instead of the soak host's: (figure, its bound in
/// [`SurgeLimits`]).
type SurgeFigure = (&'static str, fn(&SurgeLimits) -> u64);

const SURGE_REGISTERED_FIGURES: &[SurgeFigure] = &[
    ("live_artifacts", |l| l.live_artifacts_max),
    ("registered_sources", |l| l.registered_sources_max),
    ("snapshot_leases", |l| l.snapshot_leases_max),
    ("retained_bytes", |l| l.retained_bytes_max),
];

fn absolute_violations(counts: &verter_audit::soak::SoakCounts) -> Vec<CountViolation> {
    absolute_violations_for(counts, None)
}

/// The absolute limits for `counts`; a registered surge shape's host is
/// bounded by that shape's frozen maxima for the populations it scales
/// with its components, every other figure by the same limits as the soak
/// host. The maxima are fixed at their registered values: they never grow
/// with what a request asks for.
fn absolute_violations_for(
    counts: &verter_audit::soak::SoakCounts,
    surge: Option<&SurgeLimits>,
) -> Vec<CountViolation> {
    let figures = counts.figures();
    let registered = |name: &str| {
        surge.and_then(|l| {
            SURGE_REGISTERED_FIGURES
                .iter()
                .find(|(figure, _)| *figure == name)
                .map(|(_, bound)| bound(l))
        })
    };
    let mut out: Vec<CountViolation> = ABSOLUTE_LIMITS
        .iter()
        .filter_map(|(name, limit)| {
            let observed = figures.iter().find(|(n, _, _)| n == name)?.2;
            let limit = registered(name).unwrap_or(*limit);
            (observed > limit).then(|| CountViolation {
                figure: (*name).to_string(),
                expected_at_most: limit,
                observed,
            })
        })
        .collect();
    let refusable = counts.active_bytes + counts.retained_bytes;
    if refusable > REFUSABLE_CEILING_BYTES {
        out.push(CountViolation {
            figure: "active_bytes+retained_bytes".into(),
            expected_at_most: REFUSABLE_CEILING_BYTES,
            observed: refusable,
        });
    }
    out
}

fn check_inventory(expected: &Manifest, summary: &SoakSummary, failures: &mut Vec<SoakFailure>) {
    let executed: BTreeMap<u32, LifecycleClass> =
        summary.records.iter().map(|r| (r.index, r.class)).collect();
    if executed.len() != summary.records.len() {
        failures.push(SoakFailure::InventoryMismatch {
            detail: "an action index was recorded twice".into(),
        });
    }
    let missing: Vec<u32> = expected
        .actions
        .iter()
        .filter(|a| executed.get(&a.index) != Some(&a.class))
        .map(|a| a.index)
        .collect();
    let extra = executed.len().saturating_sub(expected.actions.len());
    if !missing.is_empty() || extra > 0 {
        failures.push(SoakFailure::InventoryMismatch {
            detail: format!(
                "{} manifest actions without a matching result (first {:?}), {extra} extra results",
                missing.len(),
                missing.iter().take(5).collect::<Vec<_>>()
            ),
        });
    }
    let ran: BTreeSet<LifecycleClass> = summary
        .records
        .iter()
        .filter(|r| !matches!(r.outcome, Outcome::Skipped { .. }))
        .map(|r| r.class)
        .collect();
    for class in LifecycleClass::required(expected.mode) {
        if !ran.contains(&class) {
            failures.push(SoakFailure::MissingClass { class });
        }
    }
    for record in &summary.records {
        if matches!(record.outcome, Outcome::Skipped { .. }) {
            failures.push(SoakFailure::UnexpectedSkip {
                index: record.index,
            });
        }
    }
}

fn check_outcomes(expected: &Manifest, summary: &SoakSummary, failures: &mut Vec<SoakFailure>) {
    use LifecycleClass::*;
    let actions: BTreeMap<u32, _> = expected.actions.iter().map(|a| (a.index, a)).collect();
    // The first answer of each (tranche, file, alias) at revision 0.
    let mut cold: BTreeMap<(u32, String, String), String> = BTreeMap::new();
    let wrong = |index, detail: String| SoakFailure::WrongOutcome { index, detail };
    // The digest of the answer each pin slot last held.
    let mut held: BTreeMap<u32, String> = BTreeMap::new();
    for record in &summary.records {
        let Some(action) = actions.get(&record.index) else {
            continue;
        };
        if matches!(record.outcome, Outcome::Skipped { .. }) {
            continue;
        }
        let query = match &action.target {
            Target::Query { file, alias } | Target::Hold { file, alias, .. } => {
                Some((action.tranche, file.clone(), alias.clone()))
            }
            _ => None,
        };
        let first = record.digests.first().cloned().unwrap_or_default();
        let expect = |want: Outcome| record.outcome == want;
        // Every result the action's target promises is present.
        let promised = match &action.target {
            Target::Overlap { threads, .. } => Some(*threads as usize),
            Target::Queries { queries } => Some(queries.len()),
            Target::Churn { edits, .. } => Some(*edits as usize + 1),
            Target::Surge { modules, .. } => Some(*modules as usize),
            // A single demand promises one materialised answer.
            Target::Query { .. } | Target::Hold { .. } => Some(1),
            _ => None,
        };
        if let Some(promised) = promised {
            if record.digests.len() != promised
                || record
                    .digests
                    .iter()
                    .any(|d| d == ABSENT_DELIVERY || d == "unmaterialised")
            {
                failures.push(wrong(
                    record.index,
                    format!(
                        "{} of {promised} promised results delivered",
                        record
                            .digests
                            .iter()
                            .filter(|d| *d != ABSENT_DELIVERY && *d != "unmaterialised")
                            .count()
                    ),
                ));
            }
        }
        match record.class {
            ColdDemand | HoldPin | LargeDemand | UniqueOwnerChurn => {
                if !expect(Outcome::Value) {
                    failures.push(wrong(record.index, format!("{:?}", record.outcome)));
                } else if let Some(key) = query {
                    if let Target::Hold { slot, .. } = &action.target {
                        held.insert(*slot, first.clone());
                    }
                    cold.entry(key).or_insert(first);
                }
            }
            ProfileHealing => {
                // The narrower named-symbol profile and the default profile
                // that heals it answer alike; the larger component-meta
                // profile heals its refusal.
                let healed = record.digests.len() == 2 && record.digests[0] == record.digests[1];
                if !expect(Outcome::Value) || !healed {
                    failures.push(wrong(
                        record.index,
                        "the default profile did not heal the narrower profile's answer".into(),
                    ));
                }
                match &record.refusal {
                    Some(evidence) => failures.extend(check_refusal(record.index, evidence)),
                    None => failures.push(wrong(
                        record.index,
                        "the healing action recorded no refusal observations".into(),
                    )),
                }
            }
            RefusalRepeat => match &record.refusal {
                Some(evidence) => failures.extend(check_refusal(record.index, evidence)),
                None => failures.push(wrong(
                    record.index,
                    "the refusal action recorded no observations".into(),
                )),
            },
            WarmDemand | PostEditDemand => {
                let answered = record.digests.last().cloned().unwrap_or_default();
                let before = query.as_ref().and_then(|key| cold.get(key));
                let same = before == Some(&answered);
                let rule_holds = expect(Outcome::Value)
                    && before.is_some()
                    && if record.class == PostEditDemand {
                        !same
                    } else {
                        same
                    };
                if !rule_holds {
                    failures.push(wrong(
                        record.index,
                        format!(
                            "{:?}: answer {} the cold answer",
                            record.outcome,
                            if same { "equals" } else { "differs from" }
                        ),
                    ));
                }
            }
            RevertSource => {
                let key = (action.tranche, file_of(&action.target), "A0".to_string());
                if !expect(Outcome::Value) || cold.get(&key) != Some(&first) {
                    failures.push(wrong(
                        record.index,
                        "the restored source does not answer its original answer".into(),
                    ));
                }
            }
            CandidateChurn => {
                let edits = match &action.target {
                    Target::Churn { edits, .. } => u64::from(*edits),
                    _ => 0,
                };
                // Eviction needs positive evidence: the churned identity held a
                // candidate that a later version displaced. Uncached or
                // refused demands (no candidate ever admitted) are no
                // eviction.
                let evicted = record.pressure.as_ref().is_some_and(|p| {
                    let trace = &p.memo_candidate_trace;
                    p.memo_candidate_cap > 0
                        && trace.len() as u64 == edits + 1
                        && trace.iter().all(|tokens| {
                            tokens.len() <= p.memo_candidate_cap
                                && tokens.iter().collect::<BTreeSet<_>>().len() == tokens.len()
                        })
                        && trace.windows(2).any(|pair| {
                            let (was, now) = (&pair[0], &pair[1]);
                            was.len() == p.memo_candidate_cap
                                && now.len() == p.memo_candidate_cap
                                && was.iter().any(|token| !now.contains(token))
                                && now.iter().any(|token| !was.contains(token))
                        })
                });
                if !evicted {
                    failures.push(wrong(
                        record.index,
                        "the churned identity displaced no admitted candidate: nothing was evicted"
                            .into(),
                    ));
                }
                let key = query_of(action.tranche, &action.target);
                let original = cold.get(&key);
                let (restored, churned) = record.digests.split_last().unzip();
                let churned = churned.unwrap_or_default();
                let distinct: BTreeSet<_> = churned.iter().collect();
                if !expect(Outcome::Value)
                    || churned.is_empty()
                    || distinct.len() != churned.len()
                    || churned.iter().any(|d| Some(d) == original)
                    || original.is_none()
                    || restored != original
                {
                    failures.push(wrong(
                        record.index,
                        "churned versions must answer distinctly and the restore must answer the original".into(),
                    ));
                }
            }
            OverlappingCold => {
                let in_flight = record.max_in_flight.unwrap_or(0);
                let joined = record.joined_waits.unwrap_or(0);
                let flight = record.flight.unwrap_or_default();
                // One shared cold producer, a subscriber registered on its
                // flight, and a join the semantic store recorded: caller
                // concurrency alone is not overlap.
                if in_flight < 2
                    || joined == 0
                    || flight.producer_claims != 1
                    || flight.subscribers == 0
                {
                    failures.push(SoakFailure::FakeOverlap {
                        index: record.index,
                        max_in_flight: in_flight,
                        joined_waits: joined,
                        producer_claims: flight.producer_claims,
                        subscribers: flight.subscribers,
                    });
                }
                let distinct: BTreeSet<_> = record.digests.iter().collect();
                if !expect(Outcome::Value) || distinct.len() != 1 {
                    failures.push(wrong(
                        record.index,
                        "overlapping demands must all answer one value".into(),
                    ));
                }
            }
            PublicationGates => match &record.gates {
                Some(evidence) => failures.extend(check_gates(record.index, evidence)),
                None => failures.push(wrong(
                    record.index,
                    "the publication-gate action recorded no observations".into(),
                )),
            },
            MetaIsolation => match &record.isolation {
                Some(evidence) => failures.extend(check_isolation(record.index, evidence)),
                None => failures.push(wrong(
                    record.index,
                    "the isolation action recorded no observations".into(),
                )),
            },
            PressureSurge | MixedConcurrent => {
                if !expect(Outcome::Value) {
                    failures.push(wrong(record.index, format!("{:?}", record.outcome)));
                }
            }
            Cancellation => {
                if !expect(Outcome::Cancelled) {
                    failures.push(SoakFailure::CancellationNotObserved {
                        index: record.index,
                    });
                }
                if record.retry == Some(Outcome::Value)
                    && record
                        .retry_digest
                        .as_deref()
                        .is_none_or(|d| d == ABSENT_DELIVERY || d == "unmaterialised")
                {
                    failures.push(wrong(
                        record.index,
                        "the uncancelled retry delivered no materialised answer".into(),
                    ));
                }
                if record.retry != Some(Outcome::Value) {
                    failures.push(wrong(
                        record.index,
                        format!("the uncancelled retry answered {:?}", record.retry),
                    ));
                }
            }
            ConfigChange => {
                if !expect(Outcome::Value) || first == summary.sentinel_digest {
                    failures.push(wrong(
                        record.index,
                        "the alternate configuration must move the sentinel".into(),
                    ));
                }
            }
            ConfigRestore => {
                if !expect(Outcome::Value) || first != summary.sentinel_digest {
                    failures.push(SoakFailure::WrongProjectReversion {
                        index: record.index,
                    });
                }
            }
            PublicCopy | ReleaseCopyThenOriginal | ReleaseOriginalThenCopy => {
                if !expect(Outcome::Done) {
                    failures.push(wrong(record.index, format!("{:?}", record.outcome)));
                }
                let slot = match &action.target {
                    Target::Slot { slot } | Target::Release { slot, .. } => Some(*slot),
                    _ => None,
                };
                failures.extend(check_ownership(
                    record.index,
                    record.class,
                    slot.and_then(|s| held.get(&s)),
                    &record.ownership,
                ));
            }
            Open | Edit | Close | Reclaim => {
                if !expect(Outcome::Done) {
                    failures.push(wrong(record.index, format!("{:?}", record.outcome)));
                }
            }
        }
    }
}

fn file_of(target: &Target) -> String {
    match target {
        Target::File { file, .. } => file.clone(),
        _ => String::new(),
    }
}

fn query_of(tranche: u32, target: &Target) -> (u32, String, String) {
    match target {
        Target::Churn { file, alias, .. } => (tranche, file.clone(), alias.clone()),
        _ => (tranche, String::new(), String::new()),
    }
}

fn check_samples(summary: &SoakSummary, failures: &mut Vec<SoakFailure>) {
    let samples = &summary.samples;
    let (Some(baseline), Some(last)) = (samples.first(), samples.last()) else {
        failures.push(SoakFailure::SamplingGap { from: 0, to: 0 });
        return;
    };
    // The timeline is complete: the baseline at zero actions, one `action`
    // sample at every multiple of the cadence up to the executed actions
    // (the opening window included), then the closed and final samples at the
    // executed total — no other tag, no missing or repeated position.
    let total = summary.records.len() as u32;
    let during: Vec<u32> = samples
        .iter()
        .filter(|s| s.at == "action")
        .map(|s| s.actions_done)
        .collect();
    let expected: Vec<u32> = (1..=total / SAMPLE_EVERY)
        .map(|n| n * SAMPLE_EVERY)
        .collect();
    let tags_in_order = samples.len() == during.len() + 3
        && samples
            .get(1 + during.len())
            .is_some_and(|s| s.at == "closed")
        && samples
            .iter()
            .skip(1)
            .take(during.len())
            .all(|s| s.at == "action");
    if baseline.at != "baseline"
        || last.at != "final"
        || baseline.actions_done != 0
        || last.actions_done != total
        || !tags_in_order
        || samples
            .iter()
            .filter(|s| s.at == "closed")
            .any(|s| s.actions_done != total)
        || during != expected
    {
        failures.push(SoakFailure::SamplingGap { from: 0, to: total });
    }
    for sample in samples {
        let violations = absolute_violations(&sample.counts);
        if !violations.is_empty() {
            failures.push(SoakFailure::AbsoluteLimitExceeded {
                actions_done: sample.actions_done,
                violations,
            });
        }
    }
    for pair in samples.windows(2) {
        if pair[1].actions_done.saturating_sub(pair[0].actions_done) > SAMPLE_EVERY {
            failures.push(SoakFailure::SamplingGap {
                from: pair[0].actions_done,
                to: pair[1].actions_done,
            });
        }
    }
    // The envelope holds each position within a tranche to the maximum the
    // opening window's tranches reached at that position: tranches repeat one
    // template, so a later tranche above it retained what an earlier one did
    // not.
    let tranche = super::manifest::ACTIONS_PER_TRANCHE;
    let during: Vec<_> = samples.iter().filter(|s| s.at == "action").collect();
    let mut envelopes: BTreeMap<u32, verter_audit::soak::SoakCounts> = BTreeMap::new();
    for sample in during.iter().filter(|s| s.actions_done <= ENVELOPE_ACTIONS) {
        let slot = envelopes.entry(sample.actions_done % tranche).or_default();
        *slot = envelope_of([&*slot, &sample.counts]);
    }
    for sample in during.iter().filter(|s| s.actions_done > ENVELOPE_ACTIONS) {
        let Some(envelope) = envelopes.get(&(sample.actions_done % tranche)) else {
            continue;
        };
        let violations = envelope_violations(envelope, &sample.counts);
        if !violations.is_empty() {
            failures.push(SoakFailure::EnvelopeExceeded {
                actions_done: sample.actions_done,
                violations,
            });
        }
    }
    // After the close route every drained figure of the host is back at its
    // baseline; after the host is dropped the process-wide account is too.
    let closed = samples.iter().find(|s| s.at == "closed");
    let mut leaks = closed
        .map(|closed| drain_violations(&baseline.counts, &closed.counts, DrainPoint::Closed))
        .unwrap_or_default();
    leaks.extend(drain_violations(
        &baseline.counts,
        &last.counts,
        DrainPoint::Teardown,
    ));
    if closed.is_none() {
        failures.push(SoakFailure::SamplingGap {
            from: summary.records.len() as u32,
            to: summary.records.len() as u32,
        });
    }
    if !leaks.is_empty() {
        failures.push(SoakFailure::RetainedLeak { violations: leaks });
    }
}

/// Every executed action, and the setup, published exactly the path-bound
/// bytes the regenerated manifest freezes for it.
fn check_publications(expected: &Manifest, summary: &SoakSummary, failures: &mut Vec<SoakFailure>) {
    let mismatch = |want: &[String], got: &[String]| {
        (want != got).then(|| {
            format!(
                "{} publications recorded, {} frozen; first differing at {}",
                got.len(),
                want.len(),
                want.iter()
                    .zip(got)
                    .position(|(w, g)| w != g)
                    .unwrap_or(want.len().min(got.len()))
            )
        })
    };
    if let Some(detail) = mismatch(
        &super::fixtures::setup_publications(),
        &summary.setup_published,
    ) {
        failures.push(SoakFailure::PublicationMismatch {
            index: None,
            detail,
        });
    }
    let actions: BTreeMap<u32, _> = expected.actions.iter().map(|a| (a.index, a)).collect();
    for record in &summary.records {
        if matches!(record.outcome, Outcome::Skipped { .. }) {
            continue;
        }
        let Some(action) = actions.get(&record.index) else {
            continue;
        };
        let want = super::fixtures::expected_publications(expected.mode, action);
        if let Some(detail) = mismatch(&want, &record.published) {
            failures.push(SoakFailure::PublicationMismatch {
                index: Some(record.index),
                detail,
            });
        }
    }
}
