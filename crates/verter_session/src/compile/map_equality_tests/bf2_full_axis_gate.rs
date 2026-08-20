//! Full-axis gate over the same 36-cell Vue seed matrix.
//!
//! [`bf2_seed_matrix`](super::bf2_seed_matrix) does not gate the oracle's
//! non-wire mapping verdict. This module does: same locked manifest, same
//! shipped compile, `check-candidate.mjs --authoritative` over the genuine
//! production result. Every axis must `ran` (`runtime` may be
//! `not-applicable` for VDOM-client — never `skipped`); overall verdict
//! `"pass"`.
//!
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! bf2_full_axis_gate -- --test-threads=1 --nocapture`.
//!
//! [`the_gate_detects_a_planted_defect_on_every_axis_family`] plants one
//! reversible mutation per axis family and requires `"fail"` with that
//! axis named. Plants are proven applied; an unplanted control stays green.

use std::collections::BTreeMap;

use super::bf2_seed_matrix::{
    assemble, compile_cell, read_seed_matrix, run_bounded, Backend, SeedCell, TempCandidate,
    ORACLE_TIMEOUT,
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

    let verdict = report
        .get("verdict")
        .and_then(Value::as_str)
        .unwrap_or("<absent>")
        .to_string();
    let reasons: Vec<String> = report
        .get("reasons")
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let axes: BTreeMap<String, (String, Option<String>)> = report
        .get("axes")
        .and_then(Value::as_object)
        .map(|axes| {
            axes.iter()
                .map(|(name, state)| {
                    let status = state
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("<absent>")
                        .to_string();
                    let reason = state
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(ToString::to_string);
                    (name.clone(), (status, reason))
                })
                .collect()
        })
        .unwrap_or_default();

    CellReport {
        exit_code: finished.code,
        verdict,
        reasons,
        axes,
    }
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
#[test]
fn full_axis_gate_passes_for_every_seed_matrix_cell() {
    let cells = read_seed_matrix();
    assert_eq!(
        cells.len(),
        36,
        "the locked manifest holds {} cells",
        cells.len()
    );

    let mut failures = Vec::new();
    let mut summary = Vec::new();

    for cell in &cells {
        let case = compile_cell(cell);
        let assembled = assemble(&case);
        let report = check_candidate(
            &cell.golden_name,
            &assembled.code,
            assembled.source_map.as_deref(),
        );

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

#[test]
fn the_gate_detects_a_planted_defect_on_every_axis_family() {
    let cells = read_seed_matrix();
    let base_cell = scripted_map_enabled_cell(&cells);
    let base_case = compile_cell(base_cell);
    let base = assemble(&base_case);
    let pristine_code = base.code.clone();
    let pristine_map = base
        .source_map
        .clone()
        .expect("the base cell requested a source map");

    // Unplanted control: the genuine, unmutated cell must stay green. Every
    // plant below is compared against THIS run, not assumed.
    let control = check_candidate(&base_cell.golden_name, &pristine_code, Some(&pristine_map));
    assert_eq!(
        control.verdict, "pass",
        "the unplanted control must pass before any plant can be trusted to discriminate: {:?}",
        control.reasons
    );

    // parse: corrupt the candidate into invalid JavaScript
    let parse_mutant = format!("{pristine_code}\nconst )(( = ;;;");
    assert_ne!(parse_mutant, pristine_code, "the parse plant did not apply");
    let parse_report = check_candidate(&base_cell.golden_name, &parse_mutant, Some(&pristine_map));
    assert_eq!(parse_report.verdict, "fail", "parse plant was not detected");
    assert!(
        parse_report
            .reasons
            .iter()
            .any(|r| r.contains("failed to parse")),
        "parse plant's reasons do not name parsing: {:?}",
        parse_report.reasons
    );

    // ---- link: retarget a real import to a package outside the pinned closure
    let link_mutant = pristine_code.replacen(
        "from \"vue\"",
        "from \"verter-gate-test-nonexistent-package-xyz\"",
        1,
    );
    assert_ne!(link_mutant, pristine_code, "the link plant did not apply");
    let link_report = check_candidate(&base_cell.golden_name, &link_mutant, Some(&pristine_map));
    assert_eq!(link_report.verdict, "fail", "link plant was not detected");
    assert!(
        link_report
            .reasons
            .iter()
            .any(|r| r.contains("unresolved imports") || r.contains("outside the pinned closures")),
        "link plant's reasons do not name link: {:?}",
        link_report.reasons
    );

    // ---- structural: rename a helper import alias (changes the AST shape) --
    //
    // Targets `_createElementVNode`'s own import-specifier alias, not a
    // runtime helper named `ref` (VDOM's helper set has none — see
    // `VdomHelper` — and `scripted_map_enabled_cell`'s selected cell's own
    // `import { ref } from 'vue'` is the SCRIPT's unaliased user import, not
    // a template helper). `_createElementVNode` is emitted by every VDOM
    // cell compiling at least one plain element, which every non-`slots.vue`
    // map-enabled seed fixture does. The `assert_ne!` below is the guard if
    // a future manifest change ever selects a cell without it — loud, not
    // silent, exactly the failure mode this plant exists to avoid becoming
    // itself.
    let structural_mutant = pristine_code.replacen(
        "as _createElementVNode",
        "as _createElementVNodeRenamedByPlant",
        1,
    );
    assert_ne!(
        structural_mutant, pristine_code,
        "the structural plant did not apply"
    );
    let structural_report = check_candidate(
        &base_cell.golden_name,
        &structural_mutant,
        Some(&pristine_map),
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

    // ---- diagnostics: candidate claims a diagnostic the golden does not ----
    let diag_candidate = TempCandidate::write(
        &base_cell.golden_name,
        &json!({
            "code": pristine_code,
            "map": serde_json::from_str::<Value>(&pristine_map).expect("map is JSON"),
            "diagnostics": [{
                "kind": "error",
                "code": "GATE-TEST-PLANT",
                "message": "planted diagnostic",
                "source": "plant",
                "start": 0,
                "end": 0,
                "related": [],
            }],
        })
        .to_string(),
    );
    let mut diag_command = Command::new("node");
    diag_command
        .arg(harness_entry())
        .arg("--golden")
        .arg(&base_cell.golden_name)
        .arg("--candidate")
        .arg(&diag_candidate.path)
        .arg("--authoritative")
        .current_dir(harness_root_for_gate());
    let diag_finished = run_bounded(&mut diag_command, ORACLE_TIMEOUT);
    assert!(!diag_finished.timed_out, "diagnostics plant run timed out");
    let diag_report: Value = serde_json::from_str(&diag_finished.stdout).unwrap_or_else(|error| {
        panic!(
            "diagnostics plant: no JSON report ({error}): {}",
            diag_finished.stdout
        )
    });
    assert_eq!(
        diag_report.get("verdict").and_then(Value::as_str),
        Some("fail"),
        "diagnostics plant was not detected: {diag_report:#?}"
    );
    let diag_reasons: Vec<&str> = diag_report
        .get("reasons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        diag_reasons
            .iter()
            .any(|r| r.contains("diagnostics diverge")),
        "diagnostics plant's reasons do not name diagnostics: {diag_reasons:?}"
    );

    // ---- mapping: corrupt sourcesContent so it no longer matches the fixture
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
    let mapping_report = check_candidate(
        &base_cell.golden_name,
        &pristine_code,
        Some(&mapping_mutant_map),
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
        "mapping plant must still show the mapping axis genuinely ran, not skipped"
    );

    // runtime: change rendered text content on an SSR cell
    // (SSR is the cheapest runtime-applicable backend to plant against — no
    // browser-shape jsdom mount is required, only the pinned server renderer.)
    let ssr_cell = scripted_ssr_cell(&cells, &base_cell.fixture);
    let ssr_case = compile_cell(ssr_cell);
    let ssr_assembled = assemble(&ssr_case);
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
    let runtime_report = check_candidate(
        &ssr_cell.golden_name,
        &runtime_mutant,
        ssr_assembled.source_map.as_deref(),
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
