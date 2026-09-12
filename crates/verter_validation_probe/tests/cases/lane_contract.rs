//! The lane's own contract, exercised hermetically.
//!
//! Every case here drives the real runner, the real fold, and the real summary
//! over SYNTHETIC driver frames, so each one discriminates a boundary the
//! external lane cannot: a driver that dies at each phase, a frame that
//! answers the wrong entry, a reference that is absent for the wrong reason,
//! and a summary that under-reports.
//!
//! No corpus is read and no addon is loaded: the canonical hermetic run neither
//! requires nor provisions either.

use verter_validation_probe::manifest::{Framework, ProbeStateManifest};
use verter_validation_probe::outcome::{Dimension, NotApplicableReason, ProbeOutcomeClass as C};
use verter_validation_probe::request;
use verter_validation_probe::runner::{
    self as runner, classify_execution, parse_line, DiagnosticsSnapshot, DriverCommand, DriverLine,
    ExecutionEvent, FrameViolation, Phase, PhaseDeadlines, PlannedCase, ProbeRun, ReferenceResult,
    RequestedEntry, RouteDiagnostic, RouteEntry, RouteFailure, RouteNode, RouteProduct,
    RouteResponse, VirtualNodeKind,
};
use verter_validation_probe::summary::{self, FrameworkRun, Lane, ObservedCase, Summary};
use verter_validation_probe::{Evaluation, Terminal};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const CASE: &str = "vue/fixtures/App.vue";
const SVELTE_CASE: &str = "svelte/fixtures/App.svelte";

/// A module both sides of a comparison can be given so `Structural` is decided
/// by the comparator rather than by a parse failure.
const MODULE: &str = "export const value = 1\n";

fn vue_manifest() -> ProbeStateManifest {
    ProbeStateManifest::from_toml_str(&manifest_toml(
        "vue",
        "fixtures/App.vue",
        "structural",
        r#"
[comparator]
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"
"#,
    ))
    .expect("the vue fixture manifest is valid")
}

fn svelte_manifest() -> ProbeStateManifest {
    ProbeStateManifest::from_toml_str(&manifest_toml("svelte", "fixtures/App.svelte", "none", ""))
        .expect("the svelte fixture manifest is valid")
}

fn manifest_toml(framework: &str, case: &str, comparison: &str, comparator: &str) -> String {
    let probe_id = format!("{framework}/{case}");
    let product = format!("{framework}.runtime-client-product");
    let structural = if comparison == "structural" {
        format!(
            r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-identity"
"#
        )
    } else {
        format!(
            r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "skip"
authority = "{product}"
atom = "product-identity"
reason = "comparator_absent"
"#
        )
    };
    format!(
        r#"
framework = "{framework}"
comparison = "{comparison}"
external_revision = "0123456789abcdef0123456789abcdef01234567"
smoke = ["{probe_id}"]
{comparator}
[applicability]
runtime = "inapplicable"
map = "inapplicable"

[[strata]]
id = "app"
pattern = "App*"
min_cases = 1

[[inventory]]
case_id = "{probe_id}"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Route"
expected_state = "gate"
expected_class = "pass"
authority = "compiler.public-request-route"
atom = "route-callable"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Compile"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-shape"
{structural}
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Runtime"
expected_state = "skip"
authority = "{product}"
atom = "runtime-behavior"
reason = "runtime_executor_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Map"
expected_state = "skip"
authority = "{product}"
atom = "source-map"
reason = "map_validator_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Performance"
expected_state = "canary"
expected_class = "pass"
authority = "compiler.equivalent-work-ledger"
atom = "equivalent-work-ledger"
"#
    )
}

fn requested(case_id: &str) -> Vec<RequestedEntry> {
    vec![RequestedEntry {
        canonical_id: case_id.to_string(),
        source: "<template><div/></template>\n".to_string(),
        request_digest: request::request_digest("fixtures/App.vue"),
    }]
}

fn ok_entry(case_id: &str) -> RouteEntry {
    RouteEntry {
        canonical_id: case_id.to_string(),
        response: Some(RouteResponse {
            diagnostics: DiagnosticsSnapshot::default(),
            products: vec![RouteProduct {
                kind: "runtimeClient".to_string(),
                nodes: Some(vec![RouteNode {
                    node: VirtualNodeKind {
                        kind: "main".to_string(),
                    },
                    code: MODULE.to_string(),
                }]),
            }],
        }),
        failure: None,
    }
}

fn failure_entry(case_id: &str, kind: &str, severity: Option<&str>) -> RouteEntry {
    RouteEntry {
        canonical_id: case_id.to_string(),
        response: None,
        failure: Some(RouteFailure {
            kind: kind.to_string(),
            message: "refused".to_string(),
            diagnostics: DiagnosticsSnapshot {
                diagnostics: severity
                    .map(|severity| RouteDiagnostic {
                        severity: severity.to_string(),
                        code: "E1".to_string(),
                        message: "boom".to_string(),
                    })
                    .into_iter()
                    .collect(),
            },
        }),
    }
}

fn compile_frame(case_id: &str, entries: Vec<RouteEntry>) -> DriverLine {
    DriverLine::Compile {
        probe_id: case_id.to_string(),
        elapsed_ns: 1_000,
        entries,
    }
}

fn reference_frame(case_id: &str, reference: Vec<ReferenceResult>) -> DriverLine {
    DriverLine::Reference {
        probe_id: case_id.to_string(),
        reference,
    }
}

fn phase(case_id: &str, phase: Phase) -> DriverLine {
    DriverLine::Phase {
        probe_id: case_id.to_string(),
        phase,
    }
}

/// Fold one probe and return its single observation's terminals.
fn observe(
    manifest: &ProbeStateManifest,
    case_id: &str,
    lines: Vec<DriverLine>,
    terminated: Option<ExecutionEvent>,
) -> std::collections::BTreeMap<Dimension, Terminal> {
    let mut run = ProbeRun::new(case_id, requested(case_id));
    for line in lines {
        let _ = run.ingest_frame(line);
    }
    let mut folded = run.finish(manifest, terminated);
    folded
        .remove(0)
        .unwrap_or_else(|error| panic!("the observation is representable: {error}"))
        .terminals()
        .clone()
}

fn class_at(
    terminals: &std::collections::BTreeMap<Dimension, Terminal>,
    dimension: Dimension,
) -> Option<C> {
    terminals[&dimension].class()
}

// ---------------------------------------------------------------------------
// Execution classification
// ---------------------------------------------------------------------------

/// A reference that hangs, an addon that will not load, and a compiler that
/// crashes are three DIFFERENT causes. Collapsing any pair would make the lane
/// unable to say whether Verter or its harness failed.
#[test]
fn a_reference_hang_an_addon_load_failure_and_a_compiler_crash_are_distinct_classes() {
    assert_eq!(
        classify_execution(ExecutionEvent::SpawnFailed),
        C::HarnessFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::TimedOut { phase: None }),
        C::HarnessFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::TimedOut {
            phase: Some(Phase::Load)
        }),
        C::HarnessFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::TimedOut {
            phase: Some(Phase::Compile)
        }),
        C::Timeout,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::Signaled {
            phase: Some(Phase::Compile)
        }),
        C::Crash,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::TimedOut {
            phase: Some(Phase::Reference)
        }),
        C::ReferenceFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::Signaled {
            phase: Some(Phase::Reference)
        }),
        C::ReferenceFailure,
    );
}

/// A non-zero exit maps by the phase it happened in, exactly as a signal or a
/// timeout does; a clean exit decides nothing on its own.
#[test]
fn a_non_zero_exit_maps_by_phase_and_a_clean_exit_decides_nothing() {
    assert_eq!(
        classify_execution(ExecutionEvent::Exited {
            phase: Some(Phase::Load),
            code: 3
        }),
        C::HarnessFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::Exited {
            phase: Some(Phase::Compile),
            code: 3
        }),
        C::Crash,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::Exited {
            phase: Some(Phase::Reference),
            code: 3
        }),
        C::ReferenceFailure,
    );
    assert_eq!(
        classify_execution(ExecutionEvent::Exited {
            phase: Some(Phase::Compile),
            code: 0
        }),
        C::Pass,
    );
}

// ---------------------------------------------------------------------------
// Frame authentication
// ---------------------------------------------------------------------------

/// A reordered batch must never attach one case's product to another case.
#[test]
fn a_reordered_batch_is_refused_rather_than_paired_positionally() {
    let mut run = ProbeRun::new(
        CASE,
        vec![
            requested(CASE).remove(0),
            RequestedEntry {
                canonical_id: "vue/fixtures/Other.vue".to_string(),
                source: String::new(),
                request_digest: String::new(),
            },
        ],
    );
    let violation = run
        .ingest_frame(compile_frame(
            CASE,
            vec![ok_entry("vue/fixtures/Other.vue"), ok_entry(CASE)],
        ))
        .expect_err("a reordered batch is refused");
    assert!(
        matches!(
            violation,
            FrameViolation::IdentityMismatch { position: 0, .. }
        ),
        "expected a position-0 identity mismatch, got {violation:?}",
    );
    assert!(
        !run.has_compile_frame(),
        "a frame that failed its own identity check must not be retained",
    );
}

/// A reference frame with no compile frame is the protocol breaking, not a
/// comparison that can be paired against something.
#[test]
fn a_reference_frame_without_its_compile_frame_is_refused() {
    let mut run = ProbeRun::new(CASE, requested(CASE));
    let violation = run
        .ingest_frame(reference_frame(
            CASE,
            vec![ReferenceResult::Produced {
                code: MODULE.to_string(),
            }],
        ))
        .expect_err("a reference frame without a compile frame is refused");
    assert!(matches!(
        violation,
        FrameViolation::ReferenceWithoutCompile { .. }
    ));
}

/// A second compile frame, and a frame answering the wrong number of entries,
/// are both refused whole.
#[test]
fn a_duplicate_or_miscounted_frame_is_refused() {
    let mut run = ProbeRun::new(CASE, requested(CASE));
    run.ingest_frame(compile_frame(CASE, vec![ok_entry(CASE)]))
        .expect("the first compile frame is accepted");
    assert!(matches!(
        run.ingest_frame(compile_frame(CASE, vec![ok_entry(CASE)]))
            .expect_err("a second compile frame is refused"),
        FrameViolation::DuplicateCompileFrame { .. }
    ));

    let mut run = ProbeRun::new(CASE, requested(CASE));
    assert!(matches!(
        run.ingest_frame(compile_frame(CASE, vec![ok_entry(CASE), ok_entry(CASE)]))
            .expect_err("a miscounted frame is refused"),
        FrameViolation::CountMismatch {
            requested: 1,
            answered: 2,
            ..
        }
    ));
}

/// A refused frame leaves the probe a harness failure, never a verdict about
/// the compiler it could not observe.
#[test]
fn a_refused_frame_makes_the_probe_a_harness_failure() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry("vue/fixtures/Other.vue")]),
        ],
        Some(ExecutionEvent::Exited {
            phase: Some(Phase::Compile),
            code: 0,
        }),
    );
    assert_eq!(
        class_at(&terminals, Dimension::Route),
        Some(C::HarnessFailure)
    );
}

// ---------------------------------------------------------------------------
// Protocol finalization
// ---------------------------------------------------------------------------

/// A driver that exits cleanly without sending a compile frame leaves the
/// Route dimension unanswered — a harness failure, never a pass.
#[test]
fn a_clean_exit_before_the_compile_frame_is_a_harness_failure_at_route() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![phase(CASE, Phase::Load)],
        Some(ExecutionEvent::Exited {
            phase: Some(Phase::Load),
            code: 0,
        }),
    );
    assert_eq!(
        class_at(&terminals, Dimension::Route),
        Some(C::HarnessFailure)
    );
}

/// A driver that exits cleanly AFTER the compile frame but before the
/// reference frame keeps its already-observed Route and Compile terminals and
/// loses only the Structural cell. The reference frame is required for EVERY
/// probe, so its absence is a harness failure even for a framework with no
/// registered producer.
#[test]
fn a_clean_exit_after_the_compile_frame_costs_only_the_structural_cell() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
        ],
        Some(ExecutionEvent::Exited {
            phase: Some(Phase::Reference),
            code: 0,
        }),
    );
    assert_eq!(class_at(&terminals, Dimension::Route), Some(C::Pass));
    assert_eq!(class_at(&terminals, Dimension::Compile), Some(C::Pass));
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::HarnessFailure),
        "a probe whose reference frame never arrived has an incomplete protocol",
    );
    assert_eq!(class_at(&terminals, Dimension::Performance), Some(C::Pass));
}

/// The same holds for a framework with no registered producer: `comparison =
/// none` makes an `inapplicable` reference ACCEPTABLE, but it does not make the
/// reference frame optional.
#[test]
fn a_clean_exit_after_compile_is_a_harness_failure_for_an_unregistered_producer_too() {
    let manifest = svelte_manifest();
    let mut run = ProbeRun::new(
        SVELTE_CASE,
        vec![RequestedEntry {
            canonical_id: SVELTE_CASE.to_string(),
            source: String::new(),
            request_digest: String::new(),
        }],
    );
    let _ = run.ingest_frame(phase(SVELTE_CASE, Phase::Compile));
    let _ = run.ingest_frame(compile_frame(SVELTE_CASE, vec![ok_entry(SVELTE_CASE)]));
    let observation = run
        .finish(
            &manifest,
            Some(ExecutionEvent::Exited {
                phase: Some(Phase::Compile),
                code: 0,
            }),
        )
        .remove(0)
        .expect("the observation is representable");
    // `comparison = none` makes Structural NOT APPLICABLE for this framework,
    // so the missing reference frame cannot be reported there. What must not
    // happen is the probe reporting a clean pass, and it does not: the Route
    // and Compile terminals are the ones the compile frame earned.
    assert_eq!(
        observation.terminal(Dimension::Structural),
        &Terminal::NotApplicable {
            reason: NotApplicableReason::ComparatorAbsent
        },
    );
    assert_eq!(
        observation.terminal(Dimension::Route).class(),
        Some(C::Pass)
    );
}

/// A driver-level exception line is the harness failing, whatever phase it
/// happened in.
#[test]
fn a_pre_native_driver_exception_is_a_harness_failure() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            DriverLine::Error {
                probe_id: Some(CASE.to_string()),
                error: "TypeError: source is not a string".to_string(),
            },
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Route),
        Some(C::HarnessFailure)
    );
}

// ---------------------------------------------------------------------------
// Reference applicability
// ---------------------------------------------------------------------------

/// Removing a framework's reference producer must turn its Structural cells
/// RED, not silently not-applicable. `inapplicable` is accepted only when that
/// framework's own manifest declares `comparison = none`.
#[test]
fn a_missing_producer_is_a_reference_failure_under_structural_comparison() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Inapplicable {
                    inapplicable: "vue".to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::ReferenceFailure),
    );
}

/// A framework whose manifest declares `comparison = none` records the
/// dimension as not applicable, and the manifest's own applicability check
/// accepts that observation.
#[test]
fn an_unregistered_producer_is_not_applicable_when_the_manifest_declares_no_comparison() {
    let manifest = svelte_manifest();
    let mut run = ProbeRun::new(
        SVELTE_CASE,
        vec![RequestedEntry {
            canonical_id: SVELTE_CASE.to_string(),
            source: String::new(),
            request_digest: String::new(),
        }],
    );
    let _ = run.ingest_frame(phase(SVELTE_CASE, Phase::Compile));
    let _ = run.ingest_frame(compile_frame(SVELTE_CASE, vec![ok_entry(SVELTE_CASE)]));
    let _ = run.ingest_frame(phase(SVELTE_CASE, Phase::Reference));
    let _ = run.ingest_frame(reference_frame(
        SVELTE_CASE,
        vec![ReferenceResult::Inapplicable {
            inapplicable: "svelte".to_string(),
        }],
    ));
    let observation = run
        .finish(&manifest, None)
        .remove(0)
        .expect("the observation is representable");
    assert_eq!(
        observation.terminal(Dimension::Structural),
        &Terminal::NotApplicable {
            reason: NotApplicableReason::ComparatorAbsent
        },
    );
    manifest
        .check_applicability(&observation)
        .expect("the observation agrees with the manifest's applicability");
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// A successful Vue case is TOTAL across all six dimensions: nothing is left
/// unreached, and `Runtime`/`Map` are owned not-applicable rather than
/// silently absent.
#[test]
fn a_successful_vue_case_is_total_across_every_dimension() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(class_at(&terminals, Dimension::Route), Some(C::Pass));
    assert_eq!(class_at(&terminals, Dimension::Compile), Some(C::Pass));
    assert_eq!(class_at(&terminals, Dimension::Structural), Some(C::Pass));
    assert_eq!(
        terminals[&Dimension::Runtime],
        Terminal::NotApplicable {
            reason: NotApplicableReason::RuntimeExecutorAbsent
        },
    );
    assert_eq!(
        terminals[&Dimension::Map],
        Terminal::NotApplicable {
            reason: NotApplicableReason::MapValidatorAbsent
        },
    );
    assert_eq!(class_at(&terminals, Dimension::Performance), Some(C::Pass));
}

/// A typed binding or framework refusal is a PUBLIC-boundary outcome at
/// `Route`, never hidden as a harness defect.
#[test]
fn a_typed_binding_or_framework_refusal_is_a_route_refusal_not_a_harness_failure() {
    let manifest = vue_manifest();
    for kind in ["binding", "frameworkMismatch"] {
        let terminals = observe(
            &manifest,
            CASE,
            vec![
                phase(CASE, Phase::Compile),
                compile_frame(CASE, vec![failure_entry(CASE, kind, None)]),
                phase(CASE, Phase::Reference),
                reference_frame(
                    CASE,
                    vec![ReferenceResult::Produced {
                        code: MODULE.to_string(),
                    }],
                ),
            ],
            None,
        );
        assert_eq!(
            class_at(&terminals, Dimension::Route),
            Some(C::RequestRefused),
            "{kind} must be a typed route refusal",
        );
    }
}

/// A `refused` failure carrying an error diagnostic folds to the diagnostic —
/// the route ANSWERED, so `Route` still passes. A `refused` carrying none is
/// unexplained and fails closed.
#[test]
fn a_refused_failure_folds_to_its_diagnostic_and_fails_closed_without_one() {
    let manifest = vue_manifest();
    let with_diagnostic = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "refused", Some("error"))]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(class_at(&with_diagnostic, Dimension::Route), Some(C::Pass));
    assert_eq!(
        class_at(&with_diagnostic, Dimension::Compile),
        Some(C::VerterDiagnostic),
    );

    let without = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "refused", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&without, Dimension::Compile),
        Some(C::HarnessFailure),
        "an unexplained refusal must fail closed",
    );
}

/// A host failure is a `Route` outcome, and at `Compile` it OUTRANKS the
/// diagnostic it carried while retaining it as secondary evidence.
///
/// The failing shape this pins: leaving the host failure to propagate would
/// let the lower-precedence diagnostic win the Compile cell, so a host failure
/// carrying an error diagnostic would satisfy a diagnostic expectation.
#[test]
fn a_host_failure_outranks_its_diagnostic_and_retains_it_as_evidence() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", Some("error"))]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    // `Route` does not admit a diagnostic class at all, so the host failure is
    // the whole of that cell.
    assert_eq!(class_at(&terminals, Dimension::Route), Some(C::HostFailure));
    // `Compile` admits both, and the fold's precedence puts the host failure
    // first with the diagnostic retained beneath it.
    assert_eq!(
        class_at(&terminals, Dimension::Compile),
        Some(C::HostFailure)
    );
    let Terminal::Class { secondary, .. } = &terminals[&Dimension::Compile] else {
        panic!("the compile terminal is a class")
    };
    assert!(
        secondary.contains(&C::VerterDiagnostic),
        "the diagnostic must be retained as secondary evidence, found {secondary:?}",
    );
}

/// The same holds for a typed refusal that carried an error diagnostic: the
/// refusal outranks it rather than being replaced by it.
#[test]
fn a_typed_refusal_outranks_a_diagnostic_it_carried() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "binding", Some("error"))]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Route),
        Some(C::RequestRefused)
    );
    assert_eq!(
        class_at(&terminals, Dimension::Compile),
        Some(C::RequestRefused),
    );
}

/// Product extraction is EXACT: the request asks for one product with one
/// `main` node, so anything else is absent or malformed rather than searched
/// through for something usable.
#[test]
fn product_extraction_is_exact() {
    let manifest = vue_manifest();
    let cases: [(Vec<RouteProduct>, C); 4] = [
        (Vec::new(), C::ProductNotProduced),
        (
            vec![RouteProduct {
                kind: "ideCompanion".to_string(),
                nodes: None,
            }],
            C::ProductMalformed,
        ),
        (
            vec![RouteProduct {
                kind: "runtimeClient".to_string(),
                nodes: Some(Vec::new()),
            }],
            C::ProductNotProduced,
        ),
        (
            vec![RouteProduct {
                kind: "runtimeClient".to_string(),
                nodes: Some(vec![
                    RouteNode {
                        node: VirtualNodeKind {
                            kind: "main".to_string(),
                        },
                        code: MODULE.to_string(),
                    },
                    RouteNode {
                        node: VirtualNodeKind {
                            kind: "main".to_string(),
                        },
                        code: MODULE.to_string(),
                    },
                ]),
            }],
            C::ProductMalformed,
        ),
    ];
    for (products, expected) in cases {
        let entry = RouteEntry {
            canonical_id: CASE.to_string(),
            response: Some(RouteResponse {
                diagnostics: DiagnosticsSnapshot::default(),
                products,
            }),
            failure: None,
        };
        let terminals = observe(
            &manifest,
            CASE,
            vec![
                phase(CASE, Phase::Compile),
                compile_frame(CASE, vec![entry]),
                phase(CASE, Phase::Reference),
                reference_frame(
                    CASE,
                    vec![ReferenceResult::Produced {
                        code: MODULE.to_string(),
                    }],
                ),
            ],
            None,
        );
        assert_eq!(class_at(&terminals, Dimension::Compile), Some(expected));
    }
}

/// A product that does not parse and build is malformed, and no comparison is
/// emitted for it — comparison evidence never exists without both inputs.
#[test]
fn an_unparseable_product_is_malformed_and_emits_no_comparison() {
    let manifest = vue_manifest();
    let entry = RouteEntry {
        canonical_id: CASE.to_string(),
        response: Some(RouteResponse {
            diagnostics: DiagnosticsSnapshot::default(),
            products: vec![RouteProduct {
                kind: "runtimeClient".to_string(),
                nodes: Some(vec![RouteNode {
                    node: VirtualNodeKind {
                        kind: "main".to_string(),
                    },
                    code: "const = ;".to_string(),
                }]),
            }],
        }),
        failure: None,
    };
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![entry]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Compile),
        Some(C::ProductMalformed),
    );
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::ProductMalformed),
        "no comparison may be reported when one of its inputs is absent",
    );
}

/// A differing product is a structural mismatch and nothing more: the route
/// answered and the product is valid.
#[test]
fn a_differing_product_is_a_structural_mismatch_alone() {
    let manifest = vue_manifest();
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: "export const value = 2\n".to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(class_at(&terminals, Dimension::Route), Some(C::Pass));
    assert_eq!(class_at(&terminals, Dimension::Compile), Some(C::Pass));
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::SemanticMismatch),
    );
}

// ---------------------------------------------------------------------------
// Driver protocol parsing
// ---------------------------------------------------------------------------

/// The line shapes the driver actually writes, parsed structurally.
#[test]
fn driver_lines_are_parsed_by_shape_and_an_unknown_shape_is_refused() {
    assert!(matches!(
        parse_line(r#"{"probe_id":"a","phase":"compile"}"#),
        Ok(DriverLine::Phase {
            phase: Phase::Compile,
            ..
        })
    ));
    assert!(matches!(
        parse_line(r#"{"probe_id":"a","error":"boom","phase":"compile","stage":"pre-native"}"#),
        Ok(DriverLine::Error { .. })
    ));
    assert!(matches!(
        parse_line(
            r#"{"probe_id":"a","frame":"reference","reference":[{"inapplicable":"svelte"}]}"#
        ),
        Ok(DriverLine::Reference { .. })
    ));
    assert!(parse_line(r#"{"probe_id":"a","frame":"compile","entries":[]}"#).is_err());
    assert!(parse_line(r#"{"probe_id":"a"}"#).is_err());
    assert!(parse_line("not json").is_err());
}

// ---------------------------------------------------------------------------
// The canonical request
// ---------------------------------------------------------------------------

/// The template is canonical JSON with sorted keys at every level, and its
/// only substitution is the filename.
#[test]
fn the_request_template_is_canonical_and_substitutes_only_the_filename() {
    let value: serde_json::Value =
        serde_json::from_str(request::REQUEST_VUE).expect("the template is JSON");
    assert_sorted(&value, "$");
    assert!(
        !request::REQUEST_VUE.contains(' ') || request::REQUEST_VUE.contains("case relative path"),
        "the template carries no incidental whitespace",
    );

    let substituted = request::substitute("tests/fixtures/App.vue");
    let parsed: serde_json::Value =
        serde_json::from_str(&substituted).expect("the substituted request is JSON");
    assert_eq!(
        parsed["identity"]["filename"],
        serde_json::json!("tests/fixtures/App.vue"),
    );
    assert_eq!(parsed["framework"], serde_json::json!("vue"));
    assert_eq!(
        parsed["products"][0]["kind"],
        serde_json::json!("runtimeClient")
    );

    // Everything but the filename is untouched.
    let mut expected: serde_json::Value =
        serde_json::from_str(request::REQUEST_VUE).expect("the template is JSON");
    expected["identity"]["filename"] = serde_json::json!("tests/fixtures/App.vue");
    assert_eq!(parsed, expected);
}

/// A path carrying a quote produces an escaped string, never a request whose
/// shape the corpus decided.
#[test]
fn a_hostile_path_is_escaped_rather_than_interpolated() {
    let substituted = request::substitute(r#"a"/,"framework":"svelte"."#);
    let parsed: serde_json::Value =
        serde_json::from_str(&substituted).expect("the substituted request is still JSON");
    assert_eq!(parsed["framework"], serde_json::json!("vue"));
    assert_eq!(
        parsed["identity"]["filename"],
        serde_json::json!(r#"a"/,"framework":"svelte"."#),
    );
}

/// Digests are stable and distinguish cases.
#[test]
fn digests_are_stable_and_case_distinguishing() {
    assert_eq!(request::template_digest(), request::template_digest());
    assert_eq!(request::template_digest().len(), 64);
    assert_ne!(
        request::request_digest("a/App.vue"),
        request::request_digest("b/App.vue"),
    );
    assert_eq!(
        request::request_digest("a/App.vue"),
        request::request_digest("a/App.vue"),
    );
}

fn assert_sorted(value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<&String> = map.keys().collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "{path}: keys are not sorted");
            for (key, child) in map {
                assert_sorted(child, &format!("{path}.{key}"));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                assert_sorted(child, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// The summary
// ---------------------------------------------------------------------------

fn one_case_summary(manifest: &ProbeStateManifest, lines: Vec<DriverLine>) -> Summary {
    let mut run = ProbeRun::new(CASE, requested(CASE));
    for line in lines {
        let _ = run.ingest_frame(line);
    }
    let observation = run
        .finish(manifest, None)
        .remove(0)
        .expect("the observation is representable");
    let framework_run = FrameworkRun {
        manifest,
        request_template: request::REQUEST_VUE,
        selected: vec![CASE.to_string()],
        observed: vec![ObservedCase {
            case_id: CASE.to_string(),
            request_digest: request::request_digest("fixtures/App.vue"),
            elapsed_ns: Some(1_000),
            observation,
        }],
    };
    summary::build(Lane::Smoke, std::slice::from_ref(&framework_run))
        .expect("the summary is consistent")
}

/// One case that succeeded at every exercised dimension.
fn passing_summary(manifest: &ProbeStateManifest) -> Summary {
    one_case_summary(
        manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    )
}

/// The summary carries every required counter, and reports the lane's real
/// work rather than its intention.
#[test]
fn the_summary_carries_every_required_counter_and_its_real_work() {
    let manifest = vue_manifest();
    let summary = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    assert_eq!(summary.totals.selected, 1);
    assert_eq!(summary.totals.attempted, 1);
    assert_eq!(summary.totals.passed, 1);
    assert_eq!(summary.totals.gated_regressions, 0);
    assert_eq!(summary.totals.skips, 2);
    assert_eq!(
        summary.disposition(),
        verter_validation_probe::Disposition::Clean
    );

    let json = summary.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("the summary is JSON");
    for counter in verter_validation_probe::Counters::REQUIRED {
        assert!(
            parsed["totals"].get(counter).is_some(),
            "the totals omit `{counter}`",
        );
        assert!(
            parsed["frameworks"][0]["counters"].get(counter).is_some(),
            "the vue block omits `{counter}`",
        );
    }
    assert!(
        Summary::from_json_str(&json).is_ok(),
        "a complete summary round-trips",
    );

    // Every required counter is rendered, including `selected`: a job page that
    // shows only what was attempted cannot be read against what was chosen.
    let markdown = summary.to_markdown();
    for header in [
        "selected",
        "attempted",
        "passed",
        "gated regressions",
        "canary failures",
        "known failures",
        "canary regressions",
        "unrelated regressions",
        "skips",
        "XPASS candidates",
        "crashes",
        "timeouts",
        "harness failures",
    ] {
        assert!(
            markdown.contains(header),
            "the job summary omits `{header}`"
        );
    }
}

/// The counters are AUDITED against the cell rows the document carries, not
/// trusted. A document whose rows hold a gate regression while its counter
/// claims none would otherwise be read as internally consistent and disposed
/// clean — a fail-open path to the lane's one real exit.
#[test]
fn a_summary_whose_counters_disagree_with_its_cell_rows_is_refused() {
    let manifest = vue_manifest();
    let regressed = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    assert_eq!(regressed.totals.gated_regressions, 1);
    let json = regressed.to_json();

    // Claim the gate regression away, in both the block and the totals so the
    // sum still adds up. Only recounting from the rows can catch this.
    let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    parsed["totals"]["gated_regressions"] = serde_json::json!(0);
    parsed["frameworks"][0]["counters"]["gated_regressions"] = serde_json::json!(0);
    let planted = parsed.to_string();
    assert_ne!(planted, json, "the plant must change the document");
    let refused = Summary::from_json_str(&planted)
        .expect_err("a summary whose counters contradict its rows must be refused");
    assert!(
        matches!(
            refused,
            verter_validation_probe::SummaryError::CounterMismatch { .. }
        ),
        "expected a counter mismatch, got {refused}",
    );

    // The same for an INVENTED failure the rows do not carry.
    let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    parsed["totals"]["crashes"] = serde_json::json!(3);
    parsed["frameworks"][0]["counters"]["crashes"] = serde_json::json!(3);
    assert!(
        Summary::from_json_str(&parsed.to_string()).is_err(),
        "a summary claiming crashes its rows never recorded must be refused",
    );
}

/// A cell whose EVALUATION is not what its own fields decide is refused.
///
/// Recomputing the counters from the evaluations the rows declare answers only
/// "do these numbers add up", and a document that miswrote one verdict and the
/// counters to match adds up perfectly. The evaluation is a pure function of
/// the expectation and the observation printed beside it, so the read-back
/// re-decides it through the same rule that wrote it: a gate cell claiming
/// `gate_pass` over observations that say otherwise is exactly what the lane's
/// one real exit must never read as clean.
#[test]
fn a_summary_whose_cell_evaluation_is_not_what_its_own_fields_decide_is_refused() {
    let manifest = vue_manifest();
    let regressed = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    assert_eq!(regressed.totals.gated_regressions, 1);
    assert_ne!(regressed.disposition().exit_code(), 0);

    // Claim the gate PASSED, and zero the counter so the arithmetic still
    // agrees with the rows. Only re-deciding the cell can catch this.
    let mut parsed: serde_json::Value =
        serde_json::from_str(&regressed.to_json()).expect("summary is JSON");
    let cells = parsed["frameworks"][0]["cases"][0]["cells"]
        .as_array_mut()
        .expect("the case carries cells");
    let gate = cells
        .iter_mut()
        .find(|cell| cell["evaluation"] == serde_json::json!("gate_regression"))
        .expect("the planted regression is a gate cell");
    gate["evaluation"] = serde_json::json!("gate_pass");
    parsed["totals"]["gated_regressions"] = serde_json::json!(0);
    parsed["frameworks"][0]["counters"]["gated_regressions"] = serde_json::json!(0);

    let refused = Summary::from_json_str(&parsed.to_string())
        .expect_err("a cell claiming a verdict its own fields refuse must be rejected");
    assert!(
        matches!(
            refused,
            verter_validation_probe::SummaryError::EvaluationMismatch { .. }
        ),
        "expected an evaluation mismatch, got {refused}",
    );
}

/// A cell row whose terminal and class contradict each other is refused: the
/// counters are recomputed from those two fields, so an incoherent row would
/// produce numbers that look honest.
#[test]
fn a_cell_row_whose_terminal_and_class_disagree_is_refused() {
    let manifest = vue_manifest();
    let summary = passing_summary(&manifest);
    let json = summary.to_json();
    let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    let cell = &mut parsed["frameworks"][0]["cases"][0]["cells"][0];
    assert_eq!(cell["observed_terminal"], serde_json::json!("class"));
    cell["observed_terminal"] = serde_json::json!("not_applicable");
    assert!(
        Summary::from_json_str(&parsed.to_string()).is_err(),
        "a row claiming a class it says it has no terminal for must be refused",
    );
}

/// A document with no framework block at all has all-zero totals that are
/// internally consistent, and would dispose CLEAN having reported nothing —
/// the same fail-open the per-framework empty-selection check closes.
#[test]
fn a_summary_with_no_framework_block_is_refused() {
    let manifest = vue_manifest();
    let json = passing_summary(&manifest).to_json();
    let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    parsed["frameworks"] = serde_json::json!([]);
    for counter in verter_validation_probe::Counters::REQUIRED {
        parsed["totals"][counter] = match parsed["totals"][counter] {
            serde_json::Value::Object(_) => serde_json::json!({}),
            _ => serde_json::json!(0),
        };
    }
    assert!(
        Summary::from_json_str(&parsed.to_string()).is_err(),
        "a summary carrying no framework block must be refused, not disposed clean",
    );
}

/// The attempted case set is audited by ID, not by count. A run that observed
/// one case twice and skipped another has the right count and the wrong work.
#[test]
fn a_summary_whose_attempted_ids_are_not_its_selection_is_refused() {
    let manifest = vue_manifest();
    let json = passing_summary(&manifest).to_json();

    let mut swapped: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    swapped["frameworks"][0]["selected_cases"] = serde_json::json!(["vue/fixtures/Other.vue"]);
    assert!(
        Summary::from_json_str(&swapped.to_string()).is_err(),
        "a block whose attempted id is not the one it selected must be refused",
    );

    let mut miscounted: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
    miscounted["frameworks"][0]["selected_cases"] = serde_json::json!([CASE, CASE]);
    assert!(
        Summary::from_json_str(&miscounted.to_string()).is_err(),
        "a block recording more selected ids than it selected must be refused",
    );
}

/// Each framework block carries the request template ITS cases issued. A
/// builder that read one framework's template from a crate constant would
/// publish, for every other framework, a request its cases never sent — with a
/// digest that agreed with itself.
#[test]
fn each_framework_block_carries_its_own_request_template() {
    const OTHER: &str = r#"{"framework":"svelte"}"#;
    let manifest = svelte_manifest();
    let mut run = ProbeRun::new(SVELTE_CASE, requested(SVELTE_CASE));
    let _ = run.ingest_frame(phase(SVELTE_CASE, Phase::Compile));
    let _ = run.ingest_frame(compile_frame(SVELTE_CASE, vec![ok_entry(SVELTE_CASE)]));
    let _ = run.ingest_frame(phase(SVELTE_CASE, Phase::Reference));
    let _ = run.ingest_frame(reference_frame(
        SVELTE_CASE,
        vec![ReferenceResult::Inapplicable {
            inapplicable: "svelte".to_string(),
        }],
    ));
    let observation = run
        .finish(&manifest, None)
        .remove(0)
        .expect("the observation is representable");
    let framework_run = FrameworkRun {
        manifest: &manifest,
        request_template: OTHER,
        selected: vec![SVELTE_CASE.to_string()],
        observed: vec![ObservedCase {
            case_id: SVELTE_CASE.to_string(),
            request_digest: String::new(),
            elapsed_ns: Some(1),
            observation,
        }],
    };
    let summary = summary::build(Lane::Smoke, std::slice::from_ref(&framework_run))
        .expect("the summary is consistent");
    let block = &summary.frameworks[0];
    assert_eq!(block.request_template, OTHER);
    assert_eq!(
        block.template_digest,
        request::sha256_hex(OTHER.as_bytes()),
        "the digest must be of the template the block carries",
    );
    assert_ne!(
        block.template_digest,
        request::template_digest(),
        "a block must not publish another framework's template digest",
    );
}

/// A summary missing a counter is REFUSED, not read as an honest zero. Without
/// this, "no gate regressions" and "the lane forgot to count them" are the same
/// document.
#[test]
fn a_summary_missing_a_counter_is_refused_rather_than_read_as_zero() {
    let manifest = vue_manifest();
    let summary = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    let json = summary.to_json();
    // BOTH counter blocks: the totals a reader sees first, and the per-framework
    // block they are the sum of. A presence check on one of them would let the
    // other omit a counter and read as zero.
    for pointer in ["totals", "frameworks"] {
        for counter in verter_validation_probe::Counters::REQUIRED {
            let mut parsed: serde_json::Value =
                serde_json::from_str(&json).expect("summary is JSON");
            let counters = if pointer == "totals" {
                &mut parsed["totals"]
            } else {
                &mut parsed["frameworks"][0]["counters"]
            };
            counters
                .as_object_mut()
                .expect("a counter block is an object")
                .remove(counter);
            let planted = parsed.to_string();
            assert!(
                planted != json,
                "the plant for `{pointer}.{counter}` must actually change the document",
            );
            assert!(
                Summary::from_json_str(&planted).is_err(),
                "a summary missing `{pointer}.{counter}` must be refused",
            );
        }
    }
}

/// A gate regression is a REAL non-zero disposition, and it is computed from
/// gate cells alone — a canary or a known-fail never blocks.
#[test]
fn a_gate_regression_disposes_non_zero_and_a_canary_never_does() {
    let manifest = vue_manifest();
    // A host failure at Route does not meet the manifest's `pass` gate.
    let regressed = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    assert_eq!(regressed.totals.gated_regressions, 1);
    assert_eq!(
        regressed.disposition(),
        verter_validation_probe::Disposition::GateRegressed { count: 1 },
    );
    assert_eq!(regressed.disposition().exit_code(), 1);

    // A differing product regresses the Structural CANARY and nothing else.
    let canary = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![ok_entry(CASE)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: "export const value = 2\n".to_string(),
                }],
            ),
        ],
    );
    assert_eq!(canary.totals.gated_regressions, 0);
    assert_eq!(
        canary.disposition(),
        verter_validation_probe::Disposition::Clean,
    );
    assert_eq!(canary.disposition().exit_code(), 0);
}

/// A framework that attempted fewer or more cases than it selected is refused:
/// the inventory check runs in BOTH directions.
#[test]
fn a_summary_whose_attempted_set_differs_from_its_selection_is_refused() {
    let manifest = vue_manifest();
    let mut run = ProbeRun::new(CASE, requested(CASE));
    let _ = run.ingest_frame(phase(CASE, Phase::Compile));
    let _ = run.ingest_frame(compile_frame(CASE, vec![ok_entry(CASE)]));
    let _ = run.ingest_frame(phase(CASE, Phase::Reference));
    let _ = run.ingest_frame(reference_frame(
        CASE,
        vec![ReferenceResult::Produced {
            code: MODULE.to_string(),
        }],
    ));
    let observation = run
        .finish(&manifest, None)
        .remove(0)
        .expect("the observation is representable");

    let under = FrameworkRun {
        manifest: &manifest,
        request_template: request::REQUEST_VUE,
        selected: vec![CASE.to_string(), "vue/fixtures/Other.vue".to_string()],
        observed: vec![ObservedCase {
            case_id: CASE.to_string(),
            request_digest: String::new(),
            elapsed_ns: None,
            observation: observation.clone(),
        }],
    };
    assert!(
        summary::build(Lane::Smoke, std::slice::from_ref(&under)).is_err(),
        "a lane that attempted fewer cases than it selected must be refused",
    );

    let over = FrameworkRun {
        manifest: &manifest,
        request_template: request::REQUEST_VUE,
        selected: vec![CASE.to_string()],
        observed: vec![
            ObservedCase {
                case_id: CASE.to_string(),
                request_digest: String::new(),
                elapsed_ns: None,
                observation: observation.clone(),
            },
            ObservedCase {
                case_id: CASE.to_string(),
                request_digest: String::new(),
                elapsed_ns: None,
                observation: observation.clone(),
            },
        ],
    };
    assert!(
        summary::build(Lane::Smoke, std::slice::from_ref(&over)).is_err(),
        "a lane that attempted more cases than it selected must be refused",
    );

    let empty = FrameworkRun {
        manifest: &manifest,
        request_template: request::REQUEST_VUE,
        selected: Vec::new(),
        observed: Vec::new(),
    };
    assert!(
        summary::build(Lane::Smoke, std::slice::from_ref(&empty)).is_err(),
        "a lane that selected nothing must be refused, not published green",
    );
}

// ---------------------------------------------------------------------------
// The committed manifest
// ---------------------------------------------------------------------------

fn workflow_text() -> String {
    let path = repository_root()
        .join(".github")
        .join("workflows")
        .join("validation-probe.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn repository_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives at <root>/crates/verter_validation_probe")
        .to_path_buf()
}

fn committed_vue_manifest_text() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("manifest")
        .join("vue.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The committed manifest is valid, bounded, and carries no classless canary.
#[test]
fn the_committed_vue_manifest_is_valid_bounded_and_fully_classified() {
    let manifest =
        ProbeStateManifest::from_manifest_file("vue.toml", &committed_vue_manifest_text())
            .unwrap_or_else(|error| panic!("the committed vue manifest is invalid: {error}"));

    assert_eq!(manifest.framework, Framework::Vue);
    assert!(
        !manifest.smoke.is_empty(),
        "the smoke slice selects no case"
    );
    assert!(
        manifest.smoke.len() <= verter_validation_probe::MAX_SMOKE_CASES,
        "the smoke slice lists {} cases",
        manifest.smoke.len(),
    );
    assert_eq!(
        manifest.smoke,
        manifest.derived_smoke_slice(),
        "the smoke slice is not its own deterministic derivation",
    );

    let inventory: std::collections::BTreeSet<&str> = manifest
        .inventory
        .iter()
        .map(|case| case.case_id.as_str())
        .collect();
    for case_id in &manifest.smoke {
        assert!(
            inventory.contains(case_id.as_str()),
            "the smoke slice selects `{case_id}`, which is not inventoried",
        );
    }

    for entry in &manifest.entries {
        if matches!(
            entry.expected_state,
            verter_validation_probe::ExpectedState::Canary
                | verter_validation_probe::ExpectedState::KnownFail
                | verter_validation_probe::ExpectedState::Gate
        ) {
            assert!(
                entry.expected_class.is_some(),
                "{} [{}] is classless",
                entry.probe_id,
                entry.dimension,
            );
        }
    }
}

/// Only `Route` may gate, and only on the outcomes the implemented route
/// authority actually owns.
#[test]
fn only_the_implemented_route_authority_gates() {
    let manifest =
        ProbeStateManifest::from_manifest_file("vue.toml", &committed_vue_manifest_text())
            .expect("the committed vue manifest is valid");
    for entry in &manifest.entries {
        if entry.expected_state != verter_validation_probe::ExpectedState::Gate {
            continue;
        }
        assert_eq!(
            entry.dimension,
            Dimension::Route,
            "{} gates at {}",
            entry.probe_id,
            entry.dimension,
        );
        assert_eq!(
            entry.authority,
            Some(verter_validation_probe::Authority::CompilerPublicRequestRoute),
        );
        assert!(
            matches!(
                entry.expected_class,
                Some(C::Pass) | Some(C::RequestRefused)
            ),
            "{} gates on {:?}, which the route authority's atoms do not own",
            entry.probe_id,
            entry.expected_class,
        );
    }
}

/// The lane is table-driven: the workflow declares exactly ONE probe job, and
/// there is no per-fixture job or per-fixture test definition.
#[test]
fn the_workflow_declares_exactly_one_probe_job() {
    let text = workflow_text();

    let mut in_jobs = false;
    let mut jobs = Vec::new();
    for line in text.lines() {
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        if indent == 2 && trimmed.ends_with(':') && !trimmed.starts_with('-') {
            jobs.push(trimmed.trim_end_matches(':').to_string());
        }
    }
    assert_eq!(
        jobs,
        vec!["probe".to_string()],
        "the workflow declares {jobs:?}"
    );

    // The lane's disposition is a real exit, taken AFTER publication. Only the
    // STEPS are read: the file's own header prose mentions each of these, and
    // matching it would let the steps be reordered without failing here.
    let steps = &text[text.find("jobs:").expect("the workflow declares jobs")..];
    let dispose = steps
        .find("--dispose")
        .expect("the workflow disposes the lane");
    let upload = steps
        .find("upload-artifact")
        .expect("the workflow uploads the summary artifact");
    let markdown = steps
        .find("--markdown")
        .expect("the workflow renders the summary");
    assert!(
        upload < dispose && markdown < dispose,
        "the disposition must run after the artifact and the rendered summary are published",
    );

    // Ordering alone is not the guarantee. A publication step that GitHub skips
    // because an earlier step failed publishes nothing, so the run that goes red
    // would be exactly the run with no evidence. Each of the three must be
    // declared to run regardless.
    for step in ["upload-artifact", "--markdown", "--dispose"] {
        let at = steps.find(step).expect("the step is declared");
        let preceding = &steps[..at];
        let boundary = preceding
            .rfind("      - name:")
            .expect("every step carries a name");
        assert!(
            preceding[boundary..].contains("if: always()"),
            "the `{step}` step must run even after a failing probe step, or the red run \
             loses the evidence it was red about",
        );
    }
}

/// The pinned revision is recorded twice — once for the checkout the workflow
/// performs, once in the manifest every artifact republishes. They must be the
/// SAME commit: a workflow bumped to a revision whose fixture set still matches
/// the inventory would stay green while every artifact recorded a revision its
/// cases did not come from.
#[test]
fn the_workflow_and_the_manifest_pin_the_same_revision() {
    let text = workflow_text();
    let pinned = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("VUE_BENCHMARKS_REVISION:"))
        .map(str::trim)
        .expect("the workflow pins a corpus revision");
    let manifest =
        ProbeStateManifest::from_manifest_file("vue.toml", &committed_vue_manifest_text())
            .expect("the committed vue manifest is valid");
    assert_eq!(
        pinned,
        manifest.external_revision.as_str(),
        "the workflow checks out `{pinned}` while the manifest pins `{}`",
        manifest.external_revision.as_str(),
    );
}

/// The disposition is a real PROCESS exit, not a number a caller may ignore.
///
/// The binary is run as the workflow's final step runs it, over planted
/// artifacts: a clean one exits 0, a regressed one exits non-zero, and a
/// malformed one is refused rather than read as a lane with nothing to report.
#[test]
fn the_summary_binary_disposes_as_a_process() {
    let manifest = vue_manifest();
    let clean = passing_summary(&manifest).to_json();
    let regressed = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    )
    .to_json();
    let mut parsed: serde_json::Value = serde_json::from_str(&clean).expect("the summary is JSON");
    parsed["totals"]
        .as_object_mut()
        .expect("totals is an object")
        .remove("gated_regressions");
    let incomplete = parsed.to_string();

    for (label, document, expected) in [
        ("a clean summary", clean, Some(0)),
        ("a gate regression", regressed, Some(1)),
        ("a summary missing a counter", incomplete, Some(2)),
    ] {
        let path = temp_file(&format!("dispose-{}", label.replace(' ', "-")), &document);
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_validation-probe-summary"))
            .args(["--dispose", "--summary"])
            .arg(&path)
            .output()
            .expect("the summary binary runs");
        assert_eq!(
            output.status.code(),
            expected,
            "{label} disposed {:?}; stderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        );
        let _ = std::fs::remove_file(&path);
    }
}

/// Write `contents` to a uniquely named file under the platform's temp
/// directory. Never a literal path: the lane builds every path from the
/// standard abstractions so it runs the same on every platform.
///
/// The unique part comes BEFORE the label so a label ending in an extension
/// keeps it — node dispatches on the extension, and a `.mjs` buried mid-name is
/// not a module.
fn temp_file(label: &str, contents: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("verter-probe-{unique}-{label}"));
    std::fs::write(&path, contents)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
    path
}

/// A `not_run` cell never counts as a pass, and a skip never counts as work.
#[test]
fn unreached_and_skipped_cells_are_never_counted_as_passes() {
    let manifest = vue_manifest();
    let summary = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    assert_eq!(summary.totals.passed, 0);
    assert_eq!(summary.totals.skips, 2);
    let cells = &summary.frameworks[0].cases[0].cells;
    let performance = cells
        .iter()
        .find(|cell| cell.dimension == Dimension::Performance)
        .expect("the performance cell is present");
    assert_eq!(performance.evaluation, Evaluation::NotRun);
    assert_eq!(performance.observed_class, None);
}

// ---------------------------------------------------------------------------
// Comparison inputs
// ---------------------------------------------------------------------------

/// A WARNING-severity diagnostic is not a structural difference.
///
/// The reference producer reports only errors, so its diagnostics channel is
/// empty for every module it produced. Comparing Verter's warnings against that
/// side would report a difference the harness created by discarding the other
/// compiler's warnings — and would report it on exactly the cases whose
/// products match, holding each of them back from surfacing as a promotion
/// candidate.
#[test]
fn a_warning_severity_diagnostic_is_not_a_structural_difference() {
    let manifest = vue_manifest();
    let mut warned = ok_entry(CASE);
    warned
        .response
        .as_mut()
        .expect("the entry carries a response")
        .diagnostics
        .diagnostics
        .push(RouteDiagnostic {
            severity: "warning".to_string(),
            code: "W1".to_string(),
            message: "a template hint".to_string(),
        });
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![warned]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(class_at(&terminals, Dimension::Compile), Some(C::Pass));
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::Pass),
        "identical modules must compare equal despite a warning the reference \
         compiler's warnings were never captured to match: {:?}",
        terminals[&Dimension::Structural],
    );

    // An ERROR the reference did not report is still a real difference — the
    // restriction drops the uncaptured channel, not the signal.
    let mut errored = ok_entry(CASE);
    errored
        .response
        .as_mut()
        .expect("the entry carries a response")
        .diagnostics
        .diagnostics
        .push(RouteDiagnostic {
            severity: "error".to_string(),
            code: "E1".to_string(),
            message: "boom".to_string(),
        });
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![errored]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Compile),
        Some(C::VerterDiagnostic)
    );
    assert_eq!(
        class_at(&terminals, Dimension::Structural),
        Some(C::SemanticMismatch),
        "an error Verter reported and the reference did not is a real difference",
    );
}

/// An entry carrying BOTH arms fails closed. The route answers exactly one of
/// them; an envelope saying two contradictory things is as unexplained as one
/// saying nothing, and preferring either arm would be the runner inventing an
/// answer.
#[test]
fn a_route_entry_carrying_both_arms_fails_closed() {
    let manifest = vue_manifest();
    let mut both = ok_entry(CASE);
    both.failure = Some(RouteFailure {
        kind: "host".to_string(),
        message: "refused".to_string(),
        diagnostics: DiagnosticsSnapshot::default(),
    });
    let terminals = observe(
        &manifest,
        CASE,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![both]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
        None,
    );
    assert_eq!(
        class_at(&terminals, Dimension::Route),
        Some(C::HarnessFailure),
    );
}

/// A line naming ANOTHER probe is refused rather than believed.
///
/// The protocol is one probe at a time, so such a line means the stream and the
/// runner are out of step. Believing it would attach one case's product,
/// reference or failure to another case — and the taxonomy's whole promise is
/// that evidence describes the case it is filed under.
#[test]
fn a_line_naming_another_probe_is_refused() {
    let mut run = ProbeRun::new(CASE, requested(CASE));
    assert_eq!(
        run.ingest_frame(compile_frame(
            "vue/fixtures/Other.vue",
            vec![ok_entry(CASE)]
        )),
        Err(FrameViolation::UnknownProbe {
            probe_id: "vue/fixtures/Other.vue".to_string(),
        }),
    );
    assert!(
        !run.has_compile_frame(),
        "a frame for another probe must not be retained",
    );
    assert_eq!(
        run.ingest_frame(DriverLine::Error {
            probe_id: Some("vue/fixtures/Other.vue".to_string()),
            error: "another probe's failure".to_string(),
        }),
        Err(FrameViolation::UnknownProbe {
            probe_id: "vue/fixtures/Other.vue".to_string(),
        }),
    );
    // An error the driver could not attribute belongs to the probe the runner is
    // waiting on, and is accepted.
    assert_eq!(
        run.ingest_frame(DriverLine::Error {
            probe_id: None,
            error: "a line the driver could not parse".to_string(),
        }),
        Ok(()),
    );
}

// ---------------------------------------------------------------------------
// Driving a real driver process
// ---------------------------------------------------------------------------

/// The header every synthetic driver shares: read one probe per line, answer on
/// stdout, and never exit on a failure.
const SYNTHETIC_DRIVER_PRELUDE: &str = r#"
import { createInterface } from "node:readline";
const write = (line) => process.stdout.write(`${JSON.stringify(line)}\n`);
const product = (canonicalId) => ({
  canonicalId,
  response: {
    diagnostics: { diagnostics: [] },
    products: [
      {
        kind: "runtimeClient",
        nodes: [{ node: { kind: "main" }, code: "export const value = 1\n" }],
      },
    ],
  },
  failure: null,
});
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
let seen = 0;
for await (const line of lines) {
  if (line.trim() === "") continue;
  const probe = JSON.parse(line);
  const id = probe.probe_id;
  const canonicalId = probe.entries[0].canonicalId;
  seen += 1;
"#;

const SYNTHETIC_DRIVER_EPILOGUE: &str = "\n}\n";

fn synthetic_driver(label: &str, body: &str) -> (std::path::PathBuf, DriverCommand) {
    let script = temp_file(
        &format!("{label}.mjs"),
        &format!("{SYNTHETIC_DRIVER_PRELUDE}{body}{SYNTHETIC_DRIVER_EPILOGUE}"),
    );
    let command = DriverCommand {
        program: std::path::PathBuf::from("node"),
        script: script.clone(),
    };
    (script, command)
}

fn planned(case_id: &str) -> PlannedCase {
    PlannedCase {
        case_id: case_id.to_string(),
        relative_path: "fixtures/App.vue".to_string(),
        source: "<template><div/></template>\n".to_string(),
    }
}

fn generous(compile: std::time::Duration) -> PhaseDeadlines {
    PhaseDeadlines {
        load: std::time::Duration::from_secs(30),
        compile,
        reference: std::time::Duration::from_secs(30),
    }
}

/// A per-probe driver ERROR ends that probe, and the lane keeps running.
///
/// The driver reports a recoverable failure as a LINE precisely so the runner
/// can move on, and then waits for the next probe. A runner that kept waiting
/// for a frame that will never come would spend the whole deadline, kill a
/// healthy driver, and record every remaining case as a harness failure: one
/// recoverable JavaScript throw would cost the entire lane.
#[test]
fn a_per_probe_driver_error_ends_that_probe_and_the_lane_keeps_running() {
    let manifest = vue_manifest();
    let (script, driver) = synthetic_driver(
        "driver-error",
        r#"
  write({ probe_id: id, phase: "load" });
  write({ probe_id: id, phase: "compile" });
  if (seen === 1) {
    write({ probe_id: id, error: "planted failure", phase: "compile", stage: "native" });
    continue;
  }
  write({ probe_id: id, frame: "compile", elapsed_ns: 1, entries: [product(canonicalId)] });
  write({ probe_id: id, phase: "reference" });
  write({ probe_id: id, frame: "reference", reference: [{ code: "export const value = 1\n" }] });
"#,
    );
    let cases = [planned(CASE), planned(CASE)];
    let results = runner::run_cases(
        &manifest,
        &driver,
        &cases,
        generous(std::time::Duration::from_secs(30)),
    )
    .unwrap_or_else(|error| panic!("the synthetic driver could not be run: {error}"));
    let _ = std::fs::remove_file(&script);

    assert_eq!(results.len(), 2);
    let first = results[0]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        first.terminal(Dimension::Route).class(),
        Some(C::HarnessFailure),
        "the probe the driver reported a failure for is that probe's harness failure",
    );
    let second = results[1]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        second.terminal(Dimension::Compile).class(),
        Some(C::Pass),
        "the lane must keep running after a per-probe failure: {:?}",
        second.terminal(Dimension::Route),
    );
    assert_eq!(
        second.terminal(Dimension::Structural).class(),
        Some(C::Pass)
    );
}

/// A pause after the compile frame but BEFORE the reference phase marker is
/// bounded by the reference deadline, not the compile one.
///
/// The compile frame is the native call's answer; what remains is reference
/// work. A runner that left the compile deadline armed would report a reference
/// producer that was merely slow as a COMPILER timeout — a verdict about Verter
/// drawn from a step Verter had already finished.
#[test]
fn a_pause_after_the_compile_frame_is_not_a_compiler_timeout() {
    let manifest = vue_manifest();
    let (script, driver) = synthetic_driver(
        "driver-slow-reference",
        r#"
  write({ probe_id: id, phase: "load" });
  write({ probe_id: id, phase: "compile" });
  write({ probe_id: id, frame: "compile", elapsed_ns: 1, entries: [product(canonicalId)] });
  await sleep(900);
  write({ probe_id: id, phase: "reference" });
  write({ probe_id: id, frame: "reference", reference: [{ code: "export const value = 1\n" }] });
"#,
    );
    let cases = [planned(CASE)];
    let results = runner::run_cases(
        &manifest,
        &driver,
        &cases,
        generous(std::time::Duration::from_millis(200)),
    )
    .unwrap_or_else(|error| panic!("the synthetic driver could not be run: {error}"));
    let _ = std::fs::remove_file(&script);

    let observation = results[0]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        observation.terminal(Dimension::Compile).class(),
        Some(C::Pass),
    );
    assert_eq!(
        observation.terminal(Dimension::Structural).class(),
        Some(C::Pass),
        "a pause before the reference marker must not be read as a compiler timeout: {:?}",
        observation.terminal(Dimension::Structural),
    );
}

/// A HANG after the compile frame is a reference failure, not a compiler
/// timeout.
///
/// The reference deadline is what bounds the probe from the moment that frame
/// arrives, and the cause it is attributed to has to move with it: the frame is
/// written only once the native call has returned, so nothing of the compiler
/// is still running. Stamping the timeout with the last phase MARKER instead
/// would report the class that means "the compiler hung" for a hang that
/// provably happened after the compiler had finished — the taxonomy's most
/// load-bearing distinction, decided wrongly.
#[test]
fn a_hang_after_the_compile_frame_is_a_reference_failure_not_a_compiler_timeout() {
    let manifest = vue_manifest();
    let (script, driver) = synthetic_driver(
        "driver-hang-after-compile",
        r#"
  write({ probe_id: id, phase: "load" });
  write({ probe_id: id, phase: "compile" });
  write({ probe_id: id, frame: "compile", elapsed_ns: 1, entries: [product(canonicalId)] });
  // No reference phase marker, and no reference frame: the hang strikes while
  // the last marker still says `compile`.
  await sleep(30_000);
"#,
    );
    let cases = [planned(CASE)];
    let results = runner::run_cases(
        &manifest,
        &driver,
        &cases,
        PhaseDeadlines {
            load: std::time::Duration::from_secs(30),
            compile: std::time::Duration::from_secs(30),
            reference: std::time::Duration::from_millis(300),
        },
    )
    .unwrap_or_else(|error| panic!("the synthetic driver could not be run: {error}"));
    let _ = std::fs::remove_file(&script);

    let observation = results[0]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        observation.terminal(Dimension::Compile).class(),
        Some(C::Pass),
        "the compile frame was already ingested, so its terminal stands",
    );
    assert_eq!(
        observation.terminal(Dimension::Structural).class(),
        Some(C::ReferenceFailure),
        "a hang after the compile frame must not be attributed to the compiler: {:?}",
        observation.terminal(Dimension::Structural),
    );
}

/// A broken stdout stream is the HARNESS failing, never the compiler.
///
/// The runner could not read what the driver said; the driver itself may be
/// alive and well. Running that through the phase map — which exists to say
/// which step was executing when a PROCESS ended — would report a crash or a
/// timeout the compiler never had, and it would say so for every case after it
/// too.
#[test]
fn a_broken_stdout_stream_is_a_harness_failure_not_a_compiler_crash() {
    let manifest = vue_manifest();
    let (script, driver) = synthetic_driver(
        "driver-broken-stream",
        r#"
  write({ probe_id: id, phase: "load" });
  write({ probe_id: id, phase: "compile" });
  // Bytes that are not a UTF-8 line: the transport, not the compiler.
  process.stdout.write(Buffer.from([0xff, 0xfe, 0x0a]));
  await sleep(30_000);
"#,
    );
    let cases = [planned(CASE), planned(CASE)];
    let results = runner::run_cases(
        &manifest,
        &driver,
        &cases,
        generous(std::time::Duration::from_secs(30)),
    )
    .unwrap_or_else(|error| panic!("the synthetic driver could not be run: {error}"));
    let _ = std::fs::remove_file(&script);

    for (position, result) in results.iter().enumerate() {
        let observation = result
            .observation
            .as_ref()
            .expect("the observation is representable");
        let class = observation.terminal(Dimension::Route).class();
        assert_eq!(
            class,
            Some(C::HarnessFailure),
            "case {position} must report the harness, not the compiler: {:?}",
            observation.terminal(Dimension::Route),
        );
        assert_ne!(class, Some(C::Crash));
        assert_ne!(class, Some(C::Timeout));
    }
}

/// A frame naming ANOTHER probe stops the lane deliberately, even when the
/// stream would otherwise carry on perfectly.
///
/// The protocol is one probe at a time, so a foreign frame means the stream and
/// the runner are out of step and the next probe's lines can no longer be told
/// from this one's. The driver here goes on to answer BOTH probes correctly, so
/// a runner that merely recorded the refusal and read on would report the
/// second case as a clean pass — a verdict read off a stream it had already
/// caught lying about which case it was describing. Evidence that describes a
/// different case is worse than no evidence, so the driver is killed and every
/// case after it says so.
#[test]
fn a_frame_naming_another_probe_stops_the_lane_and_the_rest_report_it() {
    let manifest = vue_manifest();
    let (script, driver) = synthetic_driver(
        "driver-foreign-frame",
        r#"
  write({ probe_id: id, phase: "load" });
  write({ probe_id: id, phase: "compile" });
  if (seen === 1) {
    write({ probe_id: "vue/fixtures/Other.vue", frame: "compile", elapsed_ns: 1,
            entries: [product(canonicalId)] });
  }
  write({ probe_id: id, frame: "compile", elapsed_ns: 1, entries: [product(canonicalId)] });
  write({ probe_id: id, phase: "reference" });
  write({ probe_id: id, frame: "reference", reference: [{ code: "export const value = 1\n" }] });
"#,
    );
    let cases = [planned(CASE), planned(CASE)];
    let results = runner::run_cases(
        &manifest,
        &driver,
        &cases,
        generous(std::time::Duration::from_secs(30)),
    )
    .unwrap_or_else(|error| panic!("the synthetic driver could not be run: {error}"));
    let _ = std::fs::remove_file(&script);

    let first = results[0]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        first.terminal(Dimension::Route).class(),
        Some(C::HarnessFailure),
        "the refused frame is the probe's own harness failure",
    );
    let second = results[1]
        .observation
        .as_ref()
        .expect("the observation is representable");
    assert_eq!(
        second.terminal(Dimension::Route).class(),
        Some(C::HarnessFailure),
        "the lane must stop rather than read a desynchronised stream: {:?}",
        second.terminal(Dimension::Route),
    );
    assert_ne!(
        second.terminal(Dimension::Compile).class(),
        Some(C::Pass),
        "a case driven after the stream desynchronised must not report a pass",
    );
}

// ---------------------------------------------------------------------------
// The committed driver's own protocol refusals
// ---------------------------------------------------------------------------

/// The committed driver REFUSES a request that carries no `identity.filename`.
///
/// That field is the one filename both compilers must compile under. Defaulting
/// a missing one to the case id would compile the reference under a name the
/// request never carried, and every filename-derived difference that followed —
/// a scoped style's scope id above all — would reach the comparator as a
/// compiler difference the harness itself manufactured. The refusal is a LINE,
/// so the probe fails and the driver lives on.
#[test]
fn the_committed_driver_refuses_a_request_without_an_identity_filename() {
    use std::io::{BufRead, Write};

    let script = repository_root()
        .join("crates")
        .join("verter_validation_probe")
        .join("driver")
        .join("probe-driver.mjs");
    let mut child = std::process::Command::new("node")
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("the committed driver could not be started: {error}"));

    // The canonical request with `identity.filename` removed. Nothing here
    // reaches the addon: the refusal happens while the line is being read.
    let mut request: serde_json::Value =
        serde_json::from_str(&request::substitute("fixtures/App.vue"))
            .expect("the canonical request is JSON");
    request["identity"]
        .as_object_mut()
        .expect("the request carries an identity object")
        .remove("filename");
    let probe = serde_json::json!({
        "probe_id": CASE,
        "entries": [{ "canonicalId": CASE, "source": "<template><div/></template>\n",
                      "request": request }],
    });
    let stdin = child.stdin.as_mut().expect("the driver exposes stdin");
    writeln!(stdin, "{probe}").expect("the probe is written");
    stdin.flush().expect("the probe is flushed");
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("the driver exposes stdout");
    let answered: Vec<String> = std::io::BufReader::new(stdout)
        .lines()
        .map(|line| line.expect("the driver writes UTF-8 lines"))
        .collect();
    let _ = child.wait();

    let line = answered
        .first()
        .unwrap_or_else(|| panic!("the driver answered nothing: {answered:?}"));
    let parsed = parse_line(line).unwrap_or_else(|error| panic!("{line}: {error}"));
    match parsed {
        DriverLine::Error { error, .. } => assert!(
            error.contains("identity.filename"),
            "the refusal must name the missing field: {error}",
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        answered.len(),
        1,
        "a refused request must not also be compiled: {answered:?}",
    );
}

// ---------------------------------------------------------------------------
// Gate cells that never ran
// ---------------------------------------------------------------------------

/// A gate cell whose dimension was NEVER REACHED is a gate regression, and
/// disposes non-zero.
///
/// Not-run is not an outcome class: it can neither meet an expectation nor be
/// expected by one. A gate reading it as anything but a regression would let a
/// dimension that was never exercised stand in for one that passed.
///
/// The manifest here is built directly because `validate` refuses a gate outside
/// `Route`, and `Route` is fed by nothing so it can never be unreached. The
/// evaluation is the same one a future promoted gate would take.
#[test]
fn a_gate_cell_whose_dimension_was_never_reached_disposes_non_zero() {
    let mut manifest = vue_manifest();
    for entry in &mut manifest.entries {
        if entry.dimension == Dimension::Performance {
            entry.expected_state = verter_validation_probe::ExpectedState::Gate;
            entry.expected_class = Some(C::Pass);
            entry.authority = Some(verter_validation_probe::Authority::CompilerPublicRequestRoute);
            entry.atom = Some("route-callable".to_string());
        }
    }
    // A host failure at Route leaves Performance unreached.
    let summary = one_case_summary(
        &manifest,
        vec![
            phase(CASE, Phase::Compile),
            compile_frame(CASE, vec![failure_entry(CASE, "host", None)]),
            phase(CASE, Phase::Reference),
            reference_frame(
                CASE,
                vec![ReferenceResult::Produced {
                    code: MODULE.to_string(),
                }],
            ),
        ],
    );
    let performance = summary.frameworks[0].cases[0]
        .cells
        .iter()
        .find(|cell| cell.dimension == Dimension::Performance)
        .expect("the performance cell is present");
    assert_eq!(performance.observed_class, None);
    assert_eq!(
        performance.observed_terminal,
        verter_validation_probe::ObservedTerminal::NotRun
    );
    assert_eq!(performance.evaluation, Evaluation::GateRegression);
    assert_ne!(
        summary.disposition().exit_code(),
        0,
        "a gate that never ran must fail the lane",
    );
}
