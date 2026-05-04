#![deny(missing_docs)]
//! [`ComponentMetaPayload`] — strongly-typed payload for
//! `RequestKind::ComponentMeta`.
//!
//! Materialiser-specific store counters and solver counters live
//! here rather than on the generic
//! [`crate::store::RequestStoreAudit`] envelope so the envelope
//! stays kind-agnostic.

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Component-meta request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ComponentMetaPayload {
    /// Total solver resolve-steps issued across all invocations
    /// during this request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_resolve_steps: u64,
    /// Number of solver invocations during this request.
    pub solve_count: u32,
    /// Total `materialize_component_meta_structure` invocations
    /// observed during the request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_calls: u64,
    /// Subset of `materialize_structure_calls` that were satisfied by
    /// the materialiser's `MaterializeStructureDb` peek (warm cache
    /// hit).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_cache_hits: u64,
    /// Lock acquisitions on the per-scope `NodeArena` dedup index.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub node_arena_lock_acquisitions: u64,
    /// Lock acquisitions on the family-map dep-signature reverse
    /// index.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub family_map_lock_acquisitions: u64,
    /// Times a `dep_signature` was merged into the materialiser's
    /// `local_fence`.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_merges: u64,
    /// Subset of `dep_signature_merges` that hit an existing intern
    /// bucket (avoided allocation).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_intern_hits: u64,
}
