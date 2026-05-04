#![deny(missing_docs)]
//! Generic store / view counters carried on every audit record
//! envelope. Kind-specific store counters (notably the materializer
//! and dep-signature lock counters) live in
//! [`crate::payloads::ComponentMetaPayload`].

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Generic store/view counters that apply across request kinds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestStoreAudit {
    /// Store-view cache hits.
    pub store_view_hits: u32,
    /// Store-view cache misses.
    pub store_view_misses: u32,
    /// Structural-merge count.
    pub structural_merges: u32,
    /// Imported-dependency entries touched.
    pub imported_dependency_entries: u32,
    /// Imported-dependency byte total.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub imported_dependency_bytes: u64,
    /// Prepared type declarations.
    pub prepared_type_decls: u32,
    /// Prepared value declarations.
    pub prepared_value_decls: u32,
}
