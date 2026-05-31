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
#[ts(export_to = "audit.generated.ts")]
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

/// Per-cache hit/miss breakdown for the cache layers participating
/// in the request's per-cache observability surface. Each field
/// mirrors a host-owned cache; the values are this-request-only
/// deltas snapshotted from the per-request counter array.
///
/// The joiner-accounting contract attributes:
/// - The cold winner records `cache: Miss` + `from_cache: false` and
///   bumps the cache layer's `misses` counter once on its TLS context.
/// - Each joiner records `cache: Hit (joined)` + `from_cache: true`
///   and bumps the cache layer's `hits` counter once on its TLS
///   context.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct CacheLayerBreakdown {
    /// `FileArtifactStore` — canonical post-parse artifact cache.
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
    /// `MemberShapeCacheDb` — per-member graph-native materialiser cache.
    pub member_shape_cache: CacheLayerHitMiss,
    /// Always-zero counter for the removed prepared-surface walker DB.
    /// Retained under the legacy name to preserve audit-harness JSON
    /// schema compatibility.
    pub prepared_surface: CacheLayerHitMiss,
    /// Always-zero counter for the removed prepared-member walker DB.
    /// Retained under the legacy name to preserve audit-harness JSON
    /// schema compatibility.
    pub prepared_member: CacheLayerHitMiss,
}

/// Rule-compliance diagnostic counters. Empirical instrumentation
/// that quantifies the bypass surfaces identified as residual
/// perf-gap suspects: per-request `HostStoreView::from_host`
/// builds, bare-host `ComponentMetaQueryEngine::new(...)`
/// constructions, and `ResolverContext::resolver_store_view()`
/// warm-hit validator rebuilds. Snapshotted from the session-side
/// `RequestContext::cache_counters.bypass_diagnostics` field at
/// request finalisation, so the values are exact deltas for THIS
/// request only (no host-global accumulation).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct BypassDiagnostics {
    /// `HostStoreView::from_host` invocation count on this request.
    /// The per-request hoist expects this to drop to a
    /// small constant; counts >1 reveal carriers that still build
    /// their own owned view.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_store_view_from_host_builds: u64,
    /// `ComponentMetaQueryEngine::new(ctx)` constructions on this
    /// request where `ctx.is_request_bound()` returned `false` —
    /// i.e. the engine was bound to a bare `&VerterHost` rather
    /// than a request-bound context. Final-state invariant: `0`.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub bare_engine_constructions: u64,
    /// `ResolverContext::resolver_store_view()` call count on this
    /// request. Each call rebuilds a full owned `HostStoreView`;
    /// warm-hit validator paths in `fact_signature_helpers`
    /// previously rebuilt on every cache lookup until the
    /// per-request hoist landed — this counter quantifies the
    /// residual rebuild surface.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub resolver_store_view_calls: u64,
}

/// Generic store/view counters that apply across request kinds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
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
    /// Rule-compliance diagnostic counters. See
    /// [`BypassDiagnostics`] for the per-counter contract.
    #[serde(default)]
    pub bypass_diagnostics: BypassDiagnostics,
}
