//! Integration coverage for [`verter_audit::batch::BatchAuditAggregator`].
//!
//! Discrimination contract:
//! - 50 records of mixed `RequestKind` are inserted into a fresh in-memory
//!   `AuditRecordSource`. The aggregator MUST report `total_records == 50`,
//!   each per-kind counter MUST sum back to `total_records`, and
//!   `slowest_5` MUST be exactly five entries sorted descending by
//!   `duration_ms`.
//! - An empty source MUST produce a zeroed payload — the cache-hit-rate
//!   path must avoid a 0/0 NaN. A pre-change aggregator that returned
//!   uninitialised fields or skipped the empty case would fail this
//!   assertion.

use std::time::Instant;

use verter_audit::batch::{AuditRecordSource, BatchAuditAggregator, SLOWEST_RECORD_LIMIT};
use verter_audit::payloads::tags::{BundlerKindTag, CompileTargetTag, LspMethodTag};
use verter_audit::payloads::workspace::WorkspaceOp;
use verter_audit::record::{RequestAuditRecord, RequestKind, RequestKindPayload};
use verter_audit::{
    BundlerBatchPayload, CompilePayload, ComponentMetaPayload, LspRequestPayload, McpToolPayload,
    SemanticAnalysisPayload, TypeResolutionPayload, WorkspacePayload,
};

/// Backing store used by the integration tests — holds an
/// `(Instant, RequestAuditRecord)` per entry, mirrors the
/// `for_each_record` contract `AuditRecordsStore` provides in
/// `verter_session` without dragging that crate into the substrate's
/// test surface.
struct VecRecordSource {
    entries: Vec<(Instant, RequestAuditRecord)>,
}

impl AuditRecordSource for VecRecordSource {
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord)) {
        for (instant, record) in &self.entries {
            f(*instant, record);
        }
    }
}

fn build_record(
    request_id: u64,
    kind: RequestKind,
    total_ms: f64,
    from_cache: bool,
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
        RequestKind::BundlerBatch { .. } => {
            RequestKindPayload::BundlerBatch(BundlerBatchPayload::default())
        }
        RequestKind::Custom { .. } => RequestKindPayload::None,
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
    };
    record.timings.total_ms = total_ms;
    record.memory.bytes_parsed = 1024;
    record
}

/// Build a 50-record corpus with a known per-kind partition,
/// monotonically increasing `total_ms`, and predictable `from_cache`
/// distribution.
fn known_50_records() -> Vec<(Instant, RequestAuditRecord)> {
    let now = Instant::now();
    let mut out: Vec<(Instant, RequestAuditRecord)> = Vec::with_capacity(50);

    // Kind partition (totals to 50):
    // 12 ComponentMeta, 8 Compile (Ide), 8 TypeResolution,
    //  6 SemanticAnalysis, 5 Workspace (ResolverWalk), 4 Lsp (Hover),
    //  3 Mcp (custom tool), 2 BundlerBatch (Vite),
    //  2 Custom (free-form name).
    let mut kinds: Vec<RequestKind> = Vec::with_capacity(50);
    let make_workspace = || RequestKind::Workspace {
        op: WorkspaceOp::ResolverWalk {
            specifier: "vue".into(),
        },
    };
    let make_lsp = || RequestKind::Lsp {
        method: LspMethodTag::Hover,
    };
    let make_mcp = || RequestKind::Mcp {
        tool: "test-tool".into(),
    };
    let make_bundler = || RequestKind::BundlerBatch {
        kind: BundlerKindTag::Vite,
    };
    let make_custom = || RequestKind::Custom {
        name: "free".into(),
    };
    let make_compile = || RequestKind::Compile {
        target: CompileTargetTag::Ide,
    };
    kinds.extend(std::iter::repeat_n(RequestKind::ComponentMeta, 12));
    kinds.extend(std::iter::repeat_with(make_compile).take(8));
    kinds.extend(std::iter::repeat_n(RequestKind::TypeResolution, 8));
    kinds.extend(std::iter::repeat_n(RequestKind::SemanticAnalysis, 6));
    kinds.extend(std::iter::repeat_with(make_workspace).take(5));
    kinds.extend(std::iter::repeat_with(make_lsp).take(4));
    kinds.extend(std::iter::repeat_with(make_mcp).take(3));
    kinds.extend(std::iter::repeat_with(make_bundler).take(2));
    kinds.extend(std::iter::repeat_with(make_custom).take(2));
    assert_eq!(kinds.len(), 50, "fixture must produce exactly 50 records");
    for (i, kind) in kinds.into_iter().enumerate() {
        let request_id = (i as u64) + 1;
        // Monotonically increasing total_ms so the slowest-5 list is
        // deterministic — entries 46..=50 should win.
        let total_ms = (request_id as f64) * 1.25;
        // Cache hit roughly every 3rd record — gives a known ratio.
        let from_cache = request_id.is_multiple_of(3);
        out.push((now, build_record(request_id, kind, total_ms, from_cache)));
    }
    out
}

#[test]
fn aggregator_summarizes_50_records_total_count_and_per_kind_partition() {
    let source = VecRecordSource {
        entries: known_50_records(),
    };
    let aggregator = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
    let payload = aggregator.summarize(None);

    assert_eq!(payload.kind, BundlerKindTag::Vite);
    assert_eq!(payload.total_records, 50, "every record must be folded in");
    assert_eq!(payload.component_meta_count, 12);
    assert_eq!(payload.compile_count, 8);
    assert_eq!(payload.type_resolution_count, 8);
    assert_eq!(payload.semantic_analysis_count, 6);
    assert_eq!(payload.workspace_count, 5);
    assert_eq!(payload.lsp_count, 4);
    assert_eq!(payload.mcp_count, 3);
    assert_eq!(payload.bundler_batch_count, 2);
    assert_eq!(payload.custom_count, 2);

    let kind_sum = payload.component_meta_count
        + payload.compile_count
        + payload.type_resolution_count
        + payload.semantic_analysis_count
        + payload.workspace_count
        + payload.lsp_count
        + payload.mcp_count
        + payload.bundler_batch_count
        + payload.custom_count;
    assert_eq!(
        kind_sum, payload.total_records,
        "per-kind counters must partition `total_records` exactly — a missing kind would \
         leave kind_sum < total_records and a double-count would push it past total_records"
    );
}

#[test]
fn aggregator_slowest_5_is_exactly_five_descending_by_duration_ms() {
    let source = VecRecordSource {
        entries: known_50_records(),
    };
    let aggregator = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
    let payload = aggregator.summarize(None);

    assert_eq!(
        payload.slowest_5.len(),
        SLOWEST_RECORD_LIMIT,
        "slowest_5 must hold exactly {SLOWEST_RECORD_LIMIT} entries when the corpus has \
         strictly more records than the cap"
    );

    // Records 46..=50 carry the largest total_ms (ascending fixture).
    let actual_request_ids: Vec<u64> = payload.slowest_5.iter().map(|s| s.request_id).collect();
    let expected_request_ids: Vec<u64> = (46..=50).rev().collect();
    assert_eq!(
        actual_request_ids, expected_request_ids,
        "slowest_5 must be ordered descending by duration_ms — top entry is the slowest"
    );

    // Strictly descending durations.
    for window in payload.slowest_5.windows(2) {
        assert!(
            window[0].duration_ms >= window[1].duration_ms,
            "slowest_5 must be sorted descending: {} >= {}",
            window[0].duration_ms,
            window[1].duration_ms
        );
    }
}

#[test]
fn aggregator_aggregates_total_duration_and_from_cache_count() {
    let source = VecRecordSource {
        entries: known_50_records(),
    };
    let aggregator = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
    let payload = aggregator.summarize(None);

    // Sum of `i * 1.25` for i=1..=50.
    let expected_total_ms: f64 = (1..=50u64).map(|i| (i as f64) * 1.25).sum();
    assert!(
        (payload.total_duration_ms - expected_total_ms).abs() < 1e-9,
        "total_duration_ms must equal the sum of every record's total_ms — got {} vs expected {}",
        payload.total_duration_ms,
        expected_total_ms
    );

    // bytes_parsed is 1024 per record × 50 records.
    assert_eq!(payload.total_bytes_parsed, 50 * 1024);

    // 50 / 3 = 16 records with from_cache (ids 3, 6, 9, ..., 48).
    let expected_from_cache = (1..=50u64).filter(|i| i.is_multiple_of(3)).count() as u32;
    assert_eq!(payload.from_cache_count, expected_from_cache);
    let expected_rate = expected_from_cache as f32 / 50.0;
    assert!((payload.cache_hit_rate - expected_rate).abs() < f32::EPSILON);
}

#[test]
fn aggregator_empty_source_yields_zeroed_payload_no_division_by_zero() {
    let source = VecRecordSource {
        entries: Vec::new(),
    };
    let aggregator = BatchAuditAggregator::new(&source, BundlerKindTag::Webpack);
    let payload: BundlerBatchPayload = aggregator.summarize(None);

    assert_eq!(payload.kind, BundlerKindTag::Webpack);
    assert_eq!(payload.total_records, 0);
    assert_eq!(payload.component_meta_count, 0);
    assert_eq!(payload.compile_count, 0);
    assert_eq!(payload.type_resolution_count, 0);
    assert_eq!(payload.semantic_analysis_count, 0);
    assert_eq!(payload.workspace_count, 0);
    assert_eq!(payload.lsp_count, 0);
    assert_eq!(payload.mcp_count, 0);
    assert_eq!(payload.bundler_batch_count, 0);
    assert_eq!(payload.custom_count, 0);
    assert_eq!(payload.total_duration_ms, 0.0);
    assert_eq!(payload.total_bytes_parsed, 0);
    assert_eq!(payload.from_cache_count, 0);
    assert!(
        payload.cache_hit_rate.is_finite() && payload.cache_hit_rate == 0.0,
        "empty-source cache_hit_rate must be exactly 0.0 (no NaN from 0/0 division)"
    );
    assert!(payload.slowest_5.is_empty());
}
