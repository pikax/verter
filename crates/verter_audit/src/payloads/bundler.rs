#![deny(missing_docs)]
//! [`BundlerBatchPayload`] — strongly-typed payload for
//! `RequestKind::BundlerBatch`. Producer crates populate the
//! `BatchAuditAggregator` that produces these payloads.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::BundlerKindTag;
use crate::record::u64_as_decimal_string;

/// Aggregate summary produced by the unplugin's batch run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct BundlerBatchPayload {
    /// Bundler kind (vite, webpack, …).
    pub kind: BundlerKindTag,
    /// Number of records in the batch.
    pub record_count: u32,
    /// Sum of `total_ms` across the batch.
    pub total_ms: f64,
    /// Sum of `bytes_parsed` across the batch.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_bytes_parsed: u64,
    /// Number of records with `from_cache = true`.
    pub from_cache_count: u32,
}
