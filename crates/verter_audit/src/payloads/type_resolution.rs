#![deny(missing_docs)]
//! [`TypeResolutionPayload`] — strongly-typed payload for
//! `RequestKind::TypeResolution`. The substrate ships the data
//! structure; producers in `verter_session::resolver_core` populate
//! it once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::ProjectionModeTag;

/// Type-resolution request payload. Producers in
/// `verter_session::resolver_core` populate the counters once the
/// resolver entry-point emits through the audit substrate.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct TypeResolutionPayload {
    /// Projection mode the resolver ran with.
    pub query_mode: ProjectionModeTag,
    /// Number of resolver hops taken.
    pub hops: u32,
    /// Number of `Navigate` hops.
    pub navigations: u32,
    /// Number of `Expanded` / `Shallow` hops that allocated new nodes.
    pub expansions: u32,
    /// Number of conditional branch decisions resolved.
    pub conditional_decisions: u32,
    /// Number of `ref_root_reaches_transitive_cycle_node` cache hits.
    pub ref_root_cycle_hits: u32,
    /// Total projection ops executed against the projection-op budget.
    pub projection_ops_executed: u32,
    /// Maximum walker depth reached during the request.
    pub depth_high_water: u16,
    /// `true` when the depth budget was exceeded.
    pub recursion_limit_reached: bool,
}
