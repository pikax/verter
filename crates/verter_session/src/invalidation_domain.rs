//! Typed cache invalidation domains for [`crate::project_type_store::ProjectTypeStore`].
//!
//! Every host-owned cache DB on `ProjectTypeStore` participates in
//! invalidation under a fixed set of domains. The domains describe
//! *what kind of change* invalidates the DB, decoupled from the
//! per-DB invalidation surface.
//!
//! # Design
//!
//! The object-safe [`ParticipatesInInvalidation`] trait lets every DB
//! declare which [`InvalidationDomain`] variants it participates in.
//! Iteration via `&dyn ParticipatesInInvalidation` lets the cascade
//! dispatch through one homogeneous list — see
//! [`crate::project_type_store::ProjectTypeStore::all_dbs_for_invalidation`].
//!
//! The companion monomorphic per-canonical drain trait
//! [`InvalidationByCanonical`] is statically dispatched so each DB
//! can pick its own internal key shape. The two-trait split keeps
//! `ParticipatesInInvalidation` object-safe (no associated types).
//!
//! Both surfaces share the inventory recorded by
//! [`crate::project_type_store::PROJECT_TYPE_STORE_DB_INVENTORY`]: a
//! new DB is registered by adding one line to the inventory and one
//! entry to
//! [`crate::project_type_store::ProjectTypeStore::all_dbs_for_invalidation`].
//! The source-structure guard
//! `every_db_field_in_project_type_store_appears_in_inventory` in
//! `tests/architecture_guards.rs` parses the struct and asserts every
//! DB-typed field is registered.
//!
//! # Domain semantics
//!
//! - [`InvalidationDomain::FileContent`] — the DB caches results
//!   that depend on a canonical file's content hash. A file edit
//!   invalidates entries whose dep-signature references the
//!   canonical id.
//! - [`InvalidationDomain::TypeGraph`] — the DB caches semantic
//!   graph nodes / memo entries. Invalidated by content changes
//!   reaching the file's declaration graph.
//! - [`InvalidationDomain::ResolverState`] — the DB caches resolver
//!   route / barrel results. Invalidated by route-surface shifts
//!   (driven by content edits or project-shape changes).
//! - [`InvalidationDomain::ComponentMeta`] — the DB caches
//!   component-meta results / supporting projections.
//! - [`InvalidationDomain::ProjectGeneration`] — the DB depends on
//!   project shape (tsconfig, path aliases, active TS SDK,
//!   workspace folders). A `bump_project_generation` invalidates
//!   it wholesale.
//! - [`InvalidationDomain::AppConfigInterfaceMerge`] — the DB's
//!   correctness depends on the workspace-level
//!   `interface AppConfig` merge state. Bumped only when
//!   (a) `IndexedReady::declares_interface_app_config` transitions,
//!   or (b) a flagged file's content_hash changes. The host's
//!   incrementality contract guarantees O(1) cost per upsert.

/// Categorical classification of what triggers invalidation for a
/// host-owned DB. A DB declares the set of domains it participates
/// in; a cascade method dispatches by domain.
///
/// Variants are deliberately ordered to mirror the cascade reading
/// order: file content first (most frequent), project-shape last
/// (rarest), workspace-level `AppConfig` interface merge isolated
/// at the end as the most narrowly-scoped facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidationDomain {
    /// File-content-keyed entries (canonical id `whole_hash`).
    FileContent,
    /// Semantic-graph nodes / memo entries.
    TypeGraph,
    /// Resolver route / barrel cache entries.
    ResolverState,
    /// Component-meta result / projection entries.
    ComponentMeta,
    /// Project-shape-keyed entries (tsconfig, path aliases, active
    /// TS SDK, workspace folders).
    ProjectGeneration,
    /// Workspace-level `interface AppConfig` merge state.
    AppConfigInterfaceMerge,
}

impl InvalidationDomain {
    /// All domains, in canonical order. Used by tests and diagnostic
    /// surfaces that need a stable enumeration.
    pub const ALL: &'static [InvalidationDomain] = &[
        InvalidationDomain::FileContent,
        InvalidationDomain::TypeGraph,
        InvalidationDomain::ResolverState,
        InvalidationDomain::ComponentMeta,
        InvalidationDomain::ProjectGeneration,
        InvalidationDomain::AppConfigInterfaceMerge,
    ];
}

/// Object-safe view of "this DB participates in invalidation under
/// these domains". The trait is callable through
/// `&dyn ParticipatesInInvalidation`, so
/// [`crate::project_type_store::ProjectTypeStore::all_dbs_for_invalidation`]
/// can return a homogeneous list.
///
/// `invalidate(domain)` is the wholesale-by-domain entry point used
/// by the project-generation cascade. Per-canonical eviction is
/// layered on top of this trait via the companion
/// [`InvalidationByCanonical`] trait — kept separate so
/// `ParticipatesInInvalidation` stays object-safe (no associated
/// types).
pub trait ParticipatesInInvalidation: Send + Sync {
    /// The domain set this DB declares membership in. Read by the
    /// guard test asserting macro-coverage; consulted by the
    /// cascade methods so a future per-domain invalidation only
    /// touches the relevant DBs.
    fn domains(&self) -> &'static [InvalidationDomain];

    /// Wholesale invalidation triggered by a domain change that
    /// cannot be narrowed to a single canonical id. The default
    /// behaviour is `invalidate_all` for the matched domain — DBs
    /// that legitimately ignore some domain transitions override
    /// this with a no-op for that domain.
    fn invalidate(&self, domain: InvalidationDomain);
}

// ---------------------------------------------------------------------------
// InvalidationByCanonical — monomorphic per-DB per-canonical eviction
// ---------------------------------------------------------------------------

/// Monomorphic per-DB trait for "drop every entry keyed on this
/// canonical id". Statically dispatched so each implementing DB can
/// choose its own internal key shape (a tuple, a struct, an
/// `Arc<str>`, etc.) without forcing object-safety constraints on the
/// homogeneous [`ParticipatesInInvalidation`] surface.
///
/// Returns the number of entries actually dropped, so the cascade can
/// surface a deterministic count for the §16.1 capture-token-based
/// regression test (`invalidate_canonical_for` must visit O(K)
/// entries owned by the canonical, NOT O(N) total entries).
///
/// # Indexing contract
///
/// Implementations that own a per-canonical reverse index (see
/// [`CanonicalReverseIndex`]) MUST drain via the index. Linear scans
/// over the entire entry table are permitted only for DBs whose
/// underlying primitive does not surface a per-canonical drain (in
/// which case the cascade still works — the count is whatever the
/// linear sweep dropped — but the perf contract is per-DB).
pub trait InvalidationByCanonical {
    /// Drop every entry owned by `canonical_id`. Returns the number
    /// of entries actually evicted.
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize;
}

// ---------------------------------------------------------------------------
// CanonicalReverseIndex — per-canonical secondary index for O(K) drain
// ---------------------------------------------------------------------------

use std::hash::Hash;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxHashSet, FxHasher};

/// Plan §12.A12 step 1 — per-DB secondary index that maps a canonical
/// id to the set of DB-internal keys belonging to that canonical.
///
/// Populated on every cooperative-admission `post_publish` (or on the
/// test-only direct-insert path) so that
/// `invalidate_canonical_for(canonical_id)` drains in O(K) where K is
/// the number of entries owned by the canonical, instead of O(N) over
/// the whole DB.
///
/// `Send + Sync` and lock-free under read/write (DashMap shards). The
/// inner per-canonical set is wrapped in `parking_lot::Mutex` because
/// concurrent post-publish + drain on the same canonical needs a
/// linearisation point.
pub struct CanonicalReverseIndex<K>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
{
    inner: DashMap<
        Arc<str>,
        parking_lot::Mutex<FxHashSet<K>>,
        std::hash::BuildHasherDefault<FxHasher>,
    >,
}

impl<K> CanonicalReverseIndex<K>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
{
    /// Construct an empty reverse index.
    pub fn new() -> Self {
        Self {
            inner: DashMap::with_hasher(std::hash::BuildHasherDefault::<FxHasher>::default()),
        }
    }

    /// Register `key` under `canonical`. Idempotent — re-registering
    /// the same `(canonical, key)` pair is a no-op.
    pub fn register(&self, canonical: &Arc<str>, key: K) {
        let entry = self
            .inner
            .entry(Arc::clone(canonical))
            .or_insert_with(|| parking_lot::Mutex::new(FxHashSet::default()));
        let mut set = entry.lock();
        set.insert(key);
    }

    /// Remove the canonical's bucket and return every key that was
    /// registered under it. Drives the per-canonical drain; the
    /// per-DB caller is responsible for then dropping each key from
    /// the live entry table.
    ///
    /// The returned `Vec` length is K (entries owned by the
    /// canonical). The §16.1 capture-token counter
    /// `invalidate_canonical_entries_visited` is incremented by K
    /// so the regression test can assert the drain visited exactly
    /// K entries (NOT N).
    pub fn drain_for(&self, canonical_id: &str) -> Vec<K> {
        let removed = self.inner.remove(canonical_id);
        let keys: Vec<K> = match removed {
            Some((_, mutex)) => {
                let set = mutex.into_inner();
                set.into_iter().collect()
            }
            None => Vec::new(),
        };
        // §16.1 capture-token hook: surface the per-canonical visit
        // count so `invalidate_canonical_touches_only_indexed_entries`
        // can assert visited == K (NOT N). No-op on the production
        // hot path (no token bound).
        let visited = keys.len() as u64;
        crate::capture_token::with_active_capture(|t| {
            t.record_counter("invalidate_canonical_entries_visited", visited);
        });
        keys
    }

    /// Clear every bucket. Used by `invalidate_all` to keep the index
    /// coherent with the underlying entry table after a wholesale
    /// drop.
    pub fn clear(&self) {
        self.inner.clear();
    }
}

impl<K> Default for CanonicalReverseIndex<K>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
