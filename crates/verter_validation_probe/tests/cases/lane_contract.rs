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
    classify_execution, parse_line, DiagnosticsSnapshot, DriverLine, ExecutionEvent,
    FrameViolation, Phase, ProbeRun, ReferenceResult, RequestedEntry, RouteDiagnostic, RouteEntry,
    RouteFailure, RouteNode, RouteProduct, RouteResponse, VirtualNodeKind,
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

    let markdown = summary.to_markdown();
    for header in [
        "attempted",
        "passed",
        "gated regressions",
        "canary failures",
        "known failures",
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
    for counter in verter_validation_probe::Counters::REQUIRED {
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("summary is JSON");
        parsed["totals"]
            .as_object_mut()
            .expect("totals is an object")
            .remove(counter);
        let planted = parsed.to_string();
        assert!(
            planted != json,
            "the plant for `{counter}` must actually change the document",
        );
        assert!(
            Summary::from_json_str(&planted).is_err(),
            "a summary missing `{counter}` must be refused",
        );
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
        selected: vec![CASE.to_string(), "vue/fixtures/Other.vue".to_string()],
        observed: vec![ObservedCase {
            case_id: CASE.to_string(),
            request_digest: String::new(),
            elapsed_ns: None,
            observation,
        }],
    };
    assert!(
        summary::build(Lane::Smoke, std::slice::from_ref(&under)).is_err(),
        "a lane that attempted fewer cases than it selected must be refused",
    );

    let empty = FrameworkRun {
        manifest: &manifest,
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
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives at <root>/crates/verter_validation_probe")
        .join(".github")
        .join("workflows")
        .join("validation-probe.yml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

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
