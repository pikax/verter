//! Workspace-wide cache-cluster schema version.
//!
//! Every host-owned cache that stores a value transitively containing
//! analyzer / parser-published types (`Analyzed*Field`, `ResolvedLocalType`,
//! `ProjectedMacroSurfaces`, `ResolvedProp`, `ResolvedEmit`, etc.) participates
//! in a single shared schema version. When the wire shape of those types
//! changes, this constant is bumped and every participating cache must reject
//! any entry that was stored under a prior version on first read.
//!
//! ## Why a single shared constant
//!
//! Bumping per-Db version constants in lockstep is an ergonomic and a
//! correctness hazard: it is easy to forget one Db, leaving a partial cohort
//! that returns torn (some-fresh, some-stale) data on first read after a
//! schema bump. A single workspace-wide [`CACHE_CLUSTER_SCHEMA_VERSION`] cascades
//! to every Db wired through [`CacheSchemaVersioned`].
//!
//! ## Invalidation contract
//!
//! Every Db in the project-global cache cluster carries a `schema_version: u32`
//! captured at construction time. The Db's lookups treat any entry whose
//! schema_version differs from the constant as stale and refuse to return it.
//! In production, all Dbs are constructed from [`CACHE_CLUSTER_SCHEMA_VERSION`]
//! at process start, so the version always matches and the check is free.
//!
//! Test fixtures construct a Db with an explicit older version via the
//! `*_with_schema_version_for_test(version)` constructor, populate entries
//! through the public insertion path, then assert that reads against the
//! current version return `None` (eviction).
//!
//! ## What participates
//!
//! The full Db cohort that stores values transitively containing
//! analyzer-published or parser-published typed fields:
//!
//! | Db | File |
//! |----|------|
//! | `FileArtifactStore`           | `project_type_store.rs`         |
//! | `AnalysisReadyDb`          | `project_type_store.rs`         |
//! | `RouteOwnedShallowDb`      | `project_type_store.rs`         |
//! | `EvalEnvCacheDb`           | `project_type_store.rs`         |
//! | `ComponentMetaResultDb`    | `component_meta_result_db.rs`   |
//! | `OwnerImportSurfaceDb`     | `owner_import_surface.rs`       |
//! | `ImportedRegistryDb`       | `component_meta_caches.rs`      |
//! | `ShapeCacheDb`             | `component_meta_caches.rs`      |
//! | `MaterializeStructureDb`   | `component_meta_caches.rs`      |
//!
//! `RefCycleResultDb` (`component_meta_caches.rs`) is intentionally NOT
//! enrolled — it caches booleans / cycle identities only and carries no
//! analyzer-published typed fields.

/// Workspace-wide cache-cluster schema version.
///
/// Bumped whenever any analyzer / parser-published type embedded in a cached
/// value gains, drops, or renames a field — including the planned addition of
/// `*_expr` and `*_expr_scope` fields on `Analyzed*Field` /
/// `ResolvedLocalType` / `ProjectedMacroSurfaces` / `ResolvedProp` /
/// `ResolvedEmit`.
///
/// History:
/// - `1` — original cohort (`FileArtifactStore`, `AnalysisReadyDb`,
///   `ComponentMetaResultDb`, etc.) without per-Db schema gating; entries had
///   no embedded typed fields.
/// - `2` — typed-IR fields. Adds `*_expr` / `*_expr_scope` on the
///   analyzer/parser-published surfaces. Every cache that transitively stores
///   these structs rejects any entry stored under version `1`.
pub const CACHE_CLUSTER_SCHEMA_VERSION: u32 = 2;

/// Trait surface every participating Db implements. The implementation is a
/// trivial getter — the reason it exists at all is so the architecture-guard
/// (W0.4) and the cache_invariant_migration test fixtures can iterate the
/// cohort uniformly.
pub trait CacheSchemaVersioned {
    /// The schema version this Db was constructed with. Always equals
    /// [`CACHE_CLUSTER_SCHEMA_VERSION`] in production; test fixtures may
    /// construct a Db with an explicit older version to exercise the
    /// stale-entry eviction invariant.
    fn schema_version(&self) -> u32;

    /// Drain every cache entry currently stored at a stale schema version.
    /// Production code never calls this — every Db is constructed from
    /// [`CACHE_CLUSTER_SCHEMA_VERSION`] so its entries are always fresh.
    /// Test fixtures call this after planting stale entries to verify the
    /// stale-eviction invariant holds.
    ///
    /// Returns the number of entries evicted.
    fn evict_if_schema_mismatch(&self, current: u32) -> usize;
}
