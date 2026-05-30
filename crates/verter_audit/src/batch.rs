#![deny(missing_docs)]
//! Bundler-side aggregation API.
//!
//! Higher layers (notably `verter_session::AuditRecordsStore`)
//! implement [`AuditRecordSource`] and pass a borrow to
//! [`BatchAuditAggregator`], which folds the live record window into
//! a [`BundlerBatchPayload`]. The substrate stays leaf — the
//! aggregator does not depend on any consumer crate; it operates
//! solely on the `&dyn AuditRecordSource` callback contract.
//!
//! ## Lifecycle contract
//!
//! - Records are read non-destructively. The source MUST NOT drain
//!   entries while a callback is in flight; the aggregator does not
//!   take ownership.
//! - Implementations callback once per stored record, exposing the
//!   `Instant` the record was inserted alongside the record borrow.
//!   The instant is the discriminator the aggregator uses to honour
//!   [`BatchAuditAggregator::summarize`]'s `since` parameter.
//! - Callback ordering is implementation-defined; the aggregator
//!   does not rely on it.

use crate::instant::Instant;
use crate::payloads::bundler::{BundlerBatchPayload, SlowRecordSummary};
use crate::payloads::tags::BundlerKindTag;
use crate::record::{RequestAuditRecord, RequestKind};

/// Maximum number of slow records carried in
/// [`BundlerBatchPayload::slowest_5`].
pub const SLOWEST_RECORD_LIMIT: usize = 5;

/// Read-only iteration trait over a host's recent audit records.
///
/// Implementations expose every currently-stored record to the
/// supplied callback together with the `Instant` the record was
/// inserted. The callback runs once per record under whatever
/// synchronisation the implementation needs to keep iteration safe.
///
/// The trait is intentionally callback-shaped (rather than returning
/// an iterator) so implementations are free to hold an internal lock
/// across iteration without leaking the lock guard outside the trait
/// boundary.
pub trait AuditRecordSource {
    /// Apply `f` to every stored record in any order. The
    /// implementation guarantees the borrow is valid for the
    /// duration of the call.
    fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord));
}

/// Folds the records exposed by an [`AuditRecordSource`] into a
/// [`BundlerBatchPayload`]. The aggregator borrows the source for
/// the lifetime of the wrapper and does not own it; recreate the
/// wrapper if the source is replaced.
pub struct BatchAuditAggregator<'a> {
    source: &'a dyn AuditRecordSource,
    kind: BundlerKindTag,
}

impl<'a> BatchAuditAggregator<'a> {
    /// Construct an aggregator that summarises records from `source`
    /// and tags the resulting payload with `kind`. The kind reflects
    /// WHO requested the summary (vite, webpack, …) — it does not
    /// filter the records the source yields.
    pub fn new(source: &'a dyn AuditRecordSource, kind: BundlerKindTag) -> Self {
        Self { source, kind }
    }

    /// Build a [`BundlerBatchPayload`] over the source's current
    /// record window.
    ///
    /// `since` filters records inserted strictly after the supplied
    /// `Instant`. `None` includes every record the source yields.
    /// An empty source produces a zeroed payload (no division-by-
    /// zero on `cache_hit_rate`).
    pub fn summarize(&self, since: Option<Instant>) -> BundlerBatchPayload {
        let mut total_records: u32 = 0;
        let mut component_meta_count: u32 = 0;
        let mut compile_count: u32 = 0;
        let mut type_resolution_count: u32 = 0;
        let mut semantic_analysis_count: u32 = 0;
        let mut workspace_count: u32 = 0;
        let mut lsp_count: u32 = 0;
        let mut mcp_count: u32 = 0;
        let mut bundler_batch_count: u32 = 0;
        let mut custom_count: u32 = 0;
        let mut typeinfo_graph_count: u32 = 0;
        let mut total_duration_ms: f64 = 0.0;
        let mut total_bytes_parsed: u64 = 0;
        let mut from_cache_count: u32 = 0;
        // Accumulated slowest-record candidates. We keep the running
        // top-N in a small Vec sorted descending by `total_ms`; once
        // it exceeds `SLOWEST_RECORD_LIMIT` we drop the smallest
        // entry. Using a plain Vec keeps deterministic ordering for
        // ties (insertion-order by callback) which is friendlier for
        // testing than a BinaryHeap.
        let mut slowest: Vec<SlowRecordSummary> = Vec::with_capacity(SLOWEST_RECORD_LIMIT + 1);

        let mut visit = |inserted_at: Instant, record: &RequestAuditRecord| {
            if let Some(threshold) = since {
                if inserted_at <= threshold {
                    return;
                }
            }
            total_records = total_records.saturating_add(1);
            match &record.kind {
                RequestKind::ComponentMeta => {
                    component_meta_count = component_meta_count.saturating_add(1)
                }
                RequestKind::TypeResolution => {
                    type_resolution_count = type_resolution_count.saturating_add(1)
                }
                RequestKind::SemanticAnalysis => {
                    semantic_analysis_count = semantic_analysis_count.saturating_add(1)
                }
                RequestKind::Compile { .. } => compile_count = compile_count.saturating_add(1),
                RequestKind::Workspace { .. } => {
                    workspace_count = workspace_count.saturating_add(1)
                }
                RequestKind::Lsp { .. } => lsp_count = lsp_count.saturating_add(1),
                RequestKind::Mcp { .. } => mcp_count = mcp_count.saturating_add(1),
                RequestKind::BundlerBatch { .. } => {
                    bundler_batch_count = bundler_batch_count.saturating_add(1)
                }
                RequestKind::Custom { .. } => custom_count = custom_count.saturating_add(1),
                RequestKind::TypeInfoGraph => {
                    typeinfo_graph_count = typeinfo_graph_count.saturating_add(1)
                }
            }
            total_duration_ms += record.timings.total_ms;
            total_bytes_parsed = total_bytes_parsed.saturating_add(record.memory.bytes_parsed);
            if record.from_cache {
                from_cache_count = from_cache_count.saturating_add(1);
            }
            // Maintain top-N descending by duration.
            let summary = SlowRecordSummary {
                request_id: record.request_id,
                canonical_id: record.canonical_id.clone(),
                kind: record.kind.clone(),
                duration_ms: record.timings.total_ms,
            };
            // Find the first entry strictly slower than `summary`
            // (descending order); insert before it.
            let insert_at = slowest
                .iter()
                .position(|existing| existing.duration_ms < summary.duration_ms)
                .unwrap_or(slowest.len());
            if insert_at < SLOWEST_RECORD_LIMIT || slowest.len() < SLOWEST_RECORD_LIMIT {
                slowest.insert(insert_at, summary);
                if slowest.len() > SLOWEST_RECORD_LIMIT {
                    slowest.truncate(SLOWEST_RECORD_LIMIT);
                }
            }
        };
        self.source.for_each_record(&mut visit);

        let cache_hit_rate = if total_records == 0 {
            0.0
        } else {
            from_cache_count as f32 / total_records as f32
        };

        BundlerBatchPayload {
            kind: self.kind.clone(),
            total_records,
            component_meta_count,
            compile_count,
            type_resolution_count,
            semantic_analysis_count,
            workspace_count,
            lsp_count,
            mcp_count,
            bundler_batch_count,
            custom_count,
            typeinfo_graph_count,
            total_duration_ms,
            total_bytes_parsed,
            from_cache_count,
            cache_hit_rate,
            slowest_5: slowest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payloads::compile::CompilePayload;
    use crate::payloads::component_meta::ComponentMetaPayload;
    use crate::payloads::tags::CompileTargetTag;
    use crate::record::RequestKindPayload;

    /// Trivial in-memory source for unit-level tests inside
    /// `verter_audit`. The full integration test lives at
    /// `crates/verter_audit/tests/batch_aggregator.rs`.
    struct VecSource {
        entries: Vec<(Instant, RequestAuditRecord)>,
    }

    impl AuditRecordSource for VecSource {
        fn for_each_record(&self, f: &mut dyn FnMut(Instant, &RequestAuditRecord)) {
            for (instant, record) in &self.entries {
                f(*instant, record);
            }
        }
    }

    fn record(
        request_id: u64,
        kind: RequestKind,
        total_ms: f64,
        from_cache: bool,
    ) -> RequestAuditRecord {
        let mut rec = RequestAuditRecord {
            request_id,
            canonical_id: format!("/req-{request_id}.vue"),
            kind: kind.clone(),
            parent_request_id: None,
            from_cache,
            timings: Default::default(),
            memory: Default::default(),
            store: Default::default(),
            footprint: None,
            scheduler: None,
            files: Vec::new(),
            waits: None,
            kind_payload: match kind {
                RequestKind::ComponentMeta => {
                    RequestKindPayload::ComponentMeta(ComponentMetaPayload::default())
                }
                RequestKind::Compile { .. } => {
                    RequestKindPayload::Compile(CompilePayload::default())
                }
                _ => RequestKindPayload::None,
            },
            trace_id: String::new(),
        };
        rec.timings.total_ms = total_ms;
        rec
    }

    #[test]
    fn empty_source_yields_zeroed_payload() {
        let source = VecSource {
            entries: Vec::new(),
        };
        let agg = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
        let payload = agg.summarize(None);
        assert_eq!(payload.total_records, 0);
        assert_eq!(payload.component_meta_count, 0);
        assert_eq!(payload.total_duration_ms, 0.0);
        assert_eq!(payload.from_cache_count, 0);
        assert_eq!(payload.cache_hit_rate, 0.0);
        assert!(payload.slowest_5.is_empty());
    }

    #[test]
    fn slowest_5_is_descending_and_capped() {
        let now = Instant::now();
        let entries: Vec<(Instant, RequestAuditRecord)> = (1..=10)
            .map(|i| {
                let rec = record(i, RequestKind::ComponentMeta, i as f64 * 7.5, false);
                (now, rec)
            })
            .collect();
        let source = VecSource { entries };
        let agg = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
        let payload = agg.summarize(None);
        assert_eq!(payload.total_records, 10);
        assert_eq!(payload.component_meta_count, 10);
        assert_eq!(payload.slowest_5.len(), SLOWEST_RECORD_LIMIT);
        // Descending top-5 of an arithmetic sequence 7.5, 15.0, ..., 75.0
        let expected: Vec<f64> = (6..=10).rev().map(|i| i as f64 * 7.5).collect();
        let actual: Vec<f64> = payload.slowest_5.iter().map(|s| s.duration_ms).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn since_filter_excludes_strictly_older_entries() {
        let early = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let watermark = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let late = Instant::now();
        let source = VecSource {
            entries: vec![
                (early, record(1, RequestKind::ComponentMeta, 1.0, false)),
                (late, record(2, RequestKind::ComponentMeta, 2.0, false)),
            ],
        };
        let agg = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
        let payload = agg.summarize(Some(watermark));
        assert_eq!(payload.total_records, 1);
        assert_eq!(payload.slowest_5.len(), 1);
        assert_eq!(payload.slowest_5[0].request_id, 2);
    }

    #[test]
    fn cache_hit_rate_aggregates_from_cache_field() {
        let now = Instant::now();
        let source = VecSource {
            entries: vec![
                (now, record(1, RequestKind::ComponentMeta, 1.0, true)),
                (now, record(2, RequestKind::ComponentMeta, 1.0, true)),
                (now, record(3, RequestKind::ComponentMeta, 1.0, false)),
                (now, record(4, RequestKind::ComponentMeta, 1.0, false)),
            ],
        };
        let agg = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
        let payload = agg.summarize(None);
        assert_eq!(payload.total_records, 4);
        assert_eq!(payload.from_cache_count, 2);
        assert!((payload.cache_hit_rate - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn per_kind_counters_partition_total_records() {
        let now = Instant::now();
        let source = VecSource {
            entries: vec![
                (now, record(1, RequestKind::ComponentMeta, 1.0, false)),
                (now, record(2, RequestKind::TypeResolution, 1.0, false)),
                (now, record(3, RequestKind::SemanticAnalysis, 1.0, false)),
                (
                    now,
                    record(
                        4,
                        RequestKind::Compile {
                            target: CompileTargetTag::Ide,
                        },
                        1.0,
                        false,
                    ),
                ),
                (now, record(5, RequestKind::TypeInfoGraph, 1.0, false)),
            ],
        };
        let agg = BatchAuditAggregator::new(&source, BundlerKindTag::Vite);
        let payload = agg.summarize(None);
        assert_eq!(payload.total_records, 5);
        assert_eq!(payload.component_meta_count, 1);
        assert_eq!(payload.type_resolution_count, 1);
        assert_eq!(payload.semantic_analysis_count, 1);
        assert_eq!(payload.compile_count, 1);
        assert_eq!(payload.typeinfo_graph_count, 1);
        let sum = payload.component_meta_count
            + payload.compile_count
            + payload.type_resolution_count
            + payload.semantic_analysis_count
            + payload.workspace_count
            + payload.lsp_count
            + payload.mcp_count
            + payload.bundler_batch_count
            + payload.custom_count
            + payload.typeinfo_graph_count;
        assert_eq!(sum, payload.total_records);
    }
}
