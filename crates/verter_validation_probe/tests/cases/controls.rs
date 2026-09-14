//! The false-green control set: one discriminating negative control per
//! invariant family of the validation machinery, each naming the false-green
//! it discriminates before it asserts anything.
//!
//! Positive coverage proves the machinery reports what it was built to report
//! on inputs that behave; it cannot detect its own blind spots. Each control
//! here constructs the failure the machinery claims to detect and asserts the
//! SPECIFIC outcome class, so a defect that would make CI report success while
//! the contract is broken turns one of these red instead.
//!
//! Exactly four control classes are seeded, one per family:
//!
//! 1. **Diagnostic propagation** — an error diagnostic carried by either arm
//!    of a route entry outranks product absence at `Compile`.
//! 2. **Timeout/refusal distinction** — a process outcome, a typed refusal,
//!    and a product absence are three different boundary classes, and a
//!    timed-out or signaled process never satisfies an expected-refusal
//!    assertion.
//! 3. **Comparator sensitivity** — a perturbed comparison product is
//!    reported, an alpha rename of a source-authored identifier is NOT
//!    alpha-equivalence, and a malformed reference emits no comparison.
//! 4. **Canary state transition** — an expected known failure stays
//!    non-blocking, an unexpected pass surfaces as XPASS, and an unrelated
//!    crash/timeout/harness failure surfaces as a new regression rather than
//!    being silently accepted as the known failure.
//!
//! # Admission rule for any future control
//!
//! A control is admitted only with all three stated up front, in the control's
//! own doc comment:
//!
//! 1. the invariant protected;
//! 2. the plausible false-green regression — the defect that would make the
//!    required lane report success while the contract is broken;
//! 3. why the existing positive coverage would not detect that defect.
//!
//! One discriminating control per invariant family. Mutation permutations
//! beyond the one discriminating control, controls that assert implementation
//! detail rather than the protected outcome, and controls without a plausible
//! false-green path are all rejected at proposal time. This file is the place
//! such a proposal would be made.
//!
//! The controls are hermetic: every input is constructed here, no corpus is
//! read, no process is spawned, and no network is touched, so the default
//! canonical gate runs them.

use verter_validation_probe::outcome::{Dimension, ProbeOutcomeClass as C, Terminal};
use verter_validation_probe::request;
use verter_validation_probe::runner::{
    classify_execution, parse_line, DiagnosticsSnapshot, DriverLine, ExecutionEvent, Phase,
    ProbeRun, ReferenceResult, RequestedEntry, RouteDiagnostic, RouteEntry, RouteFailure,
    RouteNode, RouteProduct, RouteResponse, VirtualNodeKind,
};
use verter_validation_probe::{
    Authority, CaseObservation, CellId, Evaluation, ExpectedState, Framework, ManifestError,
    ManifestViolation, ProbeEntry, ProbeStateManifest,
};

const CASE: &str = "vue/fixtures/App.vue";
const SVELTE_CASE: &str = "svelte/fixtures/App.svelte";

/// A source whose authored-identifier set is what the comparator's exactness
/// reads: every identifier-shaped token in the SFC text.
const TEMPLATE: &str = "<template><div/></template>\n";

// ---------------------------------------------------------------------------
// Shared harness constructs
// ---------------------------------------------------------------------------

const COMPARATOR_TOML: &str = r#"
[comparator]
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"
"#;

fn manifest_toml(
    framework: &str,
    case: &str,
    comparison: &str,
    comparator: &str,
    gate_class: &str,
) -> String {
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
expected_class = "{gate_class}"
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

fn vue_manifest() -> ProbeStateManifest {
    ProbeStateManifest::from_toml_str(&manifest_toml(
        "vue",
        "fixtures/App.vue",
        "structural",
        COMPARATOR_TOML,
        "pass",
    ))
    .expect("the vue fixture manifest is valid")
}

fn svelte_manifest() -> ProbeStateManifest {
    ProbeStateManifest::from_toml_str(&manifest_toml(
        "svelte",
        "fixtures/App.svelte",
        "none",
        "",
        "pass",
    ))
    .expect("the svelte fixture manifest is valid")
}

fn requested(case_id: &str, source: &str) -> Vec<RequestedEntry> {
    vec![RequestedEntry {
        canonical_id: case_id.to_string(),
        source: source.to_string(),
        request_digest: request::request_digest(Framework::Vue, "fixtures/App.vue"),
    }]
}

/// Fold one probe over the given driver lines and process outcome, through
/// the real ingestion boundary and the real fold.
fn observe(
    manifest: &ProbeStateManifest,
    case_id: &str,
    source: &str,
    lines: Vec<DriverLine>,
    terminated: Option<ExecutionEvent>,
) -> CaseObservation {
    let mut run = ProbeRun::new(case_id, requested(case_id, source));
    for line in lines {
        run.ingest_frame(line)
            .unwrap_or_else(|violation| panic!("frame ingested: {violation}"));
    }
    let mut folded = run.finish(manifest, terminated);
    folded
        .remove(0)
        .unwrap_or_else(|error| panic!("the observation is representable: {error}"))
}

/// A cell for `case_id` with an exact expectation; evaluation is a pure
/// function of the cell and the observation, so a cell a validated manifest
/// would refuse (a gate expecting a class its dimension does not admit) is
/// still the honest way to ask what the evaluator would do with one.
fn cell(case_id: &str, dimension: Dimension, state: ExpectedState, class: C) -> ProbeEntry {
    ProbeEntry {
        probe_id: case_id.to_string(),
        framework: Framework::Vue,
        case: "fixtures/App.vue".to_string(),
        dimension,
        expected_state: state,
        expected_class: Some(class),
        authority: Some(Authority::CompilerPublicRequestRoute),
        atom: Some("route-callable".to_string()),
        reason: None,
        external_revision: None,
    }
}

fn evaluated(
    case_id: &str,
    dimension: Dimension,
    state: ExpectedState,
    expected: C,
    observation: &CaseObservation,
) -> Evaluation {
    cell(case_id, dimension, state, expected).evaluate(observation)
}

fn phase_line(case_id: &str, phase: Phase) -> DriverLine {
    DriverLine::Phase {
        probe_id: case_id.to_string(),
        phase,
    }
}

/// A successful route answer: one `runtimeClient` product with one `main`
/// node carrying `code`.
fn product_entry(case_id: &str, code: &str) -> RouteEntry {
    nodes_entry(case_id, vec![main_node(code)])
}

fn main_node(code: &str) -> RouteNode {
    RouteNode {
        node: VirtualNodeKind {
            kind: "main".to_string(),
        },
        code: code.to_string(),
    }
}

fn nodes_entry(case_id: &str, nodes: Vec<RouteNode>) -> RouteEntry {
    RouteEntry {
        canonical_id: case_id.to_string(),
        response: Some(RouteResponse {
            diagnostics: DiagnosticsSnapshot::default(),
            products: vec![RouteProduct {
                kind: "runtimeClient".to_string(),
                nodes: Some(nodes),
            }],
        }),
        failure: None,
    }
}

/// The real failure arm: a typed `failure` whose snapshot optionally carries
/// exactly one error-severity diagnostic.
fn failure_entry(case_id: &str, kind: &str, error_diagnostic: bool) -> RouteEntry {
    RouteEntry {
        canonical_id: case_id.to_string(),
        response: None,
        failure: Some(RouteFailure {
            kind: kind.to_string(),
            message: "typed failure".to_string(),
            diagnostics: DiagnosticsSnapshot {
                diagnostics: error_diagnostic
                    .then(|| RouteDiagnostic {
                        severity: "error".to_string(),
                        code: "E1".to_string(),
                        message: "boom".to_string(),
                    })
                    .into_iter()
                    .collect(),
            },
        }),
    }
}

fn compile_line(case_id: &str, entries: Vec<RouteEntry>) -> DriverLine {
    DriverLine::Compile {
        probe_id: case_id.to_string(),
        elapsed_ns: 1_000,
        entries,
        memory: None,
    }
}

fn reference_line(case_id: &str, reference: Vec<ReferenceResult>) -> DriverLine {
    DriverLine::Reference {
        probe_id: case_id.to_string(),
        reference,
    }
}

fn produced(code: &str) -> ReferenceResult {
    ReferenceResult::Produced {
        code: code.to_string(),
    }
}

fn class_of(observation: &CaseObservation, dimension: Dimension) -> Option<C> {
    observation.terminal(dimension).class()
}

// ---------------------------------------------------------------------------
// Control 1 — diagnostic propagation
// ---------------------------------------------------------------------------

/// **Invariant:** an error-severity diagnostic carried by either arm of a
/// route entry contributes `verter_diagnostic` at `Compile`, and the causal
/// fold ranks it above the product absence the same entry reports.
///
/// **Plausible false-green:** a fold that keyed on the failure `kind` alone
/// would report `product_not_produced` for this envelope, and a lane whose
/// cell expected that class would pass while the compiler's actual diagnostic
/// — the thing a human reads — is silently dropped; a fold that trusted the
/// empty product list without reading the diagnostics channel would report a
/// generic empty output.
///
/// **Why positive coverage misses it:** the happy paths answer with either a
/// clean response or a failure carrying only its kind; no positive fixture
/// constructs an envelope where both an error diagnostic and a product
/// absence compete, so precedence between them is never exercised.
///
/// The envelope is the driver's own wire shape — the real failure arm of
/// `HostCompileRequestsEntry`, serialized whole with its kind-specific
/// fields — parsed by the production parser and fed to the runner's
/// ingestion boundary in-process; no test-only envelope shape exists.
#[test]
fn control_diagnostic_propagation_ranks_the_error_above_the_product_absence() {
    let manifest = vue_manifest();
    let compile_frame = r#"{"probe_id":"vue/fixtures/App.vue","frame":"compile","elapsed_ns":1048576,"entries":[{"canonicalId":"vue/fixtures/App.vue","response":null,"failure":{"kind":"productNotProduced","productKind":"runtimeClient","canonicalId":"vue/fixtures/App.vue","message":"the runtime client was not produced","diagnostics":{"diagnostics":[{"severity":"error","code":"v-0001","message":"the template references an unresolved binding"}]}}}]}"#;
    let reference_frame = r#"{"probe_id":"vue/fixtures/App.vue","frame":"reference","reference":[{"code":"export const value = 1\n"}]}"#;
    let observation = observe(
        &manifest,
        CASE,
        TEMPLATE,
        vec![
            parse_line(compile_frame).expect("the compile frame is a driver line"),
            parse_line(reference_frame).expect("the reference frame is a driver line"),
        ],
        None,
    );
    assert_eq!(
        class_of(&observation, Dimension::Route),
        Some(C::Pass),
        "the route answered with a typed failure, so Route is a pass"
    );
    let compile = observation.terminal(Dimension::Compile);
    assert_eq!(
        compile.class(),
        Some(C::VerterDiagnostic),
        "the error diagnostic must outrank the product absence"
    );
    match compile {
        Terminal::Class { secondary, .. } => assert_eq!(
            secondary,
            &vec![C::ProductNotProduced],
            "the product absence is retained as secondary evidence, never promoted"
        ),
        other => panic!("Compile must be a class terminal, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Control 2 — timeout/refusal distinction
// ---------------------------------------------------------------------------

/// **Invariant:** a process outcome (`timeout`, `crash`, `harness_failure`),
/// a typed refusal (`request_refused`), and a product absence (`unsupported`,
/// `product_not_produced`) are distinct boundary classes, mapped from the raw
/// execution event by the phase it happened in; no timed-out or signaled
/// process can satisfy an expected-refusal or expected-unsupported assertion.
///
/// **Plausible false-green:** a classifier that collapsed the phase map (a
/// `TimedOut` in `compile` read as a diagnostic, or any death read as the
/// expected refusal class) would let a dead compiler satisfy a negative
/// control, and the lane would publish success for a probe that never
/// produced a verdict; a manifest admission that authorized a `Route` gate
/// on `unsupported` would block the required job on an outcome no implemented
/// atom owns.
///
/// **Why positive coverage misses it:** positive fixtures end in clean exits
/// or typed failures with the expected shape; nothing positive constructs a
/// deadline death in a specific phase against a cell expecting a refusal, so
/// the evaluator's exact-class match is never asked about a process class.
#[test]
fn control_timeout_and_refusal_distinction_keeps_the_boundary_classes_apart() {
    // The event-to-class mapping: the phase is what makes the three boundary
    // classes distinct. A mutation of this mapping (for example `TimedOut`
    // in `compile` mapped to `verter_diagnostic`) turns this table red.
    let event_classes = [
        (
            ExecutionEvent::TimedOut {
                phase: Some(Phase::Compile),
            },
            C::Timeout,
        ),
        (
            ExecutionEvent::TimedOut {
                phase: Some(Phase::Reference),
            },
            C::ReferenceFailure,
        ),
        (
            ExecutionEvent::TimedOut {
                phase: Some(Phase::Load),
            },
            C::HarnessFailure,
        ),
        (
            ExecutionEvent::Signaled {
                phase: Some(Phase::Compile),
            },
            C::Crash,
        ),
        (
            ExecutionEvent::Signaled {
                phase: Some(Phase::Load),
            },
            C::HarnessFailure,
        ),
        (
            ExecutionEvent::Exited {
                phase: Some(Phase::Load),
                code: 1,
            },
            C::HarnessFailure,
        ),
        (
            ExecutionEvent::Exited {
                phase: Some(Phase::Reference),
                code: 1,
            },
            C::ReferenceFailure,
        ),
    ];
    for (event, class) in event_classes {
        assert_eq!(classify_execution(event), class, "{event:?}");
    }

    // The classes flow through the per-dimension fold into evaluation: a
    // timed-out or signaled compiler never satisfies a canary or gate that
    // expects a diagnostic, an unsupported outcome, or a typed refusal.
    let manifest = vue_manifest();
    for (event, class) in [
        (
            ExecutionEvent::TimedOut {
                phase: Some(Phase::Compile),
            },
            C::Timeout,
        ),
        (
            ExecutionEvent::Signaled {
                phase: Some(Phase::Compile),
            },
            C::Crash,
        ),
    ] {
        let observation = observe(&manifest, CASE, TEMPLATE, Vec::new(), Some(event));
        assert_eq!(class_of(&observation, Dimension::Route), Some(class));
        assert_eq!(class_of(&observation, Dimension::Compile), Some(class));
        let evaluations = [
            (
                Dimension::Compile,
                ExpectedState::Canary,
                C::VerterDiagnostic,
                Evaluation::UnrelatedRegression,
            ),
            (
                Dimension::Compile,
                ExpectedState::Canary,
                C::Unsupported,
                Evaluation::UnrelatedRegression,
            ),
            (
                Dimension::Compile,
                ExpectedState::Gate,
                C::VerterDiagnostic,
                Evaluation::GateRegression,
            ),
            (
                Dimension::Compile,
                ExpectedState::Gate,
                C::Unsupported,
                Evaluation::GateRegression,
            ),
            // A compile-phase timeout against any refusal gate.
            (
                Dimension::Route,
                ExpectedState::Gate,
                C::RequestRefused,
                Evaluation::GateRegression,
            ),
        ];
        for (dimension, state, expected, evaluation) in evaluations {
            assert_eq!(
                evaluated(CASE, dimension, state, expected, &observation),
                evaluation,
                "{event:?} against {state:?} expecting {expected} at {dimension}"
            );
        }
    }

    // The real-union table: driver lines whose entries fold to exactly one
    // boundary class each, for a Vue and a Svelte entry alike.
    struct UnionRow {
        name: &'static str,
        svelte: bool,
        lines: Vec<DriverLine>,
        terminated: Option<ExecutionEvent>,
        route: Option<C>,
        compile: Option<C>,
        compile_secondary: &'static [C],
        structural: Option<C>,
        evaluations: Vec<(Dimension, ExpectedState, C, Evaluation)>,
    }
    let rows = [
        UnionRow {
            name: "a runtime-surface refusal folds to unsupported",
            svelte: false,
            lines: vec![
                compile_line(
                    CASE,
                    vec![failure_entry(CASE, "runtimeSurfaceRefused", false)],
                ),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::Unsupported),
            compile_secondary: &[],
            structural: Some(C::Unsupported),
            evaluations: vec![(
                Dimension::Compile,
                ExpectedState::Canary,
                C::Unsupported,
                Evaluation::CanaryExpected,
            )],
        },
        UnionRow {
            name: "a runtimeClient product with zero main nodes is not produced",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![nodes_entry(CASE, Vec::new())]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::ProductNotProduced),
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a svelte runtimeClient product with zero main nodes is not produced",
            svelte: true,
            lines: vec![
                compile_line(SVELTE_CASE, vec![nodes_entry(SVELTE_CASE, Vec::new())]),
                reference_line(SVELTE_CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::ProductNotProduced),
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a runtimeClient product with two main nodes is malformed",
            svelte: false,
            lines: vec![
                compile_line(
                    CASE,
                    vec![nodes_entry(
                        CASE,
                        vec![
                            main_node("export const value = 1\n"),
                            main_node("export const value = 1\n"),
                        ],
                    )],
                ),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::ProductMalformed),
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a svelte runtimeClient product with two main nodes is malformed",
            svelte: true,
            lines: vec![
                compile_line(
                    SVELTE_CASE,
                    vec![nodes_entry(
                        SVELTE_CASE,
                        vec![
                            main_node("export const value = 1\n"),
                            main_node("export const value = 1\n"),
                        ],
                    )],
                ),
                reference_line(SVELTE_CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::ProductMalformed),
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a non-zero exit in load is a harness failure",
            svelte: false,
            lines: vec![phase_line(CASE, Phase::Load)],
            terminated: Some(ExecutionEvent::Exited {
                phase: Some(Phase::Load),
                code: 1,
            }),
            route: Some(C::HarnessFailure),
            compile: None,
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a non-zero exit in reference is a reference failure",
            svelte: false,
            lines: vec![
                phase_line(CASE, Phase::Load),
                phase_line(CASE, Phase::Compile),
                compile_line(CASE, vec![product_entry(CASE, "export const value = 1\n")]),
            ],
            terminated: Some(ExecutionEvent::Exited {
                phase: Some(Phase::Reference),
                code: 1,
            }),
            route: Some(C::Pass),
            compile: Some(C::Pass),
            compile_secondary: &[],
            structural: Some(C::ReferenceFailure),
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "an ingested compile frame survives a reference-phase death",
            svelte: false,
            lines: vec![
                phase_line(CASE, Phase::Load),
                phase_line(CASE, Phase::Compile),
                compile_line(CASE, vec![failure_entry(CASE, "refused", true)]),
            ],
            terminated: Some(ExecutionEvent::Signaled {
                phase: Some(Phase::Reference),
            }),
            route: Some(C::Pass),
            compile: Some(C::VerterDiagnostic),
            compile_secondary: &[],
            structural: Some(C::ReferenceFailure),
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a framework mismatch is a typed refusal, never an unsupported or a diagnostic",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![failure_entry(CASE, "frameworkMismatch", false)]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::RequestRefused),
            compile: Some(C::RequestRefused),
            compile_secondary: &[],
            structural: None,
            evaluations: vec![
                (
                    Dimension::Route,
                    ExpectedState::Gate,
                    C::RequestRefused,
                    Evaluation::GatePass,
                ),
                (
                    Dimension::Route,
                    ExpectedState::Gate,
                    C::Unsupported,
                    Evaluation::GateRegression,
                ),
            ],
        },
        UnionRow {
            name: "a refused failure with an error diagnostic is a diagnostic, never a refusal",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![failure_entry(CASE, "refused", true)]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::VerterDiagnostic),
            compile_secondary: &[],
            structural: None,
            evaluations: vec![(
                Dimension::Route,
                ExpectedState::Gate,
                C::Pass,
                Evaluation::GatePass,
            )],
        },
        UnionRow {
            name: "an unsupported product leaves the route passing",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![failure_entry(CASE, "unsupportedProduct", false)]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::Unsupported),
            compile_secondary: &[],
            structural: None,
            evaluations: vec![(
                Dimension::Route,
                ExpectedState::Gate,
                C::Pass,
                Evaluation::GatePass,
            )],
        },
        UnionRow {
            name: "a refused failure with no diagnostic fails closed",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![failure_entry(CASE, "refused", false)]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::Pass),
            compile: Some(C::HarnessFailure),
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
        UnionRow {
            name: "a host failure keeps its diagnostic as secondary and blocks a diagnostic gate",
            svelte: false,
            lines: vec![
                compile_line(CASE, vec![failure_entry(CASE, "host", true)]),
                reference_line(CASE, vec![produced("export const value = 1\n")]),
            ],
            terminated: None,
            route: Some(C::HostFailure),
            compile: Some(C::HostFailure),
            compile_secondary: &[C::VerterDiagnostic],
            structural: None,
            evaluations: vec![(
                Dimension::Compile,
                ExpectedState::Gate,
                C::VerterDiagnostic,
                Evaluation::GateRegression,
            )],
        },
        UnionRow {
            name: "a driver-level error line is a harness failure",
            svelte: false,
            lines: vec![parse_line(
                r#"{"probe_id":"vue/fixtures/App.vue","error":"the driver caught an exception"}"#,
            )
            .expect("an error line is a driver line")],
            terminated: None,
            route: Some(C::HarnessFailure),
            compile: None,
            compile_secondary: &[],
            structural: None,
            evaluations: Vec::new(),
        },
    ];
    for row in rows {
        let manifest = if row.svelte {
            svelte_manifest()
        } else {
            vue_manifest()
        };
        let case_id = if row.svelte { SVELTE_CASE } else { CASE };
        let observation = observe(&manifest, case_id, TEMPLATE, row.lines, row.terminated);
        for (dimension, expected) in [
            (Dimension::Route, row.route),
            (Dimension::Compile, row.compile),
            (Dimension::Structural, row.structural),
        ] {
            if let Some(expected) = expected {
                assert_eq!(
                    class_of(&observation, dimension),
                    Some(expected),
                    "{}: {dimension}",
                    row.name
                );
            }
        }
        if !row.compile_secondary.is_empty() {
            match observation.terminal(Dimension::Compile) {
                Terminal::Class { secondary, .. } => assert_eq!(
                    secondary, row.compile_secondary,
                    "{}: the diagnostic is retained as secondary evidence",
                    row.name
                ),
                other => panic!(
                    "{}: Compile must be a class terminal, got {other:?}",
                    row.name
                ),
            }
        }
        for (dimension, state, expected, evaluation) in row.evaluations {
            assert_eq!(
                evaluated(case_id, dimension, state, expected, &observation),
                evaluation,
                "{}: {state:?} expecting {expected} at {dimension}",
                row.name
            );
        }
    }

    // No atom authorizes a Route gate expecting `unsupported`: the
    // admissibility matrix is what authorizes a gate's class, and Route does
    // not admit it, so the validator rejects the manifest outright.
    let text = manifest_toml(
        "vue",
        "fixtures/App.vue",
        "structural",
        COMPARATOR_TOML,
        "unsupported",
    );
    assert_eq!(
        ProbeStateManifest::from_toml_str(&text),
        Err(ManifestError::Invalid(vec![
            ManifestViolation::InadmissibleClass {
                cell: CellId {
                    probe_id: CASE.to_string(),
                    dimension: Dimension::Route,
                },
                class: C::Unsupported,
            }
        ]))
    );
}

// ---------------------------------------------------------------------------
// Control 3 — comparator sensitivity
// ---------------------------------------------------------------------------

/// **Invariant:** the bound comparator reports a perturbed comparison
/// product; source-authored identifiers are compared exactly, so an
/// alpha-equivalent rename of one is still a mismatch; a reference module
/// the canonicalizer cannot read is a `reference_failure` and no comparison
/// is emitted.
///
/// **Plausible false-green:** a canonicalizer that treated every binding as
/// alpha-equivalent would report `pass` for a product that renamed a name the
/// SFC author wrote — the emitted module no longer references the source's
/// own binding and the lane would call it equivalent; a comparator fed a
/// malformed reference without prevalidation would either error late or
/// manufacture a comparison over an input it never read.
///
/// **Why positive coverage misses it:** positive fixtures compare a product
/// to an identical or structurally different reference; no positive fixture
/// differs from its reference ONLY by an authored-identifier rename, so the
/// exactness of authored names is never the deciding input.
#[test]
fn control_comparator_sensitivity_enforces_source_authored_exactness() {
    let manifest = vue_manifest();
    // The authored-identifier set is every identifier-shaped token in the
    // SFC source, so `greeting` is source-authored and `other` is not.
    let source = "<template><div>{{ greeting }}</div></template>\n<script setup>const greeting = 1</script>\n";
    let authored = verter_vue_conformance::authored_identifiers(source);
    assert!(
        authored.contains("greeting"),
        "the renamed name is authored"
    );
    assert!(
        !authored.contains("other"),
        "the rename target is not authored"
    );
    let module = "const greeting = 1\nexport const value = greeting\n";
    let rows = [
        (
            "an identical reference passes",
            module,
            produced(module),
            C::Pass,
        ),
        (
            "a perturbed literal is a semantic mismatch",
            "const greeting = 2\nexport const value = greeting\n",
            produced(module),
            C::SemanticMismatch,
        ),
        (
            "an alpha rename of a source-authored identifier is still a mismatch",
            module,
            produced("const other = 1\nexport const value = other\n"),
            C::SemanticMismatch,
        ),
        (
            "a malformed reference module is a reference failure",
            module,
            produced("const = ;"),
            C::ReferenceFailure,
        ),
    ];
    for (name, product, reference, expected) in rows {
        let observation = observe(
            &manifest,
            CASE,
            source,
            vec![
                phase_line(CASE, Phase::Compile),
                compile_line(CASE, vec![product_entry(CASE, product)]),
                phase_line(CASE, Phase::Reference),
                reference_line(CASE, vec![reference]),
            ],
            None,
        );
        assert_eq!(
            class_of(&observation, Dimension::Compile),
            Some(C::Pass),
            "{name}: premise — the product itself is valid"
        );
        let class = class_of(&observation, Dimension::Structural)
            .unwrap_or_else(|| panic!("{name}: Structural is a class terminal"));
        assert_eq!(class, expected, "{name}");
        assert_eq!(
            class.is_comparison(),
            expected.is_comparison(),
            "{name}: a comparison is emitted exactly when one ran"
        );
    }
}

// ---------------------------------------------------------------------------
// Control 4 — canary state transition
// ---------------------------------------------------------------------------

/// **Invariant:** an expected known failure stays non-blocking; an
/// unexpected pass surfaces as XPASS; an unrelated crash, timeout, or
/// harness failure surfaces as a new regression rather than being silently
/// accepted as the known failure.
///
/// **Plausible false-green:** an evaluator that matched any failure to any
/// expected failure class would read a dead compiler as the expected
/// diagnostic and keep the lane green; one that treated an unexpected pass on
/// a failure expectation as success would bury a promotion candidate; both
/// report success while the recorded expectation is wrong.
///
/// **Why positive coverage misses it:** positive fixtures observe the class
/// their cell expects, so the evaluator's mismatch arms are never taken on
/// real folded observations; only a constructed transition can ask what the
/// evaluator does when the observed class is not the expected one.
#[test]
fn control_canary_state_transition_surfaces_unexpected_outcomes() {
    let manifest = vue_manifest();
    let diagnosed = observe(
        &manifest,
        CASE,
        TEMPLATE,
        vec![
            phase_line(CASE, Phase::Compile),
            compile_line(CASE, vec![failure_entry(CASE, "refused", true)]),
            phase_line(CASE, Phase::Reference),
            reference_line(CASE, vec![produced("export const value = 1\n")]),
        ],
        None,
    );
    let passed = observe(
        &manifest,
        CASE,
        TEMPLATE,
        vec![
            phase_line(CASE, Phase::Compile),
            compile_line(CASE, vec![product_entry(CASE, "export const value = 1\n")]),
            phase_line(CASE, Phase::Reference),
            reference_line(CASE, vec![produced("export const value = 1\n")]),
        ],
        None,
    );
    let unrelated = [
        observe(
            &manifest,
            CASE,
            TEMPLATE,
            Vec::new(),
            Some(ExecutionEvent::Signaled {
                phase: Some(Phase::Compile),
            }),
        ),
        observe(
            &manifest,
            CASE,
            TEMPLATE,
            Vec::new(),
            Some(ExecutionEvent::TimedOut {
                phase: Some(Phase::Compile),
            }),
        ),
        observe(
            &manifest,
            CASE,
            TEMPLATE,
            vec![parse_line(
                r#"{"probe_id":"vue/fixtures/App.vue","error":"the driver caught an exception"}"#,
            )
            .expect("an error line is a driver line")],
            None,
        ),
    ];
    assert_eq!(
        class_of(&diagnosed, Dimension::Compile),
        Some(C::VerterDiagnostic)
    );
    assert_eq!(class_of(&passed, Dimension::Compile), Some(C::Pass));
    for observation in &unrelated {
        let class = class_of(observation, Dimension::Compile)
            .unwrap_or_else(|| panic!("Compile is a class terminal"));
        assert!(
            matches!(class, C::Crash | C::Timeout | C::HarnessFailure),
            "premise: {class} is an unrelated process outcome"
        );
    }

    // An expected known failure stays non-blocking, for a known-fail and a
    // canary alike.
    for (state, evaluation) in [
        (ExpectedState::KnownFail, Evaluation::KnownFailExpected),
        (ExpectedState::Canary, Evaluation::CanaryExpected),
    ] {
        let observed = evaluated(
            CASE,
            Dimension::Compile,
            state,
            C::VerterDiagnostic,
            &diagnosed,
        );
        assert_eq!(
            observed, evaluation,
            "{state:?} observing its expected class"
        );
        assert!(!observed.blocks(), "an expected failure never blocks");
    }

    // An unexpected pass surfaces as XPASS, never as a quiet success.
    for state in [ExpectedState::KnownFail, ExpectedState::Canary] {
        let observed = evaluated(
            CASE,
            Dimension::Compile,
            state,
            C::VerterDiagnostic,
            &passed,
        );
        assert_eq!(observed, Evaluation::Xpass, "{state:?} observing pass");
        assert!(!observed.blocks(), "an xpass never blocks");
    }

    // An unrelated crash, timeout, or harness failure is a new regression,
    // never silently accepted as the known failure.
    for observation in &unrelated {
        for state in [ExpectedState::KnownFail, ExpectedState::Canary] {
            assert_eq!(
                evaluated(
                    CASE,
                    Dimension::Compile,
                    state,
                    C::VerterDiagnostic,
                    observation
                ),
                Evaluation::UnrelatedRegression,
                "{state:?} observing an unrelated process outcome"
            );
        }
    }
}
