//! Discriminating integration tests for the `verter-audit-inspect`
//! CLI binary. Every test writes a known JSON corpus to a temp dir,
//! runs the bin via `CARGO_BIN_EXE_verter-audit-inspect`, and asserts
//! against the rendered stdout.
//!
//! Discrimination contract — these tests MUST fail on the pre-change
//! tree (no binary at all → `cargo test` cannot find
//! `CARGO_BIN_EXE_verter-audit-inspect`, and the build itself fails)
//! AND pass on the post-change tree. We additionally encode
//! per-subcommand discriminators that catch silent regressions:
//!
//! - `summary --json` returns a `BundlerBatchPayload` whose
//!   `total_records`, per-kind partition, and `slowest_5` ordering
//!   reflect the input fixture exactly. A regression that drops a
//!   record, mis-classifies a kind, or returns the slowest_5 in the
//!   wrong order would change the asserted JSON shape.
//! - `record <id> --json` returns the matching record and prints an
//!   error / exits 1 when the id is absent. A regression that
//!   returned the first record regardless of id would fail the
//!   "wrong-id exits 1" assertion.
//! - `cache-heatmap --json` aggregates the per-record
//!   `cache_layers.indexed.{hits,misses}` exactly. A regression that
//!   double-counted or skipped a record would change the asserted
//!   per-layer total.
//! - `compare --json` reports the partition delta as `b - a`. A
//!   regression that flipped the sign or swapped the dirs would fail
//!   the asserted `delta_*` fields.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use verter_audit::payloads::tags::{CompileTargetTag, LspMethodTag};
use verter_audit::payloads::workspace::WorkspaceOp;
use verter_audit::record::{RequestAuditRecord, RequestKind, RequestKindPayload};
use verter_audit::store::{CacheLayerBreakdown, CacheLayerHitMiss};
use verter_audit::{
    BundlerBatchPayload, CompilePayload, ComponentMetaPayload, LspRequestPayload, McpToolPayload,
    SemanticAnalysisPayload, TypeResolutionPayload, WorkspacePayload,
};

/// Path to the compiled binary — Cargo populates this env var for
/// integration tests in the same crate.
fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_verter-audit-inspect"))
}

/// Make a temp directory under `target/cli-test-<name>` and clean
/// any prior contents. We don't use `tempfile` because the workspace
/// already prefers `target/`-relative scratch dirs and we want the
/// path to be deterministic so failure logs name an existing
/// directory.
fn fresh_temp_dir(name: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("cli_summary_{name}"));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean prior temp dir");
    }
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Build a record with a fixed kind, total_ms, from_cache flag, and
/// per-cache-layer counters so each test can encode the exact field
/// it wants to discriminate against.
#[allow(clippy::too_many_arguments)]
fn make_record(
    request_id: u64,
    kind: RequestKind,
    total_ms: f64,
    from_cache: bool,
    indexed_hits: u64,
    indexed_misses: u64,
    component_meta_hits: u64,
    component_meta_misses: u64,
) -> RequestAuditRecord {
    let kind_payload = match &kind {
        RequestKind::ComponentMeta => {
            RequestKindPayload::ComponentMeta(ComponentMetaPayload::default())
        }
        RequestKind::TypeResolution => {
            RequestKindPayload::TypeResolution(TypeResolutionPayload::default())
        }
        RequestKind::SemanticAnalysis => {
            RequestKindPayload::SemanticAnalysis(SemanticAnalysisPayload::default())
        }
        RequestKind::Compile { .. } => RequestKindPayload::Compile(CompilePayload::default()),
        RequestKind::Workspace { .. } => RequestKindPayload::Workspace(WorkspacePayload::default()),
        RequestKind::Lsp { .. } => RequestKindPayload::Lsp(LspRequestPayload::default()),
        RequestKind::Mcp { .. } => RequestKindPayload::Mcp(McpToolPayload::default()),
        RequestKind::BundlerBatch { .. } => RequestKindPayload::BundlerBatch(Default::default()),
        RequestKind::Custom { .. } => RequestKindPayload::None,
        RequestKind::TypeInfoGraph => RequestKindPayload::TypeInfoGraph(Default::default()),
    };
    let mut record = RequestAuditRecord {
        request_id,
        canonical_id: format!("/req-{request_id}.vue"),
        kind,
        parent_request_id: None,
        from_cache,
        timings: Default::default(),
        memory: Default::default(),
        store: Default::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload,
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
        trace_id: String::new(),
    };
    record.timings.total_ms = total_ms;
    record.memory.bytes_parsed = 1024;
    record.store.cache_layers = CacheLayerBreakdown {
        indexed: CacheLayerHitMiss {
            hits: indexed_hits,
            misses: indexed_misses,
        },
        component_meta: CacheLayerHitMiss {
            hits: component_meta_hits,
            misses: component_meta_misses,
        },
        ..Default::default()
    };
    record
}

/// Write each record to `<dir>/<request_id>.json` so the CLI's
/// recursive walk finds them all.
fn write_corpus(dir: &Path, records: &[RequestAuditRecord]) {
    for record in records {
        let path = dir.join(format!("{}.json", record.request_id));
        let json = serde_json::to_string(record).expect("serialise record");
        fs::write(&path, json).expect("write record file");
    }
}

/// Run `verter-audit-inspect <args>` and return (exit_code,
/// stdout_string, stderr_string). Panics if the binary cannot be
/// spawned (which would mean the cargo build is broken).
fn run_cli(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(bin_path())
        .args(args)
        .output()
        .expect("spawn verter-audit-inspect");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Build a 5-record corpus with a known per-kind partition,
/// monotonically-increasing `total_ms`, and exact cache-layer
/// counters. Used by every subcommand test.
fn known_corpus() -> Vec<RequestAuditRecord> {
    vec![
        make_record(1, RequestKind::ComponentMeta, 10.0, false, 1, 2, 0, 1),
        make_record(2, RequestKind::ComponentMeta, 20.0, true, 3, 0, 1, 0),
        make_record(3, RequestKind::TypeResolution, 30.0, false, 0, 5, 0, 0),
        make_record(
            4,
            RequestKind::Compile {
                target: CompileTargetTag::Ide,
            },
            40.0,
            false,
            2,
            2,
            0,
            0,
        ),
        make_record(
            5,
            RequestKind::Lsp {
                method: LspMethodTag::Hover,
            },
            50.0,
            true,
            0,
            0,
            0,
            0,
        ),
    ]
}

#[test]
fn summary_json_partitions_records_by_kind_and_orders_slowest_descending() {
    let dir = fresh_temp_dir("summary_partition");
    write_corpus(&dir, &known_corpus());

    let (code, stdout, stderr) = run_cli(&["summary", dir.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "summary exit code 0; stderr={stderr}");

    let payload: BundlerBatchPayload =
        serde_json::from_str(&stdout).expect("summary --json must emit BundlerBatchPayload");

    assert_eq!(payload.total_records, 5, "fixture has exactly 5 records");
    assert_eq!(payload.component_meta_count, 2);
    assert_eq!(payload.type_resolution_count, 1);
    assert_eq!(payload.compile_count, 1);
    assert_eq!(payload.lsp_count, 1);
    assert_eq!(
        payload.semantic_analysis_count + payload.workspace_count + payload.mcp_count,
        0,
        "no records of these kinds in the fixture — a regression that mis-classifies kinds \
         would push counts here"
    );

    let kind_sum = payload.component_meta_count
        + payload.compile_count
        + payload.type_resolution_count
        + payload.semantic_analysis_count
        + payload.workspace_count
        + payload.lsp_count
        + payload.mcp_count
        + payload.bundler_batch_count
        + payload.custom_count;
    assert_eq!(kind_sum, payload.total_records);

    // Slowest-5 must be all 5 records sorted descending by duration.
    assert_eq!(payload.slowest_5.len(), 5);
    let actual_ids: Vec<u64> = payload.slowest_5.iter().map(|s| s.request_id).collect();
    assert_eq!(
        actual_ids,
        vec![5, 4, 3, 2, 1],
        "slowest_5 must be descending by total_ms — record id 5 is the slowest fixture entry"
    );

    // 2 of 5 records are from_cache → rate is exactly 2/5.
    assert!((payload.cache_hit_rate - 0.4).abs() < f32::EPSILON);
}

#[test]
fn summary_text_includes_total_count_and_slowest_top_entry() {
    let dir = fresh_temp_dir("summary_text");
    write_corpus(&dir, &known_corpus());

    let (code, stdout, stderr) = run_cli(&["summary", dir.to_str().unwrap()]);
    assert_eq!(code, 0, "summary exit code 0; stderr={stderr}");

    assert!(
        stdout.contains("total records:      5"),
        "human-readable summary must show total record count; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("1. request_id=5"),
        "slowest top entry must be request_id=5 (the fixture's slowest); stdout=\n{stdout}"
    );
}

#[test]
fn summary_missing_directory_exits_nonzero_with_stderr() {
    let dir = fresh_temp_dir("summary_missing");
    let missing = dir.join("does-not-exist");
    let (code, _stdout, stderr) = run_cli(&["summary", missing.to_str().unwrap()]);
    assert_ne!(code, 0, "missing dir must produce a non-zero exit code");
    assert!(
        stderr.contains("does not exist"),
        "stderr must explain the missing dir; got=\n{stderr}"
    );
}

#[test]
fn record_json_finds_record_by_request_id() {
    let dir = fresh_temp_dir("record_found");
    write_corpus(&dir, &known_corpus());

    let (code, stdout, stderr) =
        run_cli(&["record", "3", "--dir", dir.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "record exit 0; stderr={stderr}");

    let value: Value =
        serde_json::from_str(&stdout).expect("record --json must emit RequestAuditRecord");
    assert_eq!(
        value["request_id"].as_str(),
        Some("3"),
        "decoded record must have request_id=3 (decimal-string transport); got={value:?}"
    );
    assert!(
        value["kind"].as_str() == Some("TypeResolution"),
        "fixture record 3 is TypeResolution; got={value:?}"
    );
}

#[test]
fn record_missing_id_exits_1_with_stderr_message() {
    let dir = fresh_temp_dir("record_missing");
    write_corpus(&dir, &known_corpus());

    let (code, _stdout, stderr) = run_cli(&["record", "9999", "--dir", dir.to_str().unwrap()]);
    assert_eq!(
        code, 1,
        "record id absent from corpus must exit 1; stderr={stderr}"
    );
    assert!(
        stderr.contains("9999"),
        "stderr must name the missing id; got=\n{stderr}"
    );
}

#[test]
fn cache_heatmap_json_sums_indexed_layer_across_records() {
    let dir = fresh_temp_dir("heatmap_indexed");
    write_corpus(&dir, &known_corpus());

    let (code, stdout, stderr) = run_cli(&["cache-heatmap", dir.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "heatmap exit 0; stderr={stderr}");
    let value: Value =
        serde_json::from_str(&stdout).expect("cache-heatmap --json must be parseable");

    let layers = value["layers"].as_array().expect("layers must be an array");
    let indexed = layers
        .iter()
        .find(|row| row["layer"] == "indexed")
        .expect("indexed layer must be present");
    // indexed hits across the corpus: 1 + 3 + 0 + 2 + 0 = 6.
    // indexed misses across the corpus: 2 + 0 + 5 + 2 + 0 = 9.
    assert_eq!(
        indexed["hits"].as_u64(),
        Some(6),
        "indexed.hits must equal the sum of per-record hits — got {indexed:?}"
    );
    assert_eq!(
        indexed["misses"].as_u64(),
        Some(9),
        "indexed.misses must equal the sum of per-record misses — got {indexed:?}"
    );

    // component_meta layer total: hits 0+1+0+0+0=1, misses 1+0+0+0+0=1.
    let component_meta = layers
        .iter()
        .find(|row| row["layer"] == "component_meta")
        .expect("component_meta layer must be present");
    assert_eq!(component_meta["hits"].as_u64(), Some(1));
    assert_eq!(component_meta["misses"].as_u64(), Some(1));
}

#[test]
fn compare_json_reports_b_minus_a_delta() {
    let dir_a = fresh_temp_dir("compare_a");
    let dir_b = fresh_temp_dir("compare_b");
    // dir_a: 2 ComponentMeta records.
    write_corpus(
        &dir_a,
        &[
            make_record(10, RequestKind::ComponentMeta, 5.0, false, 0, 0, 0, 0),
            make_record(11, RequestKind::ComponentMeta, 6.0, false, 0, 0, 0, 0),
        ],
    );
    // dir_b: 5 records (1 ComponentMeta + 1 Workspace + 3 Lsp).
    let make_workspace = || RequestKind::Workspace {
        op: WorkspaceOp::ResolverWalk {
            specifier: "vue".into(),
        },
    };
    let make_lsp = || RequestKind::Lsp {
        method: LspMethodTag::Hover,
    };
    write_corpus(
        &dir_b,
        &[
            make_record(20, RequestKind::ComponentMeta, 7.0, true, 0, 0, 0, 0),
            make_record(21, make_workspace(), 8.0, false, 0, 0, 0, 0),
            make_record(22, make_lsp(), 9.0, false, 0, 0, 0, 0),
            make_record(23, make_lsp(), 10.0, false, 0, 0, 0, 0),
            make_record(24, make_lsp(), 11.0, true, 0, 0, 0, 0),
        ],
    );

    let (code, stdout, stderr) = run_cli(&[
        "compare",
        dir_a.to_str().unwrap(),
        dir_b.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 0, "compare exit 0; stderr={stderr}");

    let value: Value = serde_json::from_str(&stdout).expect("compare --json parseable");
    assert_eq!(value["a_total_records"], 2);
    assert_eq!(value["b_total_records"], 5);
    assert_eq!(
        value["delta_total_records"], 3,
        "b - a == 5 - 2 == 3 — a sign-flip regression would land here"
    );

    let per_kind = value["per_kind"]
        .as_array()
        .expect("per_kind must be an array");
    let cm = per_kind
        .iter()
        .find(|row| row["kind"] == "component_meta")
        .expect("component_meta delta must be present");
    assert_eq!(cm["a"], 2);
    assert_eq!(cm["b"], 1);
    assert_eq!(
        cm["delta"], -1,
        "component_meta dropped from 2 to 1 in b — delta must be -1"
    );

    let lsp = per_kind
        .iter()
        .find(|row| row["kind"] == "lsp")
        .expect("lsp delta must be present");
    assert_eq!(lsp["a"], 0);
    assert_eq!(lsp["b"], 3);
    assert_eq!(
        lsp["delta"], 3,
        "lsp climbed from 0 to 3 in b — delta must be +3"
    );
}
