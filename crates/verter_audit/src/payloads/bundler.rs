#![deny(missing_docs)]
//! [`BundlerBatchPayload`] — strongly-typed payload for
//! `RequestKind::BundlerBatch`. The payload is produced by
//! [`crate::batch::BatchAuditAggregator::summarize`] and summarises
//! a window of recent records the bundler observed.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::BundlerKindTag;
use crate::record::{u64_as_decimal_string, RequestKind};

/// Aggregate summary produced by the bundler's batch run.
///
/// One payload covers all records the [`crate::batch::BatchAuditAggregator`]
/// observed for the configured `kind` (vite, webpack, …) within the
/// requested window. Per-kind counters partition `total_records`;
/// `slowest_5` lists the longest-running records descending by
/// `RequestTimingAudit::total_ms`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct BundlerBatchPayload {
    /// Bundler kind (vite, webpack, …) the aggregator was tagged
    /// with. Records originate from any audited request flowing
    /// through this host, not only from the bundler — the kind
    /// records WHO requested the summary.
    pub kind: BundlerKindTag,
    /// Total number of records folded into this summary. Equal to
    /// the sum of every per-kind counter below.
    pub total_records: u32,
    /// Records with `kind == RequestKind::ComponentMeta`.
    pub component_meta_count: u32,
    /// Records with `kind == RequestKind::Compile { .. }`.
    pub compile_count: u32,
    /// Records with `kind == RequestKind::TypeResolution`.
    pub type_resolution_count: u32,
    /// Records with `kind == RequestKind::SemanticAnalysis`.
    pub semantic_analysis_count: u32,
    /// Records with `kind == RequestKind::Workspace { .. }`.
    pub workspace_count: u32,
    /// Records with `kind == RequestKind::Lsp { .. }`.
    pub lsp_count: u32,
    /// Records with `kind == RequestKind::Mcp { .. }`.
    pub mcp_count: u32,
    /// Records with `kind == RequestKind::BundlerBatch { .. }`
    /// (sub-batches; usually zero in normal operation).
    pub bundler_batch_count: u32,
    /// Records with `kind == RequestKind::Custom { .. }`.
    pub custom_count: u32,
    /// Sum of `RequestTimingAudit::total_ms` across the batch.
    pub total_duration_ms: f64,
    /// Sum of `RequestMemoryAudit::bytes_parsed` across the batch.
    /// Each record's `bytes_parsed` is itself the read-once bytes
    /// the request observed across its `FileAudit` attribution, so
    /// the aggregate reflects raw bytes parsed, not on-disk file
    /// sizes.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_bytes_parsed: u64,
    /// Number of records that were satisfied from a warm host cache
    /// (i.e. `RequestAuditRecord::from_cache == true`).
    pub from_cache_count: u32,
    /// Cache-hit rate across the batch — `from_cache_count /
    /// total_records` as f32 in `[0.0, 1.0]`. `0.0` for an empty
    /// batch (no division-by-zero).
    pub cache_hit_rate: f32,
    /// Up to five slowest records descending by `total_ms`. Each
    /// summary carries enough identity (`request_id`, `canonical_id`,
    /// `kind`) to pivot back to the full record via
    /// `AuditRecordsStore::take`.
    pub slowest_5: Vec<SlowRecordSummary>,
}

/// Compact per-record fingerprint used inside
/// [`BundlerBatchPayload::slowest_5`]. Values are copied by the
/// aggregator from the underlying `RequestAuditRecord` so the
/// summary is self-contained and safe to serialise once the source
/// store is mutated.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct SlowRecordSummary {
    /// Monotonic request id from the original record. Decimal-string
    /// transport — non-zero and unique per audited request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub request_id: u64,
    /// Canonical file id the original request targeted.
    pub canonical_id: String,
    /// Original `RequestKind` discriminant — preserved verbatim so
    /// callers can tell which surface produced the slow record.
    pub kind: RequestKind,
    /// Wall-clock duration captured from
    /// `RequestTimingAudit::total_ms` (milliseconds, f64).
    pub duration_ms: f64,
}
