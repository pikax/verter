//! Workspace-wide cache-cluster schema version.
//!
//! Every host-owned cache that stores a value transitively containing
//! analyzer / parser-published types (`Analyzed*Field`, `ResolvedLocalType`,
//! `ResolvedNativeProp`, `ResolvedProp`, `ResolvedNamedCallSignature`, etc.) participates
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
//! | `ComponentMetaResultDb`    | `component_meta_result_db.rs`   |
//! | `OwnerImportSurfaceDb`     | `owner_import_surface.rs`       |
//! | `ImportedRegistryDb`       | `component_meta_caches.rs`      |
//! | `ShapeCacheDb`             | `component_meta_caches.rs`      |
//! | `MaterializeStructureDb`   | `component_meta_caches.rs`      |
//!
//! The retired `RefCycleResultDb` was intentionally NOT enrolled — it
//! cached booleans / cycle identities only and carried no
//! analyzer-published typed fields. Its replacement (the
//! `ClassifyMaterializationCycleGate` semantic-query family) lives in
//! the semantic memo, which is likewise outside this cluster.

/// Workspace-wide cache-cluster schema version.
///
/// Bumped whenever any analyzer / parser-published type embedded in a cached
/// value gains, drops, or renames a field — including the planned addition of
/// `*_expr` and `*_expr_scope` fields on `Analyzed*Field` /
/// `ResolvedLocalType` / `ResolvedNativeProp` / `ResolvedProp` /
/// `ResolvedNamedCallSignature`.
///
/// History:
/// - `1` — original cohort (`FileArtifactStore`, `AnalysisReadyDb`,
///   `ComponentMetaResultDb`, etc.) without per-Db schema gating; entries had
///   no embedded typed fields.
/// - `2` — typed-IR fields. Adds `*_expr` / `*_expr_scope` on the
///   analyzer/parser-published surfaces. Every cache that transitively stores
///   these structs rejects any entry stored under version `1`.
/// - `3` — `ShapeCacheDb` synthetic-deepen key identity. The synthetic
///   slot-binding subject roots on the content-free
///   `SyntheticBindingId` (via `ShapeCacheKey::synthetic_binding_whole`)
///   instead of a `SemanticNodeId(value_node)` arena ordinal. The
///   `ShapeSubject` key identity changed, so any stale entry stored under
///   the old ordinal key must fail closed.
/// - `4` — exact top-level lexical-owner identity across parser facts,
///   declaration preparation, route results, semantic query identities, and
///   module-augmentation contributors. Version `3` entries lack the owner
///   discriminator and must fail closed rather than alias module and instance
///   declarations with the same spelling.
/// - `5` — canonical resolved emit occurrences and occurrence-based callable
///   replay. Version `4` entries can carry ordinal/parallel-lane associations
///   and must fail closed.
/// - `6` — exact prop callable roles and package-backed Svelte `Snippet`
///   symbol identities. Version `5` entries lack the role fact and must fail
///   closed instead of inferring from display text.
/// - `7` — call-signature emit declaration spans. Version `6` analyzer-bearing
///   entries cannot prove an exact declaration-span JSDoc join.
/// - `8` — authored-only import targets. Version `7` shallow artifacts may
///   retain a resolved canonical beside the authored source specifier, creating
///   a second import-resolution authority; version `8` removes that field.
/// - `9` — canonical ordered object-spread programs replace cached eager/open
///   surface materializations. Version `8` entries cannot distinguish ordered
///   direct effects, correlated alternatives, or raw residual operands.
pub const CACHE_CLUSTER_SCHEMA_VERSION: u32 = 9;

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
