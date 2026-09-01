//! Full-axis gate over the same 36-cell Vue seed matrix.
//!
//! [`bf2_seed_matrix`](super::bf2_seed_matrix) does not gate the oracle's
//! non-wire mapping verdict. This module does: same locked manifest, same
//! shipped compile, `check-candidate.mjs --authoritative` over the genuine
//! production result. Every axis must `ran` (`runtime` may be
//! `not-applicable` for VDOM-client — never `skipped`); overall verdict
//! `"pass"`.
//!
//! The sibling seed-matrix aggregate supplies the immutable production run and
//! executes these contracts in its single Nextest process.
//!
//! [`the_gate_detects_a_planted_defect_on_every_axis_family`] plants one
//! reversible mutation per axis family and requires `"fail"` with that
//! axis named. Plants are proven applied; an unplanted control stays green.

use std::collections::{BTreeMap, BTreeSet};

use super::bf2_seed_matrix::{
    run_bounded, Backend, SeedCell, SeedRun, TempCandidate, ORACLE_TIMEOUT,
};
use super::*;

fn harness_entry() -> PathBuf {
    harness_root_for_gate().join("bin/check-candidate.mjs")
}

/// Duplicated ONLY because `bf2_seed_matrix::harness_root` is `fn`-private
/// (not `pub(super)`) and is two lines of pure path arithmetic — promoting
/// it was judged not worth widening that module's surface for a one-line
/// helper, unlike the stateful manifest/compile/subprocess machinery above,
/// which IS reused. See the module doc for the reuse rationale.
fn harness_root_for_gate() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/framework-conformance-harness")
}

/// One cell's full report, straight off the CLI's own stdout JSON.
///
/// `pub(super)` so the sibling Svelte conformance gate drives the SAME CLI
/// through the SAME report reader instead of growing a second copy of this
/// subprocess handling and JSON projection.
pub(super) struct CellReport {
    pub(super) exit_code: Option<i32>,
    pub(super) verdict: String,
    pub(super) reasons: Vec<String>,
    pub(super) axes: BTreeMap<String, (String, Option<String>)>,
    pub(super) raw: Value,
}

pub(super) struct CandidateCase {
    case_id: String,
    golden_name: String,
    candidate: Value,
}

pub(super) fn candidate_case(
    case_id: impl Into<String>,
    golden_name: &str,
    code: &str,
    map: Option<&str>,
    diagnostics: Value,
) -> CandidateCase {
    let map_value = match map {
        Some(raw) => serde_json::from_str(raw)
            .unwrap_or_else(|error| panic!("{golden_name}: the emitted map is not JSON: {error}")),
        None => Value::Null,
    };
    CandidateCase {
        case_id: case_id.into(),
        golden_name: golden_name.to_string(),
        candidate: json!({ "code": code, "map": map_value, "diagnostics": diagnostics }),
    }
}

fn parse_cell_report(report: Value, exit_code: i32, context: &str) -> CellReport {
    let verdict = report
        .get("verdict")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: result.verdict is absent or not a string"))
        .to_string();
    let reasons = report
        .get("reasons")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{context}: result.reasons is absent or not an array"))
        .iter()
        .map(|reason| {
            reason
                .as_str()
                .unwrap_or_else(|| panic!("{context}: a result reason is not a string"))
                .to_string()
        })
        .collect();
    let axes = report
        .get("axes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{context}: result.axes is absent or not an object"))
        .iter()
        .map(|(name, state)| {
            let status = state
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{context}: axis `{name}` has no string status"))
                .to_string();
            let reason = state
                .get("reason")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            (name.clone(), (status, reason))
        })
        .collect();
    CellReport {
        exit_code: Some(exit_code),
        verdict,
        reasons,
        axes,
        raw: report,
    }
}

/// Run `check-candidate.mjs --authoritative` over one `(code, map)` pair
/// against one golden. `map` is `None` for a map-disabled cell — the
/// candidate JSON then carries `"map": null`, exactly like
/// [`bf2_seed_matrix::bf2_authored_source_oracle_runs_over_every_seed_matrix_cell`].
pub(super) fn check_candidate(golden_name: &str, code: &str, map: Option<&str>) -> CellReport {
    let map_value: Value = match map {
        Some(raw) => serde_json::from_str(raw)
            .unwrap_or_else(|error| panic!("{golden_name}: the emitted map is not JSON: {error}")),
        None => Value::Null,
    };
    let candidate = TempCandidate::write(
        golden_name,
        &json!({ "code": code, "map": map_value, "diagnostics": [] }).to_string(),
    );

    let mut command = Command::new("node");
    command
        .arg(harness_entry())
        .arg("--golden")
        .arg(golden_name)
        .arg("--candidate")
        .arg(&candidate.path)
        .arg("--authoritative")
        .current_dir(harness_root_for_gate());
    let finished = run_bounded(&mut command, ORACLE_TIMEOUT);

    assert!(
        !finished.timed_out,
        "{golden_name}: the oracle did not finish within {ORACLE_TIMEOUT:?} — it was killed, so \
         it did not run.\nstderr:\n{}",
        finished.stderr
    );
    assert!(
        matches!(finished.code, Some(0..=2)),
        "{golden_name}: the oracle exited with {:?} instead of reporting.\nstdout:\n{}\nstderr:\n{}",
        finished.code,
        finished.stdout,
        finished.stderr
    );

    let report: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "{golden_name}: the oracle emitted no JSON report ({error}), so nothing proves it \
             ran.\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    assert_eq!(
        report.get("goldenName").and_then(Value::as_str),
        Some(golden_name),
        "{golden_name}: the oracle reported on a different golden"
    );

    parse_cell_report(
        report,
        finished.code.expect("the reporting exit code was checked"),
        golden_name,
    )
}

/// Run ordered candidate cases in one Node process. The outer protocol is
/// deliberately closed: missing, extra, reordered, duplicated, or typed-error
/// rows are rejected before any cell report can count as evidence.
pub(super) fn check_candidate_batch(cases: &[CandidateCase]) -> Vec<CellReport> {
    let body = json!({
        "cases": cases.iter().map(|case| json!({
            "caseId": case.case_id,
            "goldenName": case.golden_name,
            "candidate": case.candidate,
        })).collect::<Vec<_>>()
    });
    let batch = TempCandidate::write("authoritative-batch", &body.to_string());
    let mut command = Command::new("node");
    command
        .arg(harness_entry())
        .arg("--batch")
        .arg(&batch.path)
        .arg("--authoritative")
        .current_dir(harness_root_for_gate());
    let finished = run_bounded(&mut command, ORACLE_TIMEOUT);
    assert!(
        !finished.timed_out,
        "the batched oracle did not finish within {ORACLE_TIMEOUT:?}; stderr:\n{}",
        finished.stderr
    );
    assert!(
        matches!(finished.code, Some(0..=3)),
        "the batched oracle exited with {:?} instead of a protocol exit; stdout:\n{}\nstderr:\n{}",
        finished.code,
        finished.stdout,
        finished.stderr
    );

    let envelope: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "the batched oracle emitted no JSON envelope ({error}); stdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    let object = envelope
        .as_object()
        .expect("the batched oracle envelope is an object");
    let keys: BTreeSet<&str> = object.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        BTreeSet::from(["reports", "schema", "verdict"]),
        "the batched oracle envelope has the wrong members"
    );
    assert_eq!(
        object.get("schema").and_then(Value::as_str),
        Some("verter-check-candidate-batch/v1"),
        "the batched oracle schema changed"
    );
    let rows = object
        .get("reports")
        .and_then(Value::as_array)
        .expect("the batched oracle reports member is an array");
    assert_eq!(
        rows.len(),
        cases.len(),
        "the batched oracle returned the wrong report cardinality"
    );

    let mut reports = Vec::with_capacity(cases.len());
    for (index, (expected, row)) in cases.iter().zip(rows).enumerate() {
        let row = row
            .as_object()
            .unwrap_or_else(|| panic!("batch row {index} is not an object"));
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("batch row {index} has no string status"));
        if status == "error" {
            let error_keys: BTreeSet<&str> = row.keys().map(String::as_str).collect();
            assert_eq!(
                error_keys,
                BTreeSet::from(["caseId", "failure", "goldenName", "index", "status"]),
                "typed-error row {index} has the wrong members"
            );
            let failure = row
                .get("failure")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("typed-error row {index} has no failure object"));
            let failure_keys: BTreeSet<&str> = failure.keys().map(String::as_str).collect();
            assert_eq!(
                failure_keys,
                BTreeSet::from(["kind", "message", "name"]),
                "typed-error row {index} has the wrong failure members"
            );
            for member in ["kind", "name", "message"] {
                assert!(
                    failure.get(member).is_some_and(Value::is_string),
                    "typed-error row {index} failure.{member} is not a string"
                );
            }
            panic!(
                "batch case {} failed inside the oracle: {}",
                expected.case_id,
                Value::Object(failure.clone())
            );
        }
        assert_eq!(status, "reported", "batch row {index} has unknown status");
        let report_keys: BTreeSet<&str> = row.keys().map(String::as_str).collect();
        assert_eq!(
            report_keys,
            BTreeSet::from([
                "caseId",
                "exitCode",
                "goldenName",
                "index",
                "result",
                "status",
            ]),
            "reported row {index} has the wrong members"
        );
        assert_eq!(
            row.get("index").and_then(Value::as_u64),
            Some(index as u64),
            "the batched oracle reordered row {index}"
        );
        assert_eq!(
            row.get("caseId").and_then(Value::as_str),
            Some(expected.case_id.as_str()),
            "batch row {index} belongs to a different case"
        );
        assert_eq!(
            row.get("goldenName").and_then(Value::as_str),
            Some(expected.golden_name.as_str()),
            "batch row {index} belongs to a different golden"
        );
        let exit_code = row
            .get("exitCode")
            .and_then(Value::as_i64)
            .filter(|code| (0..=2).contains(code))
            .unwrap_or_else(|| panic!("batch row {index} has no typed comparison exit code"))
            as i32;
        let result = row
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("batch row {index} has no result"));
        assert_eq!(
            result.get("goldenName").and_then(Value::as_str),
            Some(expected.golden_name.as_str()),
            "batch row {index}'s result belongs to a different golden"
        );
        assert_eq!(
            result.get("authoritative").and_then(Value::as_bool),
            Some(true),
            "batch row {index} was not authoritative"
        );
        reports.push(parse_cell_report(result, exit_code, &expected.case_id));
    }

    let expected_exit = if reports.iter().any(|report| report.exit_code == Some(1)) {
        1
    } else if reports.iter().any(|report| report.exit_code == Some(2)) {
        2
    } else {
        0
    };
    assert_eq!(
        object.get("verdict").and_then(Value::as_str),
        Some("reported"),
        "the oracle marked a fully reported batch as an error"
    );
    assert_eq!(
        finished.code,
        Some(expected_exit),
        "the batch process exit disagrees with its ordered cell reports"
    );
    reports
}

/// The six independent axes the harness's own `compareArtifacts` +
/// `checkCandidate` compose (`src/compare.mjs`, `src/check-candidate.mjs`).
/// Every one of them must genuinely execute for a cell's result to be
/// acceptance evidence; only `runtime` has a legitimate non-`ran` status
/// (`not-applicable` on a VDOM-client artifact).
const REQUIRED_AXES: &[&str] = &["parse", "link", "structural", "diagnostics", "mapping"];

// The gate

/// GATED, unlike [`bf2_seed_matrix`]: every axis genuinely runs (or is
/// structurally `not-applicable`, never `skipped`) AND the full verdict is
/// `"pass"`, for all 36 cells.
pub(super) fn full_axis_gate_passes_for_every_seed_matrix_cell(
    run: &SeedRun,
    reports: &[CellReport],
) {
    let cells = &run.cells;
    assert_eq!(
        cells.len(),
        36,
        "the locked manifest holds {} cells",
        cells.len()
    );

    let mut failures = Vec::new();
    let mut summary = Vec::new();

    assert_eq!(
        reports.len(),
        cells.len(),
        "one oracle report per seed cell"
    );
    for (cell, report) in cells.iter().zip(reports) {
        let mut cell_problems = Vec::new();

        if report.exit_code != Some(0) {
            cell_problems.push(format!("exit={:?} (expected 0/pass)", report.exit_code));
        }
        if report.verdict != "pass" {
            cell_problems.push(format!("verdict={:?} (expected \"pass\")", report.verdict));
        }
        if !report.reasons.is_empty() {
            cell_problems.push(format!("reasons={:?}", report.reasons));
        }
        for axis in REQUIRED_AXES {
            match report.axes.get(*axis) {
                Some((status, _)) if status == "ran" => {}
                Some((status, reason)) => cell_problems.push(format!(
                    "axis `{axis}` reported `{status}` instead of `ran` (reason: {reason:?})"
                )),
                None => cell_problems.push(format!("axis `{axis}` is absent from the report")),
            }
        }
        match report.axes.get("runtime") {
            Some((status, _)) if status == "ran" || status == "not-applicable" => {}
            Some((status, reason)) => cell_problems.push(format!(
                "axis `runtime` reported `{status}` instead of `ran`/`not-applicable` \
                 (reason: {reason:?})"
            )),
            None => cell_problems.push("axis `runtime` is absent from the report".to_string()),
        }

        summary.push(format!(
            "{:<48} exit={:<3} verdict={:<6} axes={:?}",
            cell.golden_name,
            report.exit_code.unwrap_or(-1),
            report.verdict,
            report.axes,
        ));

        if !cell_problems.is_empty() {
            failures.push(format!(
                "── {} ──\n  {}",
                cell.golden_name,
                cell_problems.join("\n  ")
            ));
        }
    }

    println!(
        "BF2 full-axis gate, {} cells:\n{}",
        cells.len(),
        summary.join("\n")
    );

    assert!(
        failures.is_empty(),
        "{} of {} cells failed the full-axis gate:\n\n{}",
        failures.len(),
        cells.len(),
        failures.join("\n\n")
    );
}

// Mutation-discrimination — the gate is not a stub

/// The first cell whose fixture carries a `<script>` block AND requests a
/// source map, so every axis (including a genuinely populated mapping axis)
/// is exercised by the SAME base cell the mutations below start from.
fn scripted_map_enabled_cell(cells: &[SeedCell]) -> &SeedCell {
    cells
        .iter()
        .find(|cell| {
            cell.fixture != "slots.vue" && cell.source_map && cell.backend == Backend::Vdom
        })
        .expect("the manifest carries a scripted, map-enabled vdom cell")
}

/// An SSR cell for the same fixture, so the runtime plant has something to
/// genuinely execute (`runtimeApplicability`: ssr and vapor artifacts only).
fn scripted_ssr_cell<'a>(cells: &'a [SeedCell], fixture: &str) -> &'a SeedCell {
    cells
        .iter()
        .find(|cell| cell.fixture == fixture && cell.backend == Backend::Ssr && cell.source_map)
        .expect("the manifest carries a matching ssr cell")
}

pub(super) fn the_gate_detects_a_planted_defect_on_every_axis_family(run: &SeedRun) {
    let cells = &run.cells;
    let base_cell = scripted_map_enabled_cell(cells);
    let base_index = cells
        .iter()
        .position(|cell| cell.golden_name == base_cell.golden_name)
        .expect("the selected base cell is in the run");
    let base = &run.assembled[base_index];
    let pristine_code = base.code.clone();
    let pristine_map = base
        .source_map
        .clone()
        .expect("the base cell requested a source map");

    // Construct every plant before invoking the oracle so the control and all
    // discriminators traverse one ordered batch in one shared process.
    let parse_mutant = format!("{pristine_code}\nconst )(( = ;;;");
    assert_ne!(parse_mutant, pristine_code, "the parse plant did not apply");
    let link_mutant = pristine_code.replacen(
        "from \"vue\"",
        "from \"verter-gate-test-nonexistent-package-xyz\"",
        1,
    );
    assert_ne!(link_mutant, pristine_code, "the link plant did not apply");
    let structural_mutant = pristine_code.replacen(
        "as _createElementVNode",
        "as _createElementVNodeRenamedByPlant",
        1,
    );
    assert_ne!(
        structural_mutant, pristine_code,
        "the structural plant did not apply"
    );
    let mut map_json: Value =
        serde_json::from_str(&pristine_map).expect("the pristine map is JSON");
    let sources_content = map_json
        .get_mut("sourcesContent")
        .and_then(Value::as_array_mut)
        .expect("the map declares sourcesContent");
    assert!(
        !sources_content.is_empty(),
        "no sourcesContent row to corrupt"
    );
    let original_first = sources_content[0].clone();
    sources_content[0] = json!("GATE-TEST-PLANT: this is not the authored fixture's content");
    assert_ne!(
        sources_content[0], original_first,
        "the mapping plant did not apply"
    );
    let mapping_mutant_map = map_json.to_string();
    let ssr_cell = scripted_ssr_cell(cells, &base_cell.fixture);
    let ssr_index = cells
        .iter()
        .position(|cell| cell.golden_name == ssr_cell.golden_name)
        .expect("the selected SSR cell is in the run");
    let ssr_assembled = &run.assembled[ssr_index];
    let ssr_pristine = ssr_assembled.code.clone();
    // Flip a literal the template renders verbatim so the SSR HTML changes
    // without touching import/helper shape (keeps this plant orthogonal to
    // the structural one above).
    let runtime_mutant = if ssr_pristine.contains("\"zero\"") {
        ssr_pristine.replacen("\"zero\"", "\"GATE-TEST-PLANT-ZERO\"", 1)
    } else {
        // basic-interpolation.vue is the only fixture with a literal "zero"
        // text branch; fall back to mutating ANY string literal present so
        // this plant still applies for whichever fixture the base cell named.
        ssr_pristine
            .find('"')
            .map(|start| {
                let end = ssr_pristine[start + 1..]
                    .find('"')
                    .map(|offset| start + 1 + offset)
                    .expect("a closing quote follows the opening one");
                format!(
                    "{}GATE-TEST-PLANT{}",
                    &ssr_pristine[..end],
                    &ssr_pristine[end..]
                )
            })
            .expect("the SSR module contains at least one string literal to mutate")
    };
    assert_ne!(
        runtime_mutant, ssr_pristine,
        "the runtime plant did not apply"
    );

    let diagnostics = json!([{
        "kind": "error",
        "code": "GATE-TEST-PLANT",
        "message": "planted diagnostic",
        "source": "plant",
        "start": 0,
        "end": 0,
        "related": [],
    }]);
    let cases = vec![
        candidate_case(
            "control",
            &base_cell.golden_name,
            &pristine_code,
            Some(&pristine_map),
            json!([]),
        ),
        candidate_case(
            "parse",
            &base_cell.golden_name,
            &parse_mutant,
            Some(&pristine_map),
            json!([]),
        ),
        candidate_case(
            "link",
            &base_cell.golden_name,
            &link_mutant,
            Some(&pristine_map),
            json!([]),
        ),
        candidate_case(
            "structural",
            &base_cell.golden_name,
            &structural_mutant,
            Some(&pristine_map),
            json!([]),
        ),
        candidate_case(
            "diagnostics",
            &base_cell.golden_name,
            &pristine_code,
            Some(&pristine_map),
            diagnostics,
        ),
        candidate_case(
            "mapping",
            &base_cell.golden_name,
            &pristine_code,
            Some(&mapping_mutant_map),
            json!([]),
        ),
        candidate_case(
            "runtime",
            &ssr_cell.golden_name,
            &runtime_mutant,
            ssr_assembled.source_map.as_deref(),
            json!([]),
        ),
    ];
    let reports = check_candidate_batch(&cases);
    let [control, parse_report, link_report, structural_report, diag_report, mapping_report, runtime_report] =
        reports.as_slice()
    else {
        panic!("the mutation batch returned the wrong cardinality")
    };

    assert_eq!(
        control.verdict, "pass",
        "the unplanted control must pass before any plant can discriminate: {:?}",
        control.reasons
    );
    assert_eq!(parse_report.verdict, "fail", "parse plant was not detected");
    assert!(
        parse_report
            .reasons
            .iter()
            .any(|r| r.contains("failed to parse")),
        "parse plant's reasons do not name parsing: {:?}",
        parse_report.reasons
    );
    assert_eq!(link_report.verdict, "fail", "link plant was not detected");
    assert!(
        link_report
            .reasons
            .iter()
            .any(|r| r.contains("unresolved imports") || r.contains("outside the pinned closures")),
        "link plant's reasons do not name link: {:?}",
        link_report.reasons
    );
    assert_eq!(
        structural_report.verdict, "fail",
        "structural plant was not detected"
    );
    assert!(
        structural_report
            .reasons
            .iter()
            .any(|r| r.contains("structural divergence")),
        "structural plant's reasons do not name structural divergence: {:?}",
        structural_report.reasons
    );
    assert_eq!(
        diag_report.verdict, "fail",
        "diagnostics plant was not detected"
    );
    assert!(
        diag_report
            .reasons
            .iter()
            .any(|r| r.contains("diagnostics diverge")),
        "diagnostics plant's reasons do not name diagnostics: {:?}",
        diag_report.reasons
    );
    assert_eq!(
        mapping_report.verdict, "fail",
        "mapping plant was not detected"
    );
    assert!(
        mapping_report
            .reasons
            .iter()
            .any(|r| r.contains("not truthful about its own output")),
        "mapping plant's reasons do not name the mapping oracle: {:?}",
        mapping_report.reasons
    );
    assert!(
        mapping_report
            .axes
            .get("mapping")
            .is_some_and(|(status, _)| status == "ran"),
        "mapping plant must still show the mapping axis genuinely ran"
    );
    assert_eq!(
        runtime_report.verdict, "fail",
        "runtime plant was not detected"
    );
    assert!(
        runtime_report
            .reasons
            .iter()
            .any(|r| { r.contains("runtime divergence") || r.contains("structural divergence") }),
        "runtime plant's reasons name neither runtime nor structural divergence: {:?}",
        runtime_report.reasons
    );
    assert!(
        runtime_report
            .axes
            .get("runtime")
            .is_some_and(|(status, _)| status == "ran"),
        "the runtime plant's own axis must have genuinely run (ssr cell), not skipped/not-applicable: {:?}",
        runtime_report.axes
    );
}
