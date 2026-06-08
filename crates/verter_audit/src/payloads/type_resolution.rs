#![deny(missing_docs)]
//! [`TypeResolutionPayload`] — strongly-typed payload for
//! `RequestKind::TypeResolution`. The substrate ships the data
//! structure; producers in `verter_session::resolver_core` populate
//! it once they emit through the audit substrate.

use serde::{Deserialize, Serialize};

use crate::payloads::component_meta::AuditDiagnosticEntry;
use crate::payloads::tags::ProjectionModeTag;

/// Type-resolution request payload. Producers in
/// `verter_session::resolver_core` populate the counters once the
/// resolver entry-point emits through the audit substrate.
///
/// `walker_diagnostics` and `cache_suppress` mirror the parallel fields
/// on [`crate::payloads::component_meta::ComponentMetaPayload`] so
/// observers can correlate suppressed type-resolution publications with
/// the same diagnostic vocabulary they consume on the component-meta
/// side. Producers populate them at the typeinfo audit-emission
/// boundary in `verter_session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
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
    /// Walker / synthesis diagnostics surfaced by the resolver. Empty
    /// for clean resolutions; populated only when a producer routes
    /// structured diagnostics through the audit-emission boundary.
    ///
    /// Marked `#[serde(default)]` so existing audit corpus consumers
    /// that emit records without this field deserialize cleanly into
    /// an empty vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walker_diagnostics: Vec<AuditDiagnosticEntry>,
    /// `true` when the request was driven through a synthesis path that
    /// landed with `cache_suppress=true` and therefore made no
    /// synthesis-attributable warm-cache insertions. Mirror of the
    /// component-meta `cache_suppress` signal so observers can correlate
    /// suppressed type-resolution publications.
    ///
    /// Marked `#[serde(default)]` for the same compatibility reason as
    /// `walker_diagnostics`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache_suppress: bool,
    /// Bitmask of the `SemanticQueryKey` variants this resolution dispatched —
    /// bit `i` set iff a key whose tag has `bit_index() == i` dispatched at least
    /// once through the shared
    /// `ProjectSemanticDispatch::execute_via_cold_build_helper` cold-build choke
    /// point. Because both the `execute` trait method and the
    /// dep-signature-preserving `execute_read` subquery entry funnel through that
    /// helper, this is the COMPLETE dispatched-tag trace for the request — every
    /// variant dispatched anywhere, including nested reducer sub-dispatches that
    /// enter only via `execute_read` — distinct from the focused cold/warm
    /// hot-path counters that cover only a subset.
    /// `verter_session::SemanticQueryKeyTag::{bit_index, decode_dispatch_mask}`
    /// own the bit assignment + decode.
    ///
    /// Marked `#[serde(default)]` so pre-existing audit corpus records without
    /// this field deserialize cleanly to `0` (no trace recorded).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub semantic_query_dispatch_mask: u32,
}

/// `skip_serializing_if` helper — the default (no trace) mask is `0`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}
