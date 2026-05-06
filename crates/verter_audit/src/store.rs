#![deny(missing_docs)]
//! Generic store / view counters carried on every audit record
//! envelope. Kind-specific store counters (notably the materializer
//! and dep-signature lock counters) live in
//! [`crate::payloads::ComponentMetaPayload`].

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Per-cache hit/miss attribution for a single cache layer, scoped to
/// the request that produced this audit record. Snapshotted from the
/// session-side `RequestContext::cache_counters` field at request
/// finalisation, so the values are exact deltas for THIS request only
/// (no host-global accumulation, no cross-request leakage). Bumped by
/// the cache's get/insert boundary when a `RequestContext` is
/// installed in TLS.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct CacheLayerHitMiss {
    /// Hits observed on this cache layer during the request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub hits: u64,
    /// Misses observed on this cache layer during the request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub misses: u64,
}

pub struct CacheLayerBreakdown {
    /// `IndexedReadyDb` — canonical post-parse artifact cache.
    pub indexed: CacheLayerHitMiss,
    /// `AnalysisReadyDb` — analysis-stage artifact cache.
    pub analysis: CacheLayerHitMiss,
    /// `OwnerImportSurfaceDb` — owner direct-import surface cache.
    pub owner_import: CacheLayerHitMiss,
    /// `RouteOwnedShallowDb` — route-only shallow cache.
    pub route_owned_shallow: CacheLayerHitMiss,
    /// `ComponentMetaResultDb` — final component-meta result cache.
    pub component_meta: CacheLayerHitMiss,
    /// `RouteDb` — host-backed resolver route cache.
    pub route_db: CacheLayerHitMiss,
    /// `RefCycleResultDb` — transitive-cycle result cache for
    /// parameterized generic helpers.
    pub ref_cycle: CacheLayerHitMiss,
    /// `IntrinsicRegistry` — intrinsic dispatch lookup cache.
    pub intrinsic_registry: CacheLayerHitMiss,
    /// `SemanticGraphStore` — semantic-query memo / graph cache.
    pub semantic_graph: CacheLayerHitMiss,
    /// `MaterializeStructureDb` — structural materialisation cache.
    pub materialize_structure: CacheLayerHitMiss,
    /// `MaterializeMemoDb` — materialiser memo cache.
    pub materialize_memo: CacheLayerHitMiss,
    /// `MemberRouteResultDb` — macro-member walker route-result cache
    /// keyed on `(scope, member_name, lowered, mode)`. Hits indicate
    /// the route-candidate builder + per-candidate `until_stable`
    /// recursion was short-circuited inside the macro-member walker.
    pub member_route_result: CacheLayerHitMiss,
    /// `PreparedSurfaceDb` — prepared-surface cache.
    pub prepared_surface: CacheLayerHitMiss,
    /// `PreparedMemberDb` — prepared-member cache.
    pub prepared_member: CacheLayerHitMiss,
}

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
    /// Per-cache hit/miss breakdown for this request.
    /// Snapshotted at request finalisation from the session-side
    /// `RequestContext::cache_counters`. Each field is a
    /// this-request-only delta.
    #[serde(default)]
    pub cache_layers: CacheLayerBreakdown,
}
