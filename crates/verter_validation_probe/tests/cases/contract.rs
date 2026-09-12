//! The probe outcome contract: manifest rejection fixtures (one per
//! rejection), the precedence fold, per-dimension propagation, and
//! expectation evaluation.

use std::collections::BTreeMap;

use verter_validation_probe::{
    Authority, CaseObservation, CellId, Dimension, DimensionInput, Evaluation, ExpectedState,
    Framework, InvalidObservation, ManifestError, ManifestViolation, NotApplicableReason,
    ProbeEntry, ProbeOutcomeClass as C, ProbeStateManifest, Terminal,
};

const CASE: &str = "vue/fixtures/App.vue";

/// A valid manifest: one case, one cell per dimension, every state in use.
const MANIFEST: &str = r#"
framework = "vue"
comparison = "structural"
external_revision = "0123456789abcdef0123456789abcdef01234567"

smoke = ["vue/fixtures/App.vue"]

[comparator]
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"

[applicability]
runtime = "inapplicable"
map = "inapplicable"

[[strata]]
id = "app"
pattern = "App*"
min_cases = 1

[[inventory]]
case_id = "vue/fixtures/App.vue"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Route"
expected_state = "gate"
expected_class = "pass"
authority = "compiler.public-request-route"
atom = "route-callable"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Compile"
expected_state = "canary"
expected_class = "pass"
authority = "vue.runtime-client-product"
atom = "product-shape"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Structural"
expected_state = "canary"
expected_class = "semantic_mismatch"
authority = "vue.runtime-client-product"
atom = "product-identity"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Runtime"
expected_state = "skip"
authority = "vue.runtime-client-product"
atom = "runtime-behavior"
reason = "runtime_executor_absent"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Map"
expected_state = "skip"
authority = "vue.runtime-client-product"
atom = "source-map"
reason = "map_validator_absent"

[[entries]]
probe_id = "vue/fixtures/App.vue"
framework = "vue"
case = "fixtures/App.vue"
dimension = "Performance"
expected_state = "canary"
expected_class = "pass"
authority = "compiler.equivalent-work-ledger"
atom = "equivalent-work-ledger"
"#;

/// Replace exactly one occurrence; a fixture whose edit did not apply would
/// otherwise test the unmodified manifest.
fn planted(find: &str, replace: &str) -> String {
    assert_eq!(
        MANIFEST.matches(find).count(),
        1,
        "fixture edit must target exactly one occurrence: {find:?}"
    );
    MANIFEST.replacen(find, replace, 1)
}

fn cell(dimension: Dimension) -> CellId {
    CellId {
        probe_id: CASE.to_string(),
        dimension,
    }
}

fn violations(text: &str) -> Vec<ManifestViolation> {
    match ProbeStateManifest::from_toml_str(text) {
        Err(ManifestError::Invalid(violations)) => violations,
        other => panic!("expected contract violations, got {other:?}"),
    }
}

fn manifest_entry(dimension: Dimension) -> ProbeEntry {
    let manifest = ProbeStateManifest::from_toml_str(MANIFEST).expect("fixture manifest is valid");
    manifest
        .entries
        .into_iter()
        .find(|entry| entry.dimension == dimension)
        .expect("fixture manifest has a cell per dimension")
}

fn entry(dimension: Dimension, expected_state: ExpectedState, expected_class: C) -> ProbeEntry {
    ProbeEntry {
        probe_id: CASE.to_string(),
        framework: Framework::Vue,
        case: "fixtures/App.vue".to_string(),
        dimension,
        expected_state,
        expected_class: Some(expected_class),
        authority: Some(Authority::VueRuntimeClientProduct),
        atom: Some("product-shape".to_string()),
        reason: None,
        external_revision: None,
    }
}

fn seen(classes: &[C]) -> DimensionInput {
    DimensionInput::Observed {
        classes: classes.to_vec(),
        evidence: Vec::new(),
    }
}

/// Route and Compile as given; Structural and Performance unreached unless
/// given; Runtime and Map not applicable, as the fixture manifest declares.
fn observe(
    route: DimensionInput,
    compile: DimensionInput,
    structural: DimensionInput,
    performance: DimensionInput,
) -> Result<CaseObservation, InvalidObservation> {
    CaseObservation::fold(
        CASE,
        BTreeMap::from([
            (Dimension::Route, route),
            (Dimension::Compile, compile),
            (Dimension::Structural, structural),
            (
                Dimension::Runtime,
                DimensionInput::NotApplicable(NotApplicableReason::RuntimeExecutorAbsent),
            ),
            (
                Dimension::Map,
                DimensionInput::NotApplicable(NotApplicableReason::MapValidatorAbsent),
            ),
            (Dimension::Performance, performance),
        ]),
    )
}

#[test]
fn contract_manifest_validates() {
    let manifest = ProbeStateManifest::from_toml_str(MANIFEST).expect("fixture manifest is valid");
    assert_eq!(manifest.entries.len(), Dimension::ALL.len());
}

#[test]
fn a_manifest_filed_under_another_framework_is_rejected() {
    assert!(ProbeStateManifest::from_manifest_file("vue.toml", MANIFEST).is_ok());
    assert_eq!(
        ProbeStateManifest::from_manifest_file("svelte.toml", MANIFEST),
        Err(ManifestError::Invalid(vec![
            ManifestViolation::FileNameMismatch {
                file_name: "svelte.toml".to_string()
            }
        ]))
    );
}

#[test]
fn generic_failure_alias_is_not_an_outcome_class() {
    let text = planted(
        r#"expected_class = "semantic_mismatch""#,
        r#"expected_class = "failed""#,
    );
    match ProbeStateManifest::from_toml_str(&text) {
        Err(ManifestError::Parse(message)) => assert!(
            message.contains("failed"),
            "the rejection names the alias: {message}"
        ),
        other => panic!("an outcome alias must not parse, got {other:?}"),
    }
}

#[test]
fn known_fail_without_owner_is_rejected() {
    let text = planted(
        "expected_state = \"canary\"\nexpected_class = \"semantic_mismatch\"\nauthority = \"vue.runtime-client-product\"\natom = \"product-identity\"",
        "expected_state = \"known-fail\"\nexpected_class = \"semantic_mismatch\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::MissingCitation {
            cell: cell(Dimension::Structural)
        }]
    );
}

#[test]
fn route_gate_expecting_unsupported_is_rejected() {
    let text = planted(
        "expected_state = \"gate\"\nexpected_class = \"pass\"",
        "expected_state = \"gate\"\nexpected_class = \"unsupported\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::InadmissibleClass {
            cell: cell(Dimension::Route),
            class: C::Unsupported
        }]
    );
}

#[test]
fn known_fail_expecting_pass_is_rejected() {
    let text = planted(
        "expected_state = \"canary\"\nexpected_class = \"pass\"\nauthority = \"vue.runtime-client-product\"",
        "expected_state = \"known-fail\"\nexpected_class = \"pass\"\nauthority = \"vue.runtime-client-product\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::KnownFailExpectsPass {
            cell: cell(Dimension::Compile)
        }]
    );
}

#[test]
fn uncited_pass_canary_is_rejected() {
    let text = planted(
        "expected_class = \"pass\"\nauthority = \"vue.runtime-client-product\"\natom = \"product-shape\"",
        "expected_class = \"pass\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::MissingCitation {
            cell: cell(Dimension::Compile)
        }]
    );
}

#[test]
fn classless_canary_is_rejected() {
    let text = planted("expected_class = \"semantic_mismatch\"\n", "");
    assert_eq!(
        violations(&text),
        [ManifestViolation::MissingClass {
            cell: cell(Dimension::Structural)
        }]
    );
}

#[test]
fn uncited_failure_canary_is_rejected() {
    let text = planted(
        "expected_class = \"semantic_mismatch\"\nauthority = \"vue.runtime-client-product\"\natom = \"product-identity\"",
        "expected_class = \"semantic_mismatch\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::MissingCitation {
            cell: cell(Dimension::Structural)
        }]
    );
}

#[test]
fn expected_class_outside_the_dimension_matrix_is_rejected() {
    let text = planted(
        "dimension = \"Performance\"\nexpected_state = \"canary\"\nexpected_class = \"pass\"",
        "dimension = \"Performance\"\nexpected_state = \"canary\"\nexpected_class = \"verter_diagnostic\"",
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::InadmissibleClass {
            cell: cell(Dimension::Performance),
            class: C::VerterDiagnostic
        }]
    );
}

#[test]
fn omitted_sibling_cell_is_rejected() {
    let map_cell = "[[entries]]\nprobe_id = \"vue/fixtures/App.vue\"\nframework = \"vue\"\ncase = \"fixtures/App.vue\"\ndimension = \"Map\"\nexpected_state = \"skip\"\nauthority = \"vue.runtime-client-product\"\natom = \"source-map\"\nreason = \"map_validator_absent\"\n";
    let text = planted(map_cell, "");
    assert_eq!(
        violations(&text),
        [ManifestViolation::MissingCell {
            cell: cell(Dimension::Map)
        }]
    );
}

#[test]
fn duplicated_cell_is_rejected() {
    let text = format!(
        "{MANIFEST}\n[[entries]]\nprobe_id = \"vue/fixtures/App.vue\"\nframework = \"vue\"\ncase = \"fixtures/App.vue\"\ndimension = \"Route\"\nexpected_state = \"canary\"\nexpected_class = \"request_refused\"\nauthority = \"compiler.public-request-route\"\natom = \"typed-refusal\"\n"
    );
    assert_eq!(
        violations(&text),
        [ManifestViolation::DuplicateCell {
            cell: cell(Dimension::Route)
        }]
    );
}

#[test]
fn structural_cell_without_a_bound_comparator_must_be_a_skip() {
    let without_comparator = planted(
        "[comparator]\ncrate = \"verter_vue_conformance\"\npath = \"src/compare.rs\"\nfunction = \"compare_modules\"\natom = \"product-identity\"\n",
        "",
    );
    assert_eq!(
        without_comparator
            .matches("comparison = \"structural\"")
            .count(),
        1
    );
    let text =
        without_comparator.replacen("comparison = \"structural\"", "comparison = \"none\"", 1);
    assert_eq!(
        violations(&text),
        [ManifestViolation::InapplicableCellNotSkipped {
            cell: cell(Dimension::Structural)
        }]
    );
}

#[test]
fn a_structural_mismatch_never_touches_the_route_cell() {
    let observation = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::SemanticMismatch]),
        seen(&[C::Pass]),
    )
    .expect("a structural mismatch over a produced product is observable");
    assert_eq!(
        manifest_entry(Dimension::Route).evaluate(&observation),
        Evaluation::GatePass
    );
    assert_eq!(
        manifest_entry(Dimension::Structural).evaluate(&observation),
        Evaluation::CanaryExpected
    );
}

#[test]
fn impossible_observations_are_rejected_not_resolved() {
    let rows: [(Dimension, &[C], InvalidObservation); 5] = [
        (
            Dimension::Compile,
            &[],
            InvalidObservation::NothingObserved {
                dimension: Dimension::Compile,
            },
        ),
        (
            Dimension::Compile,
            &[C::Pass, C::VerterDiagnostic],
            InvalidObservation::PassWithFailure {
                dimension: Dimension::Compile,
                failure: C::VerterDiagnostic,
            },
        ),
        (
            Dimension::Structural,
            &[C::SemanticMismatch, C::ProductNotProduced],
            InvalidObservation::ComparisonWithoutInput {
                dimension: Dimension::Structural,
                comparison: C::SemanticMismatch,
                absent: C::ProductNotProduced,
            },
        ),
        (
            Dimension::Structural,
            &[C::SemanticMismatch, C::ReferenceFailure],
            InvalidObservation::ComparisonWithoutInput {
                dimension: Dimension::Structural,
                comparison: C::SemanticMismatch,
                absent: C::ReferenceFailure,
            },
        ),
        (
            Dimension::Route,
            &[C::SemanticMismatch],
            InvalidObservation::Inadmissible {
                dimension: Dimension::Route,
                class: C::SemanticMismatch,
            },
        ),
    ];
    for (dimension, observed, expected) in rows {
        assert_eq!(
            C::terminal(dimension, observed),
            Err(expected),
            "{dimension} {observed:?}"
        );
    }

    // Across dimensions: a comparison over a product that was never produced.
    assert_eq!(
        observe(
            seen(&[C::Pass]),
            seen(&[C::ProductNotProduced]),
            seen(&[C::SemanticMismatch]),
            DimensionInput::Unreached,
        ),
        Err(InvalidObservation::ComparisonWithoutInput {
            dimension: Dimension::Structural,
            comparison: C::SemanticMismatch,
            absent: C::ProductNotProduced,
        })
    );
}

#[test]
fn a_diagnostic_without_a_product_stays_a_diagnostic() {
    assert_eq!(
        C::terminal(
            Dimension::Compile,
            &[C::ProductNotProduced, C::VerterDiagnostic]
        ),
        Ok(Terminal::Class {
            class: C::VerterDiagnostic,
            secondary: vec![C::ProductNotProduced],
            evidence: Vec::new(),
        })
    );
}

/// The causal precedence, highest first, exactly as the contract states it.
const PRECEDENCE: [C; 14] = [
    C::HarnessFailure,
    C::Crash,
    C::Timeout,
    C::HostFailure,
    C::RequestRefused,
    C::Unsupported,
    C::VerterDiagnostic,
    C::ReferenceFailure,
    C::ProductNotProduced,
    C::ProductMalformed,
    C::SemanticMismatch,
    C::RuntimeMismatch,
    C::SourceMapMismatch,
    C::Pass,
];

/// Classes that, beside a comparison class in one dimension, show that the
/// comparison had no product or no reference.
const ABSENT_COMPARISON_INPUT: [C; 5] = [
    C::ProductNotProduced,
    C::Unsupported,
    C::RequestRefused,
    C::HostFailure,
    C::ReferenceFailure,
];

#[test]
fn the_fold_is_total_and_deterministic_over_every_admissible_combination() {
    for dimension in Dimension::ALL {
        let admissible: Vec<C> = PRECEDENCE
            .into_iter()
            .filter(|class| dimension.admits(*class))
            .collect();
        for mask in 1u32..(1 << admissible.len()) {
            let subset: Vec<C> = admissible
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, class)| *class)
                .collect();
            let mut reversed = subset.clone();
            reversed.reverse();
            let folded = C::terminal(dimension, &subset);
            assert_eq!(
                folded,
                C::terminal(dimension, &reversed),
                "{dimension} {subset:?}: input order changed the fold"
            );

            let has_pass = subset.contains(&C::Pass);
            let has_comparison = subset.iter().any(|class| class.is_comparison());
            let has_absent_input = subset
                .iter()
                .any(|class| ABSENT_COMPARISON_INPUT.contains(class));
            if (has_pass && subset.len() > 1) || (has_comparison && has_absent_input) {
                assert!(folded.is_err(), "{dimension} {subset:?} must be rejected");
                continue;
            }
            // `subset` is already in contract precedence order.
            assert_eq!(
                folded,
                Ok(Terminal::Class {
                    class: subset[0],
                    secondary: subset[1..].to_vec(),
                    evidence: Vec::new(),
                }),
                "{dimension} {subset:?}"
            );
        }
    }
}

#[test]
fn process_failures_never_satisfy_an_expected_diagnostic_or_unsupported() {
    let rows = [
        (
            ExpectedState::Canary,
            C::VerterDiagnostic,
            C::Crash,
            Evaluation::UnrelatedRegression,
        ),
        (
            ExpectedState::KnownFail,
            C::Unsupported,
            C::Timeout,
            Evaluation::UnrelatedRegression,
        ),
        (
            ExpectedState::Gate,
            C::VerterDiagnostic,
            C::HarnessFailure,
            Evaluation::GateRegression,
        ),
    ];
    for (state, expected, observed, evaluation) in rows {
        let observation = observe(
            seen(&[observed]),
            DimensionInput::Unreached,
            DimensionInput::Unreached,
            DimensionInput::Unreached,
        )
        .expect("a route-level process failure propagates");
        assert_eq!(
            observation.terminal(Dimension::Compile).class(),
            Some(observed)
        );
        assert_eq!(
            entry(Dimension::Compile, state, expected).evaluate(&observation),
            evaluation,
            "{state:?} expecting {expected} observing {observed}"
        );
    }
}

#[test]
fn a_timeout_against_an_unsupported_gate_is_a_gate_regression() {
    let observation = observe(
        seen(&[C::Timeout]),
        DimensionInput::Unreached,
        DimensionInput::Unreached,
        DimensionInput::Unreached,
    )
    .expect("a route-level timeout propagates");
    assert_eq!(
        entry(Dimension::Compile, ExpectedState::Gate, C::Unsupported).evaluate(&observation),
        Evaluation::GateRegression
    );
}

#[test]
fn a_pass_canary_reports_observed_pass_then_canary_regression() {
    let canary = manifest_entry(Dimension::Compile);
    assert_eq!(canary.expected_state, ExpectedState::Canary);
    assert_eq!(canary.expected_class, Some(C::Pass));
    let passing = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
    )
    .expect("a passing case is observable");
    let failing = observe(
        seen(&[C::Pass]),
        seen(&[C::ProductMalformed]),
        DimensionInput::Unreached,
        DimensionInput::Unreached,
    )
    .expect("a malformed product is observable");
    let sequence = [canary.evaluate(&passing), canary.evaluate(&failing)];
    assert_eq!(
        sequence,
        [Evaluation::ObservedPass, Evaluation::CanaryRegression]
    );
    assert!(!sequence.contains(&Evaluation::GatePass));
}

#[test]
fn an_unexpected_pass_on_a_failure_expectation_is_an_xpass_not_a_gate() {
    let passing = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
    )
    .expect("a passing case is observable");
    for state in [ExpectedState::KnownFail, ExpectedState::Canary] {
        let evaluation = entry(Dimension::Compile, state, C::VerterDiagnostic).evaluate(&passing);
        assert_eq!(evaluation, Evaluation::Xpass, "{state:?} observing pass");
        assert!(!evaluation.blocks(), "an xpass never blocks");
    }
}

#[test]
fn a_still_failing_known_fail_is_its_own_expected_match() {
    let diagnosed = observe(
        seen(&[C::Pass]),
        seen(&[C::VerterDiagnostic]),
        DimensionInput::Unreached,
        DimensionInput::Unreached,
    )
    .expect("a compile diagnostic is observable");
    let rows = [
        (ExpectedState::KnownFail, Evaluation::KnownFailExpected),
        (ExpectedState::Canary, Evaluation::CanaryExpected),
    ];
    for (state, expected) in rows {
        let evaluation = entry(Dimension::Compile, state, C::VerterDiagnostic).evaluate(&diagnosed);
        assert_eq!(
            evaluation, expected,
            "{state:?} observing its expected class"
        );
        assert!(!evaluation.blocks(), "an expected failure never blocks");
    }
}

#[test]
fn lower_failures_propagate_and_independent_higher_failures_win() {
    let observation = observe(
        seen(&[C::Pass]),
        seen(&[C::VerterDiagnostic]),
        seen(&[C::ReferenceFailure]),
        seen(&[C::Pass]),
    )
    .expect("a diagnostic followed by a reference death is observable");
    assert_eq!(
        observation.terminal(Dimension::Compile).class(),
        Some(C::VerterDiagnostic)
    );
    assert_eq!(
        observation.terminal(Dimension::Structural).class(),
        Some(C::ReferenceFailure)
    );
    assert_eq!(
        observation.terminal(Dimension::Performance),
        &Terminal::NotRun {
            blocked_by: C::VerterDiagnostic
        }
    );

    let performance_canary = manifest_entry(Dimension::Performance);
    assert_eq!(
        performance_canary.evaluate(&observation),
        Evaluation::NotRun
    );
    let performance_gate = entry(Dimension::Performance, ExpectedState::Gate, C::Pass);
    assert_eq!(
        performance_gate.evaluate(&observation),
        Evaluation::GateRegression
    );

    assert_eq!(
        manifest_entry(Dimension::Runtime).evaluate(&observation),
        Evaluation::Skipped
    );
    let runtime_canary = entry(Dimension::Runtime, ExpectedState::Canary, C::Pass);
    assert_eq!(
        runtime_canary.evaluate(&observation),
        Evaluation::NotApplicable
    );
    let runtime_gate = entry(Dimension::Runtime, ExpectedState::Gate, C::Pass);
    assert_eq!(
        runtime_gate.evaluate(&observation),
        Evaluation::GateRegression
    );
}

/// An absent comparator reported under a manifest that binds one is a
/// failure of that dimension, never a not-applicable cell.
#[test]
fn not_applicable_is_accepted_only_where_the_manifest_declares_it() {
    let manifest = ProbeStateManifest::from_toml_str(MANIFEST).expect("fixture manifest is valid");
    let total = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
    )
    .expect("a passing case is observable");
    assert_eq!(manifest.check_applicability(&total), Ok(()));

    let comparator_absent = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        DimensionInput::NotApplicable(NotApplicableReason::ComparatorAbsent),
        seen(&[C::Pass]),
    )
    .expect("a not-applicable structural dimension is representable");
    assert_eq!(
        manifest.check_applicability(&comparator_absent),
        Err(InvalidObservation::ApplicabilityMismatch {
            dimension: Dimension::Structural,
            declared: None,
            observed: Some(NotApplicableReason::ComparatorAbsent),
        })
    );
}

/// Evaluating through the manifest is what makes that check unskippable. A
/// cell reads one terminal and never sees the manifest header, so an absent
/// comparator classifies the structural canary as the non-blocking
/// `NotApplicable` — a missing comparator read as "cannot be exercised here".
#[test]
fn manifest_evaluation_refuses_an_observation_whose_applicability_disagrees() {
    let manifest = ProbeStateManifest::from_toml_str(MANIFEST).expect("fixture manifest is valid");
    let comparator_absent = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        DimensionInput::NotApplicable(NotApplicableReason::ComparatorAbsent),
        seen(&[C::Pass]),
    )
    .expect("a not-applicable structural dimension is representable");
    let structural = manifest
        .entries
        .iter()
        .find(|entry| entry.dimension == Dimension::Structural)
        .expect("the fixture manifest has a structural cell");

    let unchecked = structural.evaluate(&comparator_absent);
    assert_eq!(unchecked, Evaluation::NotApplicable);
    assert!(!unchecked.blocks(), "the fail-open this API closes");

    assert_eq!(
        manifest.evaluate_case(&comparator_absent).unwrap_err(),
        InvalidObservation::ApplicabilityMismatch {
            dimension: Dimension::Structural,
            declared: None,
            observed: Some(NotApplicableReason::ComparatorAbsent),
        }
    );
}

/// A sound observation evaluates every cell of its own case, in manifest
/// order, exactly as the per-cell API would.
#[test]
fn manifest_evaluation_returns_every_cell_of_its_case() {
    let manifest = ProbeStateManifest::from_toml_str(MANIFEST).expect("fixture manifest is valid");
    let total = observe(
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
        seen(&[C::Pass]),
    )
    .expect("a passing case is observable");
    let evaluated = manifest
        .evaluate_case(&total)
        .expect("a sound observation evaluates");
    assert_eq!(
        evaluated
            .iter()
            .map(|(entry, evaluation)| (entry.dimension, *evaluation))
            .collect::<Vec<_>>(),
        vec![
            (Dimension::Route, Evaluation::GatePass),
            (Dimension::Compile, Evaluation::ObservedPass),
            (Dimension::Structural, Evaluation::Xpass),
            (Dimension::Runtime, Evaluation::Skipped),
            (Dimension::Map, Evaluation::Skipped),
            (Dimension::Performance, Evaluation::ObservedPass),
        ]
    );
}

/// Hand-built (or deserialized) terminals must honour propagation: a pass
/// under a failed feeding dimension is a false green, and a not-run where the
/// feeding failure is admissible would hide the propagated class.
#[test]
fn terminals_inconsistent_with_their_feeding_dimension_are_rejected() {
    let propagated = observe(
        seen(&[C::Pass]),
        seen(&[C::ProductNotProduced]),
        DimensionInput::Unreached,
        DimensionInput::Unreached,
    )
    .expect("a missing product is observable")
    .terminals()
    .clone();
    let rows = [
        (
            Terminal::Class {
                class: C::Pass,
                secondary: Vec::new(),
                evidence: Vec::new(),
            },
            InvalidObservation::ContradictsFeeding {
                dimension: Dimension::Structural,
                feeding: C::ProductNotProduced,
            },
        ),
        (
            Terminal::NotRun {
                blocked_by: C::ProductNotProduced,
            },
            InvalidObservation::UnblockedNotRun {
                dimension: Dimension::Structural,
                blocked_by: C::ProductNotProduced,
            },
        ),
    ];
    for (structural, expected) in rows {
        let mut terminals = propagated.clone();
        terminals.insert(Dimension::Structural, structural);
        assert_eq!(CaseObservation::new(CASE, terminals), Err(expected));
    }
}

// ---------------------------------------------------------------------------
// The smoke slice
// ---------------------------------------------------------------------------

/// An EMPTIED slice is refused. Otherwise the required pull-request lane could
/// be silently re-scoped to nothing and still publish a green summary: the one
/// outcome a bounded lane must never be able to report.
#[test]
fn an_emptied_smoke_slice_is_refused() {
    let planted = planted("smoke = [\"vue/fixtures/App.vue\"]\n", "");
    assert!(
        violations(&planted).contains(&ManifestViolation::EmptySmokeSlice),
        "an empty smoke slice must be refused: {:?}",
        violations(&planted),
    );
}

/// A PADDED slice is refused. The pull-request bound is structural, so a slice
/// that grew past it has changed what the required job costs without anyone
/// deciding to.
#[test]
fn a_smoke_slice_above_the_bound_is_refused() {
    let padded: Vec<String> = (0..verter_validation_probe::MAX_SMOKE_CASES + 1)
        .map(|_| format!("\"{CASE}\""))
        .collect();
    let planted = planted(
        "smoke = [\"vue/fixtures/App.vue\"]",
        &format!("smoke = [{}]", padded.join(", ")),
    );
    assert!(
        violations(&planted).contains(&ManifestViolation::SmokeSliceTooLarge {
            listed: verter_validation_probe::MAX_SMOKE_CASES + 1,
        }),
        "a slice above the bound must be refused: {:?}",
        violations(&planted),
    );
}

/// A HAND-PICKED slice is refused. The listed slice is what a reviewer reads,
/// and its derivation is what the strata promise; a slice that is not its own
/// derivation has quietly replaced the representative coverage with a choice
/// nobody reviewed.
#[test]
fn a_smoke_slice_that_is_not_its_own_derivation_is_refused() {
    let planted = planted(
        "smoke = [\"vue/fixtures/App.vue\"]",
        "smoke = [\"vue/fixtures/Other.vue\"]",
    );
    assert!(
        violations(&planted).contains(&ManifestViolation::SmokeSliceNotDerived {
            expected: vec![CASE.to_string()],
            listed: vec!["vue/fixtures/Other.vue".to_string()],
        }),
        "a hand-picked slice must be refused: {:?}",
        violations(&planted),
    );
}

// ---------------------------------------------------------------------------
// Gate admission
// ---------------------------------------------------------------------------

/// Only `Route` may gate. Every other dimension's behaviour is owned by an
/// authority that is not implemented yet, so a gate there would block the
/// required job on an outcome nobody has promised.
#[test]
fn a_gate_outside_route_is_refused() {
    let planted = planted(
        "dimension = \"Compile\"\nexpected_state = \"canary\"\nexpected_class = \"pass\"\n\
         authority = \"vue.runtime-client-product\"\natom = \"product-shape\"",
        "dimension = \"Compile\"\nexpected_state = \"gate\"\nexpected_class = \"pass\"\n\
         authority = \"compiler.public-request-route\"\natom = \"route-callable\"",
    );
    assert_eq!(
        violations(&planted),
        vec![ManifestViolation::GateOutsideRoute {
            cell: cell(Dimension::Compile)
        }],
    );
}

/// A gate may cite only the implemented route authority. A gate citing a
/// product authority would turn an unimplemented promise into a required job's
/// blocking condition.
#[test]
fn a_gate_citing_another_authority_is_refused() {
    let planted = planted(
        "authority = \"compiler.public-request-route\"\natom = \"route-callable\"",
        "authority = \"vue.runtime-client-product\"\natom = \"product-shape\"",
    );
    assert_eq!(
        violations(&planted),
        vec![ManifestViolation::GateAuthorityNotRoute {
            cell: cell(Dimension::Route),
            authority: Authority::VueRuntimeClientProduct,
        }],
    );
}
