//! Seed conformance run — Verter Vapor + VDOM output vs the vendored
//! official Vue 3.6 RC goldens for all 32 seed SFCs (64 cells).
//!
//! Per case per backend, the harness compiles the SFC with Verter
//! (`verter_compiler::compile`, `force_vapor` for the vapor cell) and
//! assembles the runtime Main through the GENUINE shipped pipeline
//! (`vue_result_to_runtime_bundle` → `assemble_vue_main_module`), then runs
//! the structural comparator against the vendored golden. The goldens are
//! the official NON-inline topology — `compileScript({inlineTemplate:false})`
//! plus `compileTemplate(bindingMetadata)` — the same `_sfc_main` + separate
//! render-function shape Verter ships, so the comparison is apples-to-apples
//! per backend. Source maps are NOT a conformance dimension (a source map maps
//! its own compiler's output); Verter's source-map correctness is verified by
//! the separate position-encoding tests.
//!
//! Dispositions:
//! - PASS — the comparator found no in-contract difference.
//! - KNOWN-DIVERGENCE — the comparator's failure signature exactly matches a
//!   tracked entry in `corpus/known-divergences.json` (the parity backlog).
//! - otherwise the suite FAILS: new/changed divergences, or a stale entry
//!   for a cell that now passes (parity improved — remove the entry).
//!
//! `VERTER_CONFORMANCE_UPDATE=1` regenerates the dispositions file from the
//! actual results (curated `note`s are preserved); review the diff before
//! committing. `VERTER_CONFORMANCE_DEBUG=<case-id-substring>` prints one
//! cell's assembled Verter module, golden, and comparator reasons.

use std::collections::BTreeSet;

use verter_compiler::compile::{CodegenOptions, CompileDiagnosticSeverity, VerterCompileOptions};
use verter_vue_conformance::compare::{compare_modules, Comparison, DiagnosticRow, ModuleInput};
use verter_vue_conformance::{
    corpus_file, corpus_root, Backend, GoldenMeta, KnownDivergenceCell, KnownDivergences, Manifest,
    Topology,
};

use crate::common::{authored, case_sfc_source, golden_code};

const MAX_REASONS: usize = 24;

fn dispositions_path() -> std::path::PathBuf {
    corpus_root().join("known-divergences.json")
}

/// Golden directory for one (backend, topology) pair: non-inline cells live
/// under `goldens/<ver>/<backend>/…`, inline cells under
/// `goldens/<ver>/<backend>-inline/…`.
fn golden_dir(backend: Backend, topology: Topology) -> String {
    match topology {
        Topology::NonInline => backend.as_str().to_string(),
        Topology::Inline => format!("{}-inline", backend.as_str()),
    }
}

fn golden_meta(backend: Backend, topology: Topology, case_id: &str) -> GoldenMeta {
    let path = corpus_file(
        &corpus_root(),
        &format!(
            "goldens/3.6.0-rc.1/{}/{case_id}.meta.json",
            golden_dir(backend, topology)
        ),
    );
    GoldenMeta::load(&path).expect("load golden meta")
}

/// The assembled Verter module + the comparison-relevant side channels.
///
/// The module is produced by the GENUINE shipped pipeline — no hand copy:
/// `verter_compiler::compile` (blocks) →
/// `verter_compiler::framework_common::vue_result_to_runtime_bundle`
/// (the carrier's real VerterCompileResult → RuntimeCompileOutput conversion)
/// → `verter_session::compile::assemble_vue_main_module` (the host's real
/// runtime-Main assembly). The harness `CompileProfile` uses
/// `is_production: true` so the assembly omits the bundler-only `__file`/HMR
/// suffixes the compiler-level oracle does not have (block codegen itself
/// stays dev, matching the official `compileTemplate` defaults; the
/// assembler's `is_production` flag gates only those suffixes).
struct VerterCell {
    code: String,
    diagnostics: Vec<DiagnosticRow>,
}

fn compile_verter_cell(case_id: &str, backend: Backend, topology: Topology) -> VerterCell {
    let sfc = case_sfc_source(case_id);
    let alloc = oxc_allocator::Allocator::new();
    let options = CodegenOptions {
        filename: Some(format!("cases/{case_id}.vue")),
        // Inline cells compile Verter in the inline topology (the render is
        // merged into `setup()`); non-inline cells keep the default.
        inline: match topology {
            Topology::NonInline => None,
            Topology::Inline => Some(true),
        },
        ..Default::default()
    };
    let verter_options = VerterCompileOptions {
        force_js: true,
        force_vapor: backend == Backend::Vapor,
        ..Default::default()
    };
    // A Verter compile panic is itself a divergence signal, not a harness
    // crash — keep the suite able to report every cell. (`AssertUnwindSafe`:
    // the oxc allocator is not `UnwindSafe`; a panic mid-compile poisons
    // nothing we reuse — the allocator is dropped right after.)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        verter_compiler::compile::compile(&sfc, &options, &verter_options, &alloc)
    }));
    let result = match result {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "<non-string panic>".to_string());
            return VerterCell {
                code: String::new(),
                diagnostics: vec![DiagnosticRow {
                    kind: "error".to_string(),
                    code: Some("VERTER_COMPILE_PANIC".to_string()),
                    message,
                }],
            };
        }
    };

    let diagnostics = result
        .errors
        .iter()
        .map(|d| DiagnosticRow {
            kind: match d.severity {
                CompileDiagnosticSeverity::Error => "error",
                CompileDiagnosticSeverity::Warning => "warning",
                CompileDiagnosticSeverity::Info => "info",
            }
            .to_string(),
            code: Some(d.code.clone()),
            message: d.message.clone(),
        })
        .collect();

    let sfc_has_script = sfc.contains("<script");
    let bundle =
        verter_compiler::framework_common::vue_bridge::vue_result_to_runtime_bundle(result);
    let profile = verter_session::CompileProfile {
        filename: Some(format!("cases/{case_id}.vue")),
        // Assembly-only flag: skips the bundler-only `__file`/HMR suffixes
        // the compiler-level oracle lacks (see struct docs).
        is_production: true,
        ..Default::default()
    };
    let meta = verter_session::FileMeta {
        has_script: sfc_has_script,
        has_template: true,
        ..Default::default()
    };
    let module = verter_session::assemble_vue_main_module(
        &format!("cases/{case_id}.vue"),
        &bundle,
        &meta,
        &profile,
    );

    VerterCell {
        code: module,
        diagnostics,
    }
}

fn compare_cell(case_id: &str, backend: Backend, topology: Topology) -> Comparison {
    let verter = compile_verter_cell(case_id, backend, topology);
    let golden_code = golden_code(&golden_dir(backend, topology), case_id);
    let meta = golden_meta(backend, topology, case_id);
    let authored = authored(case_id);

    let verter_input = ModuleInput {
        code: verter.code.clone(),
        diagnostics: verter.diagnostics.clone(),
    };
    let golden_diagnostics = meta
        .diagnostics
        .iter()
        .map(|d| DiagnosticRow {
            kind: d.kind.clone(),
            code: d.code.as_ref().map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }),
            message: d.message.clone(),
        })
        .collect();
    let golden_input = ModuleInput {
        code: golden_code.clone(),
        diagnostics: golden_diagnostics,
    };

    let debug = std::env::var("VERTER_CONFORMANCE_DEBUG").unwrap_or_default();
    if !debug.is_empty() && case_id.contains(&debug) {
        eprintln!(
            "===== VERTER {} {} {case_id} =====\n{}\n===== GOLDEN =====\n{}",
            backend.as_str(),
            topology.as_str(),
            verter.code,
            golden_code
        );
    }

    match compare_modules(&verter_input, &golden_input, &authored, MAX_REASONS) {
        Ok(comparison) => comparison,
        Err(error) => {
            // A hard failure (e.g. Verter emitted unparseable JS) is itself
            // the divergence signature.
            let mut reasons = Vec::new();
            reasons.push(verter_vue_conformance::compare::DiffReason {
                dim: verter_vue_conformance::compare::DiffDim::Structure,
                path: "/".to_string(),
                detail: format!("comparator hard failure: {error}"),
            });
            Comparison { reasons, total: 1 }
        }
    }
}

fn reason_summaries(comparison: &Comparison) -> Vec<String> {
    comparison.reasons.iter().map(|r| r.summary()).collect()
}

/// The seed conformance run: every cell's outcome must match its tracked
/// disposition (PASS with no entry, or KNOWN-DIVERGENCE with an exact
/// signature match). Cells are keyed case × backend × topology — non-inline
/// cells for every case × backend, plus inline (official production shape)
/// cells for VDOM script-setup cases.
#[test]
fn seed_conformance_matches_tracked_dispositions() {
    let manifest = Manifest::load(&corpus_root()).expect("load manifest");
    let update = std::env::var("VERTER_CONFORMANCE_UPDATE").is_ok();
    let path = dispositions_path();

    let mut outcomes: Vec<(String, Backend, Topology, Comparison)> = Vec::new();
    for case in &manifest.cases {
        for backend in Backend::ALL {
            let comparison = compare_cell(&case.id, backend, Topology::NonInline);
            outcomes.push((case.id.clone(), backend, Topology::NonInline, comparison));
        }
        for backend in case.inline_backends.keys() {
            let comparison = compare_cell(&case.id, *backend, Topology::Inline);
            outcomes.push((case.id.clone(), *backend, Topology::Inline, comparison));
        }
    }

    // Per-case report (visible with --nocapture).
    let pass = outcomes.iter().filter(|(_, _, _, c)| c.passed()).count();
    let inline_pass = outcomes
        .iter()
        .filter(|(_, _, t, c)| *t == Topology::Inline && c.passed())
        .count();
    let inline_total = outcomes
        .iter()
        .filter(|(_, _, t, _)| *t == Topology::Inline)
        .count();
    eprintln!(
        "vue-conformance seed run: {pass}/{} cells PASS ({inline_pass}/{inline_total} inline)",
        outcomes.len()
    );

    if update {
        let existing = KnownDivergences::load(&path).unwrap_or(KnownDivergences {
            schema: 1,
            cells: Vec::new(),
        });
        let mut cells = Vec::new();
        for (case_id, backend, topology, comparison) in &outcomes {
            if comparison.passed() {
                continue;
            }
            let note = existing
                .find(case_id, *backend, *topology)
                .map(|c| c.note.clone())
                .unwrap_or_else(|| "TODO: triage this divergence".to_string());
            cells.push(KnownDivergenceCell {
                case_id: case_id.clone(),
                backend: *backend,
                topology: *topology,
                reasons: reason_summaries(comparison),
                total: comparison.total,
                note,
            });
        }
        let dispositions = KnownDivergences { schema: 1, cells };
        let json = serde_json::to_string_pretty(&dispositions).expect("serialize dispositions");
        std::fs::write(&path, format!("{json}\n")).expect("write known-divergences.json");
        eprintln!(
            "VERTER_CONFORMANCE_UPDATE: wrote {} divergence cells to {}",
            dispositions.cells.len(),
            path.display()
        );
        return;
    }

    let dispositions = KnownDivergences::load(&path).expect("load known-divergences.json");
    let mut failures: Vec<String> = Vec::new();
    for (case_id, backend, topology, comparison) in &outcomes {
        let entry = dispositions.find(case_id, *backend, *topology);
        match (comparison.passed(), entry) {
            (true, None) => {}
            (true, Some(_)) => failures.push(format!(
                "{case_id} [{backend:?} {topology:?}]: STALE divergence entry — the cell now PASSES; \
                 remove the entry (parity improved)"
            )),
            (false, None) => failures.push(format!(
                "{case_id} [{backend:?} {topology:?}]: UNTRACKED divergence ({} differences): {:?}",
                comparison.total,
                reason_summaries(comparison)
            )),
            (false, Some(entry)) => {
                let actual = reason_summaries(comparison);
                if actual != entry.reasons || comparison.total != entry.total {
                    failures.push(format!(
                        "{case_id} [{backend:?} {topology:?}]: divergence signature CHANGED\n  expected ({}): \
                         {:?}\n  actual ({}): {:?}\n  (review, then regenerate with \
                         VERTER_CONFORMANCE_UPDATE=1)",
                        entry.total, entry.reasons, comparison.total, actual
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "seed conformance disposition mismatches:\n{}",
        failures.join("\n")
    );
}

/// The dispositions file itself: well-formed, minimal, and in bijection with
/// (case × backend × topology) cells that actually diverge.
#[test]
fn known_divergences_file_is_well_formed() {
    let manifest = Manifest::load(&corpus_root()).expect("load manifest");
    let dispositions = KnownDivergences::load(&dispositions_path()).expect("load dispositions");
    assert_eq!(dispositions.schema, 1, "dispositions schema version");

    let case_ids: BTreeSet<&str> = manifest.cases.iter().map(|c| c.id.as_str()).collect();
    let mut seen = BTreeSet::new();
    for cell in &dispositions.cells {
        assert!(
            case_ids.contains(cell.case_id.as_str()),
            "dispositions entry for unknown case {}",
            cell.case_id
        );
        assert!(
            seen.insert((cell.case_id.as_str(), cell.backend, cell.topology)),
            "duplicate dispositions entry for {} [{:?} {:?}]",
            cell.case_id,
            cell.backend,
            cell.topology
        );
        assert!(
            !cell.reasons.is_empty(),
            "{} [{:?} {:?}]: divergence entry must carry its reason signature",
            cell.case_id,
            cell.backend,
            cell.topology
        );
        assert!(
            cell.total >= cell.reasons.len(),
            "{} [{:?} {:?}]: total {} < reasons {}",
            cell.case_id,
            cell.backend,
            cell.topology,
            cell.total,
            cell.reasons.len()
        );
        assert!(
            !cell.note.trim().is_empty() && !cell.note.starts_with("TODO"),
            "{} [{:?} {:?}]: divergence entry needs a curated note (the backlog item)",
            cell.case_id,
            cell.backend,
            cell.topology
        );
        // Inline cells exist only for VDOM script-setup cases (Vapor inline
        // deferred; template-only SFCs have no setup to inline into).
        if cell.topology == Topology::Inline {
            let case = manifest
                .cases
                .iter()
                .find(|c| c.id == cell.case_id)
                .expect("inline cell case exists");
            assert!(
                cell.backend == Backend::Vdom && case.inline_backends.contains_key(&Backend::Vdom),
                "{}: inline divergence entry but the case declares no vdom inline cell",
                cell.case_id
            );
        }
    }
}
