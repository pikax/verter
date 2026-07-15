//! `SessionView` — read-only view trait over the base host.
//!
//! `SessionView` is the read-only surface that resolver-tier code uses to
//! observe host state. It is the **read substrate** for queries; it never
//! mutates the host (R17). Two concrete impls cover the cases we care
//! about:
//!
//! - [`HostView`] — direct passthrough to the base [`VerterHost`]. The
//!   query reads come from the live host state. Used by overlay-free
//!   sessions and by background work that operates on the workspace's
//!   canonical content.
//! - [`OverlaidView`] — overlay-aware view backed by per-canonical
//!   overlay sources. Resolution falls through to the base host when the
//!   overlay map does not carry the requested canonical. Overlays never
//!   mutate the base host; they coexist with the base under different
//!   content hashes (R17).
//!
//! ## Architectural role
//!
//! The fact-based cache refactor consolidates session-side reads
//! onto this trait. The session view threads into the resolver-tier
//! `StoreView` via
//! [`HostStoreView::with_session_overlay`](crate::resolver_store::HostStoreView::with_session_overlay),
//! re-rooting overlay-bearing per-canonical snapshots so fact
//! validation observes the session's CURRENT content identity; the
//! host's overlay-mutation machinery is gone (R17). See
//! `/type-cache-architecture` and `/host-session` skills for the
//! architectural rules (R17–R20).
//!
//! ## What `SessionView` is NOT
//!
//! - It is NOT the cache-correctness oracle. Fact validation (per-cache
//!   `fact_dep_signature` matched against recorded facts) decides
//!   whether a cached entry is fresh (R19).
//! - It is NOT a thread-local. `SessionView` is passed explicitly
//!   through `ResolverContext` (R18). Thread-local "current view"
//!   globals — `_in_view` / `RequestStoreView` / `CURRENT_REQUEST_VIEW`
//!   — are forbidden by the
//!   [`request_view_is_retired_from_crate_sources`](crate::project_global_cache_tests)
//!   architecture guard.
//! - It is NOT a snapshot of `VerterHost` state. Methods route through
//!   the host's live accessors so that workspace-side updates are
//!   reflected without re-creating the view.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::file_artifact_store::ProjectIdentity;
use crate::types::Hash16;
use crate::VerterHost;

/// Five-way environment-hash carrier (R21).
///
/// Carries `[parse, resolve, type_, lib]` env-hash dimensions plus the
/// implicit project-identity context (held alongside on the view). The
/// `Default` impl returns an all-zero bundle and is reserved for test
/// fixtures + arch guards; production view constructors compose the
/// bundle from the workspace's published env-hash tables (see
/// [`crate::VerterHost::host_view_env_hashes`] and
/// [`crate::VerterHost::host_view_env_hashes_for`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EnvHashes {
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
}

/// Read-only view over the base host's source / artifact state.
///
/// All resolver-tier queries route their host reads through this
/// trait. The implementations are [`HostView`] (passthrough to
/// `VerterHost`) and [`OverlaidView`] (per-canonical source overlays
/// stacked over a base host).
///
/// Lifetime + ownership: `SessionView` is passed by reference
/// (`&dyn SessionView`) so consumers do not hold strong references
/// to the underlying host beyond the call chain. Both concrete
/// impls hold `Arc<VerterHost>` internally so cross-thread
/// resolver work can clone the impl cheaply when needed.
pub trait SessionView: Send + Sync {
    /// Return the source for a canonical id if the view knows about
    /// it. Returns `None` for canonicals not yet ingested or
    /// (`OverlaidView` only) explicitly overlay-deleted.
    ///
    /// Overlays are checked before the base host. Base reads come
    /// from the host's shared file-cache; the view never mutates
    /// the host on read.
    fn source(&self, canonical: &str) -> Option<Arc<str>>;

    /// View-authoritative **current** content hash of `canonical`, if
    /// known.
    ///
    /// This is the hash of the exact bytes [`Self::source`] returns for
    /// `canonical` — the two methods agree on freshness by contract. An
    /// overlay-covered canonical resolves to the overlay source's hash;
    /// every other canonical resolves to the base host's
    /// scheduler-authoritative current content hash
    /// ([`crate::VerterHost::authoritative_current_content_hash`]).
    ///
    /// The base fallthrough is the **scheduler authority**, never a
    /// permissive `FileArtifactStore` scan. A stale pre-edit
    /// `IndexedReady` lingering in
    /// [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
    /// past a same-canonical edit (lazy invalidation) does NOT surface
    /// here — an evicted / deleted canonical reports `None`, and a live
    /// canonical reports the current hash, so a content-pinned lookup
    /// keyed by this value never resolves a stale artifact via its own
    /// hash.
    fn content_hash_for(&self, canonical: &str) -> Option<Hash16>;

    /// Content hash of the **explicit overlay** covering `canonical`,
    /// or `None` when this view carries no overlay for it.
    ///
    /// Distinct from [`Self::content_hash_for`]: that method reports
    /// the view-authoritative current content hash for every known
    /// canonical (overlay hash when masked, scheduler-authoritative
    /// base hash otherwise). This method reports `Some` **only** when
    /// the session has installed an overlay-Upsert for `canonical` —
    /// the overlay source's hash, which the overlay `IndexedReady`
    /// candidate was prewarmed under — and `None` for an unmasked
    /// canonical even when the base host knows it. Consumers that must
    /// distinguish "an explicit overlay covers this" from "the base
    /// host knows this" (overlay-detection in
    /// [`crate::host_manage::overlay_priority`]) consult this method,
    /// not `content_hash_for`.
    ///
    /// Overlay-tombstoned canonicals report `None` (the session
    /// deleted the file — there is no overlay content).
    ///
    /// Default returns `None` — base-only views ([`HostView`],
    /// [`HostViewRef`]) never carry overlays.
    fn overlay_content_hash_for(&self, _canonical: &str) -> Option<Hash16> {
        None
    }

    /// Session-overlay artifact-store discriminator for `canonical`.
    ///
    /// Returns `Some(discriminator)` **only** when this view carries an
    /// explicit overlay for `canonical` — the same condition under
    /// which [`Self::overlay_content_hash_for`] reports `Some`. The
    /// discriminator is a non-zero [`Hash16`] derived from the view's
    /// overlay-set [`Self::fingerprint`]; it is the `parse_env_hash`
    /// dimension of [`crate::file_artifact_store::FileArtifactKey::overlay_scoped`].
    ///
    /// Purpose: an overlay `IndexedReady` whose source bytes are
    /// identical to the base file has a content hash equal to the
    /// base hash, so a [`crate::file_artifact_store::FileArtifactKey::base`]
    /// key for it would collide with the base artifact. The overlay
    /// materialiser can resolve a relative import to an overlay-only
    /// helper the base workspace cannot see, so the overlay's import
    /// routes genuinely diverge from the base's — a collision would
    /// either poison base reads with session routes or silently serve
    /// the overlay base routes. Keying the overlay artifact under
    /// `overlay_scoped(canonical, hash, discriminator)` keeps it
    /// isolated from the base (`parse_env_hash = BASE_PARSE_ENV_HASH`)
    /// and from other sessions (distinct overlay-set fingerprints).
    ///
    /// Default returns `None` — base-only views ([`HostView`],
    /// [`HostViewRef`]) never carry overlays, so their reads stay on
    /// the base key. Overlay-bearing views ([`OverlaidView`],
    /// [`OverlaidViewRef`]) override this.
    fn overlay_artifact_discriminator(&self, _canonical: &str) -> Option<Hash16> {
        None
    }

    /// Project identity (16-byte stable key) for this view's
    /// project.
    fn project_identity(&self) -> ProjectIdentity;

    /// Five-way environment-hash bundle for this view (R21).
    fn env_hashes(&self) -> &EnvHashes;

    /// Whether the view explicitly tombstones (overlay-Deletes) the
    /// given canonical.
    ///
    /// Distinguishes "canonical was deleted by this session" from
    /// "canonical is unknown / not yet loaded". Consumer paths that
    /// short-circuit on tombstones (e.g.,
    /// [`crate::VerterHost::get_component_meta_via_view`]) consult
    /// this instead of inferring tombstoning from
    /// `source().is_none()` + `content_hash_for().is_none()` (which
    /// would also fire for "canonical not loaded yet").
    ///
    /// Default returns `false` — base-only views (`HostView`,
    /// `HostViewRef`) never tombstone.
    fn is_tombstoned(&self, _canonical: &str) -> bool {
        false
    }

    /// Cached resolved-import facts for a canonical id under this
    /// view, if present.
    ///
    /// Looks up the
    /// [`crate::resolved_import_facts::ResolvedImportFactsDb`]
    /// slot keyed by
    /// `(canonical, content_hash_for(canonical),
    /// env_hashes().parse_env_hash,
    /// env_hashes().resolve_env_hash,
    /// RESOLVED_IMPORT_FACTS_RESOLVER_VERSION)`. Returns `None`
    /// when the cache has not been populated for that quintuple —
    /// the resolver populates entries on first demand and
    /// downstream consumers re-read from here instead of
    /// re-walking the AST.
    ///
    /// `lib_env_hash` is intentionally absent from the cache key
    /// (R21 scoping rule — base import-target resolution does not
    /// depend on TS lib data).
    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>>;

    /// Stable fingerprint identifying the overlay set on this view.
    ///
    /// Two views with identical overlay sets (same canonical →
    /// content-hash mapping) return the same fingerprint; two views
    /// with different overlays return different fingerprints. Base-
    /// only views (no overlays) return `0`.
    ///
    /// Resolver-tier consumers fold the fingerprint into singleflight
    /// keys so two concurrent sessions with different overlays cannot
    /// coalesce onto the same in-flight build (R20 multi-candidate
    /// isolation). The default impl returns `0`; overlay-bearing
    /// implementations (`OverlaidView`, `OverlaidViewRef`) override
    /// it to hash their overlay-hash table.
    fn fingerprint(&self) -> u64 {
        0
    }

    /// Enumerate every canonical id the view carries an overlay
    /// source for. Used by the session-bearing query entry points
    /// to pre-warm overlay [`IndexedReady`](crate::project_type_store::IndexedReady)
    /// candidates for the owner and every dep the overlay set
    /// covers, before the resolver-tier reads kick in.
    ///
    /// Default impl returns an empty vector (base-only views have no
    /// overlays). Overlay-bearing implementations (`OverlaidView`,
    /// `OverlaidViewRef`) override it to return their overlay map's
    /// keys.
    ///
    /// This enumerates only overlay-*source* canonicals (overlay-Upsert
    /// entries) — a canonical the session DELETED but never re-upserted
    /// is reported by [`Self::tombstoned_canonicals`] instead.
    fn overlay_canonicals(&self) -> Vec<String> {
        Vec::new()
    }

    /// Enumerate every canonical id the view explicitly TOMBSTONES
    /// (overlay-Deletes).
    ///
    /// Distinct from [`Self::overlay_canonicals`]: that method yields
    /// only overlay-*source* keys. A canonical the session deleted but
    /// never re-upserted has no overlay source, so it is absent from
    /// `overlay_canonicals()` — only this accessor reports it.
    ///
    /// Validation-rebasing consumers
    /// ([`crate::resolver_store::HostStoreView::with_session_overlay`])
    /// iterate BOTH sets so a session-deleted canonical's base
    /// per-canonical snapshots (`whole_hashes` / `file_facts` /
    /// `derived_hashes`) are dropped from the validation view — a warm
    /// entry rooted on the now-deleted base file must miss, exactly as
    /// an edit invalidates it.
    ///
    /// Default impl returns an empty vector (base-only views never
    /// tombstone). Only `OverlaidViewRef` — the sole view that carries
    /// a tombstone set — overrides it.
    fn tombstoned_canonicals(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Shared lookup helper used by the base-only views
/// ([`HostView`], [`HostViewRef`]).
///
/// Resolves the `(canonical, content_hash, parse_env_hash,
/// resolve_env_hash, resolver_version)` quintuple from the host's
/// scheduler-cached source data + the supplied env hashes, then
/// reads from [`crate::resolved_import_facts::ResolvedImportFactsDb`].
///
/// The `content_hash` source mirrors the producer
/// (`admit_resolved_import_facts_for_owner`): both read the
/// scheduler-authoritative `parse.whole_hash` so the producer's
/// admission key and the view's lookup key reach the same `DashMap`
/// slot immediately after `upsert`, without waiting for the lazy
/// `IndexedReady` materialization through `ensure_indexed_ready_serve`. The
/// source is strict — [`current_content_hash_from_scheduler`] — with
/// no permissive `FileArtifactStore` fallback: a scan-derived stale
/// hash would resolve a stale `ResolvedImportFacts` entry instead of
/// missing.
///
/// Returns `None` when the scheduler has no current content hash for
/// `canonical` (unloaded / evicted / deleted), or when the cache has
/// not been populated for the resolved quintuple.
fn resolved_import_facts_via_host(
    base: &VerterHost,
    canonical: &str,
    env_hashes: &EnvHashes,
) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
    let content_hash = current_content_hash_from_scheduler(base, canonical)?;
    let known_miss_generation = known_miss_generation_tag_for_owner(base, canonical);
    let key = crate::resolved_import_facts::ResolvedImportFactsKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: env_hashes.parse_env_hash,
        resolve_env_hash: env_hashes.resolve_env_hash,
        resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        known_miss_generation,
    };
    base.project_type_store().resolved_import_facts().get(&key)
}

/// Resolve the owner's known-miss generation tag for composing
/// [`crate::resolved_import_facts::ResolvedImportFactsKey`].
///
/// Reads
/// [`DerivedRawState::import_routes_known_miss_recorded_at_generation`](crate::types::DerivedRawState)
/// and folds it via
/// [`crate::resolved_import_facts::compute_known_miss_generation_tag`].
/// When no `DerivedRawState` entry exists yet for the canonical (the
/// owner has never had its routes recorded), returns `[0u8; 16]` so
/// the lookup composes the same tag value as the producer's "no
/// known-misses" first call.
///
/// Used by the base-only views ([`HostView`], [`HostViewRef`]) AND
/// the base-fallthrough branch of the overlay views ([`OverlaidView`],
/// [`OverlaidViewRef`]) so the lookup key matches the producer
/// (`admit_resolved_import_facts_for_owner`) byte-for-byte
/// regardless of overlay state.
fn known_miss_generation_tag_for_owner(
    base: &VerterHost,
    canonical: &str,
) -> verter_semantic::analysis::Hash16 {
    let entry = base.derived_raw_cache().get(canonical);
    match entry {
        Some(e) => crate::resolved_import_facts::compute_known_miss_generation_tag(
            &e.import_routes_known_miss_recorded_at_generation,
        ),
        None => [0u8; 16],
    }
}

/// Resolve `canonical`'s **current** content hash from the scheduler
/// authority — the sole parse authority, available immediately
/// post-`upsert`.
///
/// Strict by contract: this delegates to
/// [`VerterHost::authoritative_current_content_hash`] (scheduler
/// `HostSourceData.parse.whole_hash`, gated on the `DerivedRawState`
/// entry being non-evicted) and has **no permissive fallback**. It
/// never derives a hash from a `FileArtifactStore` scan: a scan can
/// surface a stale pre-edit artifact's own hash, and feeding that into
/// a content-pinned `ResolvedImportFacts` lookup would resolve the
/// stale resolution instead of yielding a miss. When only a stale
/// artifact could answer — the canonical was evicted / deleted while
/// its `IndexedReady` lingers — this returns `None`, which is correct:
/// `ResolvedImportFacts` is produced keyed from the same scheduler
/// `parse.whole_hash`, so a `None` here is a true miss, not a lost
/// cache hit.
///
/// Used by `resolved_import_facts_via_host` to match the producer
/// (`admit_resolved_import_facts_for_owner`) on cache-key
/// `content_hash` composition.
fn current_content_hash_from_scheduler(
    base: &VerterHost,
    canonical: &str,
) -> Option<verter_semantic::analysis::Hash16> {
    base.authoritative_current_content_hash(canonical)
}

// ---------------------------------------------------------------------------
// HostView — direct passthrough to `VerterHost`.
// ---------------------------------------------------------------------------

/// Read-only view that forwards every method to the base [`VerterHost`].
///
/// `HostView` carries an `Arc<VerterHost>` so it can be cloned cheaply
/// across resolver threads. The base host is the read substrate;
/// `HostView` never wraps additional state.
///
/// Used by overlay-free sessions and by any caller that wants to
/// observe the host's canonical state without overlay layering.
#[derive(Clone)]
pub struct HostView {
    base: Arc<VerterHost>,
    env_hashes: EnvHashes,
}

impl HostView {
    /// Construct a `HostView` over the supplied host.
    ///
    /// The returned view's `env_hashes()` reports the workspace-default
    /// env-hash bundle (composed from workspace config); callers that
    /// have per-project env hashes use [`HostView::with_env_hashes`]
    /// instead.
    pub fn new(base: Arc<VerterHost>) -> Self {
        let env_hashes = base.host_view_env_hashes();
        Self { base, env_hashes }
    }

    /// Construct a `HostView` with explicit env hashes.
    ///
    /// Used by producers that carry real per-project env hashes from
    /// the workspace; callers without one go through
    /// [`HostView::new`] which composes the workspace-default bundle.
    #[allow(dead_code)]
    pub fn with_env_hashes(base: Arc<VerterHost>, env_hashes: EnvHashes) -> Self {
        Self { base, env_hashes }
    }

    /// Borrow the underlying host. Reserved for impls that need
    /// to reach the host directly (e.g., scheduler context
    /// construction); resolver-tier code should not use this.
    #[allow(dead_code)]
    pub fn host(&self) -> &VerterHost {
        &self.base
    }
}

impl SessionView for HostView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // View-authoritative current content hash. A base-only view
        // resolves it from the scheduler authority
        // (`authoritative_current_content_hash`) — never a permissive
        // `FileArtifactStore` scan, which could surface a stale
        // pre-edit artifact's hash and disagree with `source()`.
        // `None` for an unloaded / evicted / deleted canonical.
        self.base.authoritative_current_content_hash(canonical)
    }

    fn project_identity(&self) -> ProjectIdentity {
        // View-level (workspace-default) project identity. Per-canonical
        // resolution callers use `VerterHost::host_view_project_identity_for`.
        self.base.host_view_project_identity()
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        resolved_import_facts_via_host(self.base.as_ref(), canonical, &self.env_hashes)
    }
}

// ---------------------------------------------------------------------------
// OverlaidView — overlay-aware view stacking sources over a base host.
// ---------------------------------------------------------------------------

/// Read-only view that layers per-canonical overlay sources over a
/// base [`VerterHost`].
///
/// The overlay map is consulted first for `source` and
/// `content_hash_for`. Canonicals absent from the overlay fall
/// through to the base host. An overlay `IndexedReady` candidate is
/// materialised on demand and published into
/// [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
/// under an
/// [`overlay_scoped`](crate::file_artifact_store::FileArtifactKey::overlay_scoped)
/// key — the overlay content hash plus this view's overlay-set
/// discriminator — so it stays isolated from the base artifact even
/// when the overlay bytes are identical to the base file.
///
/// `OverlaidView` is `Send + Sync` because its overlay map is
/// behind an `Arc<FxHashMap>`; mutation happens by constructing a
/// new `Arc<FxHashMap>` and a new `OverlaidView` value (R17 —
/// overlays do not mutate the base host).
#[derive(Clone)]
pub struct OverlaidView {
    overlays: Arc<FxHashMap<String, Arc<str>>>,
    overlay_hashes: Arc<FxHashMap<String, Hash16>>,
    base: Arc<VerterHost>,
    env_hashes: EnvHashes,
    /// Overlay-set fingerprint, computed LAZILY on the first
    /// [`SessionView::fingerprint`] read via [`overlay_set_fingerprint`]
    /// over the immutable `overlay_hashes` map, then memoized for the
    /// view's lifetime — no per-call collect + sort + hash, and ZERO
    /// computations for a view that never reads the fingerprint (e.g. an
    /// analysis-only request, whose path never touches it). The overlay
    /// map is behind an `Arc` and never mutated in place (R17 — mutation
    /// builds a new view), so the lazily-computed value is correct for
    /// the view's whole lifetime. The cell is `OnceLock` (NOT `OnceCell`)
    /// because the view is shared across rayon worker threads during a
    /// batch and so MUST be `Sync`; concurrent first-readers are handled
    /// by `OnceLock::get_or_init` (one computes, the rest block then read
    /// the same value). `Clone` copies the cell's contents, so a cloned
    /// view either carries the already-computed value or recomputes the
    /// identical value on its own first read.
    overlay_set_fingerprint: std::sync::OnceLock<u64>,
}

impl OverlaidView {
    /// Construct an overlaid view from a map of canonical → source.
    ///
    /// Content hashes for the overlay are computed once via
    /// `crate::hash::content_hash_str`. This is cheap (xxh3 over
    /// the overlay source) and stable so concurrent reads see the
    /// same hash without re-hashing.
    pub fn new(base: Arc<VerterHost>, overlays: FxHashMap<String, Arc<str>>) -> Self {
        let mut overlay_hashes = FxHashMap::default();
        overlay_hashes.reserve(overlays.len());
        for (canonical, source) in &overlays {
            let hash = crate::hash::hash_16(source.as_bytes());
            overlay_hashes.insert(canonical.clone(), hash);
        }
        let env_hashes = base.host_view_env_hashes();
        Self {
            overlays: Arc::new(overlays),
            overlay_hashes: Arc::new(overlay_hashes),
            base,
            env_hashes,
            // Lazy: computed on the first `fingerprint()` read through the
            // single shared algorithm, then memoized. A view that never
            // reads the fingerprint never pays the collect + sort + hash.
            overlay_set_fingerprint: std::sync::OnceLock::new(),
        }
    }

    /// Variant that takes pre-computed overlay hashes (used by
    /// future caller pathways where the hash is already known).
    #[allow(dead_code)]
    pub fn with_overlay_hashes(
        base: Arc<VerterHost>,
        overlays: Arc<FxHashMap<String, Arc<str>>>,
        overlay_hashes: Arc<FxHashMap<String, Hash16>>,
        env_hashes: EnvHashes,
    ) -> Self {
        Self {
            overlays,
            overlay_hashes,
            base,
            env_hashes,
            // Lazy: see `Self::new`.
            overlay_set_fingerprint: std::sync::OnceLock::new(),
        }
    }

    /// Borrow the base host. Reserved for impls that need to
    /// reach the host directly; resolver-tier code should not use
    /// this.
    #[allow(dead_code)]
    pub fn host(&self) -> &VerterHost {
        &self.base
    }

    /// Whether the view has an overlay for the requested canonical.
    #[allow(dead_code)]
    pub fn has_overlay(&self, canonical: &str) -> bool {
        self.overlays.contains_key(canonical)
    }
}

impl SessionView for OverlaidView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if let Some(overlay_source) = self.overlays.get(canonical) {
            return Some(Arc::clone(overlay_source));
        }
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // View-authoritative current content hash. An overlay-covered
        // canonical resolves to the overlay source's hash; an unmasked
        // canonical falls through to the base host's
        // scheduler-authoritative current hash — never a permissive
        // `FileArtifactStore` scan (a stale pre-edit artifact's hash
        // would disagree with `source()`).
        if let Some(hash) = self.overlay_hashes.get(canonical) {
            return Some(*hash);
        }
        self.base.authoritative_current_content_hash(canonical)
    }

    fn overlay_content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // Explicit overlay hash only — no base fallthrough. `OverlaidView`
        // has no tombstone set, so a present overlay hash is authoritative.
        self.overlay_hashes.get(canonical).copied()
    }

    fn overlay_artifact_discriminator(&self, canonical: &str) -> Option<Hash16> {
        // `Some` exactly when an explicit overlay covers `canonical`,
        // matching `overlay_content_hash_for`. The value namespaces
        // this view's overlay candidates away from the base artifact
        // (and from other sessions) in the `parse_env_hash` dimension.
        self.overlay_hashes
            .get(canonical)
            .map(|_| overlay_artifact_discriminator_from_fingerprint(self.fingerprint()))
    }

    fn project_identity(&self) -> ProjectIdentity {
        // View-level (workspace-default) project identity. Per-canonical
        // resolution callers use `VerterHost::host_view_project_identity_for`.
        self.base.host_view_project_identity()
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        // The resolved-import facts cache keys on `content_hash`,
        // which is overlay-aware: an overlay that differs from the
        // base source yields a distinct cache slot. The lookup
        // reads the overlay's content hash when an overlay covers
        // the canonical and falls through to the base host's
        // strict scheduler-authoritative current hash otherwise.
        // `current_content_hash_from_scheduler` matches the producer
        // (`admit_resolved_import_facts_for_owner`), which admits
        // keyed from the same scheduler `parse.whole_hash`.
        let content_hash = if let Some(hash) = self.overlay_hashes.get(canonical) {
            *hash
        } else {
            current_content_hash_from_scheduler(self.base.as_ref(), canonical)?
        };
        // `known_miss_generation`: read the owner's
        // sidecar so the lookup composes the same tag value as the
        // producer; lets a later route snapshot that re-resolves a
        // previously-missing specifier reach a fresh cache slot
        // instead of being pinned by an earlier first-writer-wins
        // negative entry.
        let known_miss_generation =
            known_miss_generation_tag_for_owner(self.base.as_ref(), canonical);
        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };
        self.base
            .project_type_store()
            .resolved_import_facts()
            .get(&key)
    }

    fn fingerprint(&self) -> u64 {
        // Lazily memoized (see `Self::overlay_set_fingerprint`): computed
        // on the first read through the single shared algorithm, then
        // returned directly with no recompute. The overlay map is
        // immutable for this view's lifetime, so the first-read value is
        // correct for the whole lifetime. `OverlaidView` carries no
        // tombstone set, so `None` is passed.
        *self.overlay_set_fingerprint.get_or_init(|| {
            overlay_set_fingerprint(&self.overlay_hashes, None, self.base.provenance())
        })
    }

    fn overlay_canonicals(&self) -> Vec<String> {
        self.overlays.keys().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// HostViewRef — borrow-shaped HostView for short-lived call chains.
// ---------------------------------------------------------------------------

/// Borrow-shaped [`HostView`] variant — holds `&'a VerterHost`
/// instead of `Arc<VerterHost>`.
///
/// `HostView` carries `Arc<VerterHost>` so it can be cloned across
/// threads. That ownership shape is wrong for short-lived call paths
/// that already have a `&VerterHost` in scope (and would otherwise
/// need a self-referential `Arc<VerterHost>` cycle). `HostViewRef<'a>`
/// borrows the host instead and is constructed inline at every base-
/// host call site that needs a base-view `&dyn SessionView` without
/// owning an `Arc<VerterHost>`. The impl mirrors `HostView`; once
/// real env-hash + project-identity plumbing lands, both shapes
/// consume the workspace config the same way.
#[derive(Clone, Copy)]
pub struct HostViewRef<'a> {
    base: &'a VerterHost,
    env_hashes: EnvHashes,
}

impl<'a> HostViewRef<'a> {
    /// Construct a view that borrows `base` and reports the
    /// workspace-default env hashes computed from the host's workspace.
    pub fn new(base: &'a VerterHost) -> Self {
        let env_hashes = base.host_view_env_hashes();
        Self { base, env_hashes }
    }

    /// Construct a view with explicit env hashes.
    ///
    /// Used by producers that carry real env hashes from the
    /// workspace config.
    #[allow(dead_code)]
    pub fn with_env_hashes(base: &'a VerterHost, env_hashes: EnvHashes) -> Self {
        Self { base, env_hashes }
    }
}

impl SessionView for HostViewRef<'_> {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // View-authoritative current content hash — scheduler
        // authority, no permissive `FileArtifactStore` scan (see
        // `HostView::content_hash_for` for the same rationale).
        self.base.authoritative_current_content_hash(canonical)
    }

    fn project_identity(&self) -> ProjectIdentity {
        // View-level (workspace-default) project identity. Per-canonical
        // resolution callers use `VerterHost::host_view_project_identity_for`.
        self.base.host_view_project_identity()
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        resolved_import_facts_via_host(self.base, canonical, &self.env_hashes)
    }
}

// ---------------------------------------------------------------------------
// OverlaidViewRef — borrow-based OverlaidView
// ---------------------------------------------------------------------------

/// Borrow-shaped [`OverlaidView`] variant — holds `&'a VerterHost` plus
/// borrowed overlay maps instead of `Arc<VerterHost>` + `Arc<FxHashMap>`.
///
/// Use this from `MetaSession` query paths that already hold
/// `&self.project.host` (a `&VerterHost`) and want to thread a
/// session-local overlay map into the consumer path without copying
/// the host into an `Arc`. Reads check `overlays` first; absent
/// canonicals fall through to the base host (R17).
///
/// **Concurrency.** Construction borrows the overlay maps; the
/// reference is `Send + Sync` as long as the borrowed `FxHashMap`s are
/// reachable. Sessions construct this on the stack inside a single
/// query call and drop it before the call returns, so the borrow
/// stays scoped to the query.
pub struct OverlaidViewRef<'a> {
    /// Per-canonical overlay sources (overlay → owned `Arc<str>` body).
    overlays: &'a rustc_hash::FxHashMap<String, Arc<str>>,
    /// Per-canonical overlay content hashes (precomputed by the
    /// session at overlay-installation time).
    overlay_hashes: &'a rustc_hash::FxHashMap<String, Hash16>,
    /// Set of canonicals the session has explicitly tombstoned via
    /// `MetaSession::delete`. Present in the set → the view reports
    /// `source = None` and `content_hash_for = None`, irrespective of
    /// what the base host says.
    overlay_tombstones: &'a std::collections::HashSet<String>,
    base: &'a VerterHost,
    env_hashes: EnvHashes,
    /// Overlay-set fingerprint, computed LAZILY on the first
    /// [`SessionView::fingerprint`] read via [`overlay_set_fingerprint`]
    /// over the immutable `overlay_hashes` map + `overlay_tombstones`
    /// set, then memoized for the view's lifetime — no per-call collect +
    /// sort + hash, and ZERO computations for a view that never reads the
    /// fingerprint. The analysis-only path (`get_analysis` →
    /// [`crate::meta::MetaSession::with_overlay_view`] →
    /// `get_analysis_via_view`) reads only tombstone / source / overlay
    /// content hash and never calls `fingerprint()`, so it pays nothing;
    /// the cache-key paths (component-meta / payload) read it once on
    /// first use and reuse the memo. The view holds the overlay maps as
    /// immutable borrows for its (single query-scoped) lifetime, so the
    /// first-read value is correct for the whole lifetime. A session
    /// mutation builds a NEW view over NEW maps, so the memo can never go
    /// stale — there is no in-place mutation path. The cell is `OnceLock`
    /// (NOT `OnceCell`) because the view is shared across rayon worker
    /// threads during a batch and so MUST be `Sync`; concurrent
    /// first-readers are handled by `OnceLock::get_or_init` (one computes,
    /// the rest block then read the same value).
    overlay_set_fingerprint: std::sync::OnceLock<u64>,
}

impl<'a> OverlaidViewRef<'a> {
    /// Construct a borrow-based overlaid view.
    ///
    /// `overlays` and `overlay_hashes` MUST be aligned: every key in
    /// `overlays` MUST have a precomputed entry in `overlay_hashes`.
    /// `overlay_tombstones` holds the set of canonicals the session
    /// has explicitly deleted (overlay-Delete entries).
    pub fn new(
        base: &'a VerterHost,
        overlays: &'a rustc_hash::FxHashMap<String, Arc<str>>,
        overlay_hashes: &'a rustc_hash::FxHashMap<String, Hash16>,
        overlay_tombstones: &'a std::collections::HashSet<String>,
    ) -> Self {
        let env_hashes = base.host_view_env_hashes();
        Self {
            overlays,
            overlay_hashes,
            overlay_tombstones,
            base,
            env_hashes,
            // Lazy: computed on the first `fingerprint()` read through the
            // single shared algorithm, then memoized. An analysis-only
            // request never reads it, so it pays nothing; the cache-key
            // paths read it once on first use. The O(N²) per-query
            // recompute the batch path paid is gone either way.
            overlay_set_fingerprint: std::sync::OnceLock::new(),
        }
    }

    /// Whether the view tombstones the given canonical (overlay-Delete).
    pub fn is_tombstoned(&self, canonical: &str) -> bool {
        self.overlay_tombstones.contains(canonical)
    }

    /// Whether the view has an overlay-Upsert for the given canonical.
    #[allow(dead_code)]
    pub fn has_overlay(&self, canonical: &str) -> bool {
        self.overlays.contains_key(canonical)
    }

    /// Borrow the base host. Reserved for internal consumer paths
    /// that need to reach the host directly after consulting the
    /// view.
    pub fn host(&self) -> &VerterHost {
        self.base
    }
}

impl SessionView for OverlaidViewRef<'_> {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        if let Some(overlay_source) = self.overlays.get(canonical) {
            return Some(Arc::clone(overlay_source));
        }
        self.base.get_source(canonical)
    }

    fn content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // View-authoritative current content hash. A tombstoned
        // canonical has no current content (the session deleted it).
        // An overlay-covered canonical resolves to the overlay
        // source's hash; an unmasked canonical falls through to the
        // base host's scheduler-authoritative current hash — never a
        // permissive `FileArtifactStore` scan.
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        if let Some(hash) = self.overlay_hashes.get(canonical) {
            return Some(*hash);
        }
        self.base.authoritative_current_content_hash(canonical)
    }

    fn overlay_content_hash_for(&self, canonical: &str) -> Option<Hash16> {
        // A tombstoned canonical has no overlay content even if a
        // stale overlay hash lingered in the map.
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        self.overlay_hashes.get(canonical).copied()
    }

    fn overlay_artifact_discriminator(&self, canonical: &str) -> Option<Hash16> {
        // `Some` exactly when an explicit overlay covers `canonical`
        // (and it is not tombstoned), matching `overlay_content_hash_for`.
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        self.overlay_hashes
            .get(canonical)
            .map(|_| overlay_artifact_discriminator_from_fingerprint(self.fingerprint()))
    }

    fn is_tombstoned(&self, canonical: &str) -> bool {
        self.overlay_tombstones.contains(canonical)
    }

    fn project_identity(&self) -> ProjectIdentity {
        // View-level (workspace-default) project identity. Per-canonical
        // resolution callers use `VerterHost::host_view_project_identity_for`.
        self.base.host_view_project_identity()
    }

    fn env_hashes(&self) -> &EnvHashes {
        &self.env_hashes
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        // Tombstoned canonicals report no resolved-import facts so
        // consumers cannot read a stale resolution past a delete.
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        // Overlay-bearing read: when an overlay covers the
        // canonical use its precomputed hash; otherwise fall
        // through to the base host's strict scheduler-authoritative
        // current hash. `current_content_hash_from_scheduler`
        // matches the producer (`admit_resolved_import_facts_for_owner`),
        // which admits keyed from the same scheduler `parse.whole_hash`.
        let content_hash = if let Some(hash) = self.overlay_hashes.get(canonical) {
            *hash
        } else {
            current_content_hash_from_scheduler(self.base, canonical)?
        };
        // `known_miss_generation`: read the owner's
        // sidecar to match the producer's key.
        let known_miss_generation = known_miss_generation_tag_for_owner(self.base, canonical);
        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };
        self.base
            .project_type_store()
            .resolved_import_facts()
            .get(&key)
    }

    fn fingerprint(&self) -> u64 {
        // Lazily memoized (see `Self::overlay_set_fingerprint`): computed
        // on the first read through the single shared algorithm, then
        // returned directly with no recompute. The overlay maps are
        // immutable for this view's lifetime, so the first-read value is
        // correct for the whole lifetime. On the hot batch path every
        // per-job `cache_key` / warm-probe / store after the first reads
        // the memo O(1); an analysis-only view that never calls this pays
        // nothing.
        *self.overlay_set_fingerprint.get_or_init(|| {
            overlay_set_fingerprint(
                self.overlay_hashes,
                Some(self.overlay_tombstones),
                self.base.provenance(),
            )
        })
    }

    fn overlay_canonicals(&self) -> Vec<String> {
        self.overlays.keys().cloned().collect()
    }

    fn tombstoned_canonicals(&self) -> Vec<String> {
        self.overlay_tombstones.iter().cloned().collect()
    }
}

/// Derive the [`crate::file_artifact_store::FileArtifactKey::overlay_scoped`]
/// `parse_env_hash` discriminator from a view's overlay-set
/// [`SessionView::fingerprint`].
///
/// The discriminator namespaces a session's overlay `IndexedReady`
/// candidates away from the base artifact (always
/// `parse_env_hash = BASE_PARSE_ENV_HASH`, i.e. `[0u8; 16]`) and away
/// from other sessions (distinct overlay-set fingerprints → distinct
/// discriminators). It MUST be non-zero so it can never alias the
/// base key.
///
/// Layout: a fixed 8-byte namespace tag in the high half guarantees the
/// result is non-zero even for the (precondition-excluded) `fingerprint
/// == 0` case, and keeps the value clear of any real env hash; the
/// fingerprint occupies the low 8 bytes so distinct overlay sets map to
/// distinct discriminators.
/// The session-wide overlay artifact discriminator: the `Hash16` every overlay
/// artifact in this session carries in its `FileArtifactKey::parse_env_hash`
/// dimension. The overlay-aware augmentation-index scan
/// ([`crate::file_artifact_store::FileArtifactStore::collect_augmenter_candidates`])
/// matches non-base artifacts against this value to union the session's own
/// overlay augmenters with base while excluding other sessions' overlays.
///
/// Returns `None` when the view carries no overlays (`fingerprint() == 0`): no
/// overlay artifacts exist, so the scan stays base-only.
#[must_use]
pub fn session_overlay_discriminator(view: &dyn SessionView) -> Option<Hash16> {
    let fingerprint = view.fingerprint();
    if fingerprint == 0 {
        return None;
    }
    Some(overlay_artifact_discriminator_from_fingerprint(fingerprint))
}

/// Derive the overlay-aware augmentation-index population identity and the
/// matching overlay artifact discriminator for an optional active session view.
///
/// This is the SINGLE derivation every augmentation-index producer routes
/// through, including the semantic body stitch in
/// [`crate::project_semantic_dispatch::build::ProjectSemanticDispatch`], so
/// producers cannot disagree on what
/// [`crate::file_artifact_store::AugmentationPopulation::Session`] means.
///
/// The `Session` discriminant is keyed by the overlay-set
/// [`SessionView::fingerprint`] — NOT a raw session id — because the fingerprint
/// captures *which* overlays are installed: two sessions with identical overlay
/// sets share an augmenter set, and a session whose overlays change gets a fresh
/// key. A bare session id would let a session presenting a base-only augmenter
/// set be cached as session-correct (the "base presented as session" hazard).
///
/// A view with no overlays (`fingerprint() == 0`) collapses to
/// [`AugmentationPopulation::Base`] with a `None` discriminator — the scan stays
/// base-only, identical to no session view at all.
#[must_use]
pub fn augmentation_population_for_view(
    view: Option<&dyn SessionView>,
) -> (
    crate::file_artifact_store::AugmentationPopulation,
    Option<Hash16>,
) {
    use crate::file_artifact_store::AugmentationPopulation;
    match view {
        Some(sv) if sv.fingerprint() != 0 => (
            AugmentationPopulation::Session(sv.fingerprint()),
            session_overlay_discriminator(sv),
        ),
        _ => (AugmentationPopulation::Base, None),
    }
}

/// The overlay artifact discriminator (`FileArtifactKey::parse_env_hash`
/// dimension) for a given overlay-set [`SessionView::fingerprint`]. This is the
/// canonical derivation [`session_overlay_discriminator`] applies to a non-zero
/// fingerprint; it is exposed so producers and tests share ONE source of truth
/// for the overlay key bytes rather than hand-rolling them.
#[must_use]
pub fn overlay_artifact_discriminator_for_fingerprint(fingerprint: u64) -> Hash16 {
    overlay_artifact_discriminator_from_fingerprint(fingerprint)
}

fn overlay_artifact_discriminator_from_fingerprint(fingerprint: u64) -> Hash16 {
    // Arbitrary fixed namespace tag — distinguishes an overlay-scoped
    // key from any zeroed / real `parse_env_hash` value.
    const OVERLAY_DISCRIMINATOR_TAG: [u8; 8] = *b"vovl-art";
    let fp = fingerprint.to_le_bytes();
    [
        OVERLAY_DISCRIMINATOR_TAG[0],
        OVERLAY_DISCRIMINATOR_TAG[1],
        OVERLAY_DISCRIMINATOR_TAG[2],
        OVERLAY_DISCRIMINATOR_TAG[3],
        OVERLAY_DISCRIMINATOR_TAG[4],
        OVERLAY_DISCRIMINATOR_TAG[5],
        OVERLAY_DISCRIMINATOR_TAG[6],
        OVERLAY_DISCRIMINATOR_TAG[7],
        fp[0],
        fp[1],
        fp[2],
        fp[3],
        fp[4],
        fp[5],
        fp[6],
        fp[7],
    ]
}

/// Hash the `(canonical, content_hash)` pairs of an overlay map (plus
/// optional tombstone set) into a single `u64` fingerprint.
///
/// Empty overlay maps with no tombstones return `0` so a tombstone-
/// free, overlay-free view is indistinguishable from a base view at
/// the fingerprint surface. Non-empty maps produce a value derived
/// from the sorted overlay entries, so two views with the same
/// overlay set return the same fingerprint regardless of insertion
/// order.
///
/// This fold is the [`SessionView::fingerprint`] surface identity — the
/// `u64` used for cache-key derivation and the augmentation-population
/// identity. It is one of TWO overlay-set folds in this crate: the other
/// is the validation-token overlay identity
/// (`OverlayIdentity::overlay_fingerprint`, built in
/// `resolver_store.rs::with_session_overlay`), a `Hash16` fold with
/// per-entry tombstone/upsert domain markers that discriminates
/// validation-token (singleflight-lane) identities. The two serve
/// different surfaces with different layouts and output widths; they are
/// deliberately NOT unified — changing either fold changes that surface's
/// identity values. Overlay views
/// ([`OverlaidView`], [`OverlaidViewRef`]) memoize the result LAZILY in a
/// [`std::sync::OnceLock`]: the first [`SessionView::fingerprint`] read
/// computes it through this function, and every later read returns the
/// stored value with no recompute. A view that never reads its
/// fingerprint (e.g. an analysis-only request) never calls this function
/// at all. The function is pure over its inputs (the overlay map and the
/// tombstone set) and the views hold those maps immutably for their
/// lifetime, so the first-read value stays correct for the view's whole
/// lifetime. The cost is bounded by the overlay-set cardinality — only
/// canonicals the session has upserted or tombstoned participate — not by
/// workspace size.
///
/// `provenance` counts each FULL computation (the collect + sort + hash
/// body, NOT the empty short-circuit) on the owning host via
/// [`crate::types::MetaProvenance::overlay_set_fingerprint_full_computations`],
/// so both the per-batch O(1) memoization AND the zero-cost analysis-only
/// path are mechanically observable.
fn overlay_set_fingerprint(
    overlay_hashes: &FxHashMap<String, Hash16>,
    tombstones: Option<&std::collections::HashSet<String>>,
    provenance: &crate::types::MetaProvenance,
) -> u64 {
    if overlay_hashes.is_empty() && tombstones.is_none_or(std::collections::HashSet::is_empty) {
        return 0;
    }
    provenance
        .overlay_set_fingerprint_full_computations
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    use std::hash::{Hash, Hasher};
    let mut entries: Vec<(&str, [u8; 16])> = overlay_hashes
        .iter()
        .map(|(canonical, hash)| (canonical.as_str(), *hash))
        .collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    let mut hasher = rustc_hash::FxHasher::default();
    for (canonical, hash) in &entries {
        canonical.hash(&mut hasher);
        hash.hash(&mut hasher);
    }
    if let Some(set) = tombstones {
        let mut tombs: Vec<&str> = set.iter().map(String::as_str).collect();
        tombs.sort_unstable();
        // Domain separator so an overlay whose canonical equals a
        // tombstoned canonical in some other view does not collide.
        b"|tombstones|".hash(&mut hasher);
        for canonical in &tombs {
            canonical.hash(&mut hasher);
        }
    }
    let raw = hasher.finish();
    // Reserve `0` for "no overlays / no tombstones".
    if raw == 0 {
        1
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileLanguage, UpsertRequest};
    use crate::{CompileErrorPolicy, HostConfig};

    fn fresh_host() -> Arc<VerterHost> {
        Arc::new(VerterHost::new_standalone(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        }))
    }

    fn upsert(host: &VerterHost, canonical: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert succeeds");
        // Materialise IndexedReady so `FileArtifactStore` has an entry
        // for this canonical — the file-artifact store is populated
        // lazily on first demand, not synchronously from `upsert`.
        let _ = host.ensure_indexed_ready(canonical);
    }

    #[test]
    fn host_view_passes_through_source() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let view = HostView::new(Arc::clone(&host));
        let observed = view.source("/x.ts");
        assert!(observed.is_some());
        assert_eq!(observed.as_deref(), Some("export const a = 1;"));
        assert!(view.source("/missing.ts").is_none());
    }

    #[test]
    fn host_view_reports_content_hash_after_upsert() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let view = HostView::new(Arc::clone(&host));
        let hash = view.content_hash_for("/x.ts");
        assert!(
            hash.is_some(),
            "HostView.content_hash_for must report a hash for an ingested canonical"
        );
    }

    #[test]
    fn overlaid_view_overlay_source_wins_over_base() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/x.ts".to_string(), Arc::from("export const a = 999;"));
        let view = OverlaidView::new(Arc::clone(&host), overlays);

        let observed = view.source("/x.ts");
        assert_eq!(observed.as_deref(), Some("export const a = 999;"));
    }

    #[test]
    fn overlaid_view_falls_through_to_base_for_unmasked_canonical() {
        let host = fresh_host();
        upsert(&host, "/base.ts", "export const a = 1;");

        let overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        let view = OverlaidView::new(Arc::clone(&host), overlays);

        assert_eq!(
            view.source("/base.ts").as_deref(),
            Some("export const a = 1;")
        );
    }

    #[test]
    fn overlaid_view_content_hash_diverges_from_base_under_overlay() {
        let host = fresh_host();
        upsert(&host, "/x.ts", "export const a = 1;");

        let host_view = HostView::new(Arc::clone(&host));
        let base_hash = host_view.content_hash_for("/x.ts");
        assert!(base_hash.is_some());

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(
            "/x.ts".to_string(),
            Arc::from("export const a = 'overlay';"),
        );
        let overlay_view = OverlaidView::new(Arc::clone(&host), overlays);
        let overlay_hash = overlay_view.content_hash_for("/x.ts");
        assert!(overlay_hash.is_some());
        assert_ne!(
            base_hash, overlay_hash,
            "OverlaidView with a different source must report a different content hash than the base"
        );
    }

    #[test]
    fn overlaid_view_byte_identical_overlay_matches_base_hash() {
        // R17 byte-identical guarantee — surfacing the same source
        // via the overlay must produce the same content hash as
        // the base.
        let host = fresh_host();
        let body = "export const a = 1;";
        upsert(&host, "/x.ts", body);

        let host_view = HostView::new(Arc::clone(&host));
        let base_hash = host_view.content_hash_for("/x.ts").expect("base hash");

        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/x.ts".to_string(), Arc::from(body));
        let overlay_view = OverlaidView::new(Arc::clone(&host), overlays);
        let overlay_hash = overlay_view
            .content_hash_for("/x.ts")
            .expect("overlay hash");

        assert_eq!(
            base_hash, overlay_hash,
            "byte-identical overlay must collapse to the base content hash"
        );
    }

    #[test]
    fn session_view_trait_is_object_safe() {
        // Compile-time check: `&dyn SessionView` must work for
        // both impls so resolver-tier code can take a single
        // trait object reference.
        fn assert_dyn(view: &dyn SessionView) -> bool {
            view.env_hashes().parse_env_hash != [0xffu8; 16]
        }

        let host = fresh_host();
        let host_view: Box<dyn SessionView> = Box::new(HostView::new(Arc::clone(&host)));
        let overlaid: Box<dyn SessionView> =
            Box::new(OverlaidView::new(Arc::clone(&host), FxHashMap::default()));

        // Env-hashes default to zero — the assertion exercises the
        // dyn trait call path; the predicate is non-trivial
        // (compares against an explicit sentinel value).
        assert!(assert_dyn(host_view.as_ref()));
        assert!(assert_dyn(overlaid.as_ref()));
    }

    /// The memoized `view.fingerprint()` equals a fresh
    /// `overlay_set_fingerprint(...)` recomputation over the SAME overlay
    /// maps — proving the memo carries the value the single shared
    /// algorithm produces (no second algorithm, no divergence). Holds for
    /// both the Arc-based `OverlaidView` and the borrow-based
    /// `OverlaidViewRef` (the hot path, with a tombstone set).
    #[test]
    fn memoized_fingerprint_matches_fresh_overlay_set_fingerprint() {
        let host = fresh_host();
        upsert(&host, "/a.ts", "export const a = 1;");
        upsert(&host, "/b.ts", "export const b = 2;");

        // Arc-based OverlaidView.
        let mut overlays_arc: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays_arc.insert("/a.ts".to_string(), Arc::from("export const a = 9;"));
        overlays_arc.insert("/b.ts".to_string(), Arc::from("export const b = 8;"));
        let arc_view = OverlaidView::new(Arc::clone(&host), overlays_arc);
        let fresh_arc =
            overlay_set_fingerprint(arc_view.overlay_hashes.as_ref(), None, host.provenance());
        assert_eq!(
            arc_view.fingerprint(),
            fresh_arc,
            "OverlaidView memoized fingerprint must equal a fresh recomputation \
             over the same overlay-hash map",
        );

        // Borrow-based OverlaidViewRef WITH a tombstone (the hot path).
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert("/a.ts".to_string(), Arc::from("export const a = 9;"));
        let mut overlay_hashes: FxHashMap<String, Hash16> = FxHashMap::default();
        overlay_hashes.insert(
            "/a.ts".to_string(),
            crate::hash::hash_16(b"export const a = 9;"),
        );
        let mut tombstones: std::collections::HashSet<String> = std::collections::HashSet::new();
        tombstones.insert("/b.ts".to_string());
        let ref_view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
        let fresh_ref =
            overlay_set_fingerprint(&overlay_hashes, Some(&tombstones), host.provenance());
        assert_eq!(
            ref_view.fingerprint(),
            fresh_ref,
            "OverlaidViewRef memoized fingerprint must equal a fresh \
             recomputation over the same overlay-hash map + tombstone set",
        );
        assert_ne!(
            ref_view.fingerprint(),
            0,
            "a non-empty overlay set must not collapse to the base sentinel 0",
        );
    }

    /// The memoized fingerprint NEVER goes stale: a different overlay set
    /// yields a different fingerprint, the SAME overlay set yields the
    /// SAME fingerprint, and a tombstone is fingerprint-significant. Each
    /// view memoizes the fingerprint of the EXACT immutable maps it was
    /// constructed from, so a session mutation (which builds a fresh view
    /// over fresh maps) is reflected — a stale value can never be served.
    #[test]
    fn memoized_fingerprint_tracks_overlay_set_changes() {
        let host = fresh_host();
        upsert(&host, "/a.ts", "export const a = 1;");
        upsert(&host, "/b.ts", "export const b = 2;");

        let empty_overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        let empty_hashes: FxHashMap<String, Hash16> = FxHashMap::default();
        let no_tombstones: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Empty overlay set → base sentinel 0 (no behavior change for base
        // sessions).
        let base_view = OverlaidViewRef::new(&host, &empty_overlays, &empty_hashes, &no_tombstones);
        assert_eq!(
            base_view.fingerprint(),
            0,
            "empty overlay set + no tombstones must fingerprint to 0",
        );

        // Overlay set {a}.
        let mut overlays_a: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays_a.insert("/a.ts".to_string(), Arc::from("export const a = 9;"));
        let mut hashes_a: FxHashMap<String, Hash16> = FxHashMap::default();
        hashes_a.insert(
            "/a.ts".to_string(),
            crate::hash::hash_16(b"export const a = 9;"),
        );
        let view_a = OverlaidViewRef::new(&host, &overlays_a, &hashes_a, &no_tombstones);
        let fp_a = view_a.fingerprint();

        // A SECOND view over the IDENTICAL overlay set → IDENTICAL
        // fingerprint (order-independent, stable). Distinct view value,
        // distinct memo slot, same algorithm input → same output.
        let view_a2 = OverlaidViewRef::new(&host, &overlays_a, &hashes_a, &no_tombstones);
        assert_eq!(
            fp_a,
            view_a2.fingerprint(),
            "two views over the same overlay set must produce the same \
             fingerprint (the memo is per-view but the value is a pure \
             function of the overlay set)",
        );

        // Overlay set {a, b}: adding a second overlay entry MUST change
        // the fingerprint (a stale memo would keep reporting fp_a).
        let mut overlays_ab = overlays_a.clone();
        overlays_ab.insert("/b.ts".to_string(), Arc::from("export const b = 8;"));
        let mut hashes_ab = hashes_a.clone();
        hashes_ab.insert(
            "/b.ts".to_string(),
            crate::hash::hash_16(b"export const b = 8;"),
        );
        let view_ab = OverlaidViewRef::new(&host, &overlays_ab, &hashes_ab, &no_tombstones);
        let fp_ab = view_ab.fingerprint();
        assert_ne!(
            fp_a, fp_ab,
            "adding a second overlay entry must change the fingerprint — a \
             stale memo serving the old set's value is a wrong-cache-hit risk",
        );

        // Tombstoning a canonical (overlay set {a} + tombstone {b}) MUST
        // differ from both {a} and {a,b} — the tombstone domain separator
        // is fingerprint-significant.
        let mut tombstone_b: std::collections::HashSet<String> = std::collections::HashSet::new();
        tombstone_b.insert("/b.ts".to_string());
        let view_a_tomb_b = OverlaidViewRef::new(&host, &overlays_a, &hashes_a, &tombstone_b);
        let fp_a_tomb_b = view_a_tomb_b.fingerprint();
        assert_ne!(
            fp_a, fp_a_tomb_b,
            "adding a tombstone must change the fingerprint vs the same \
             overlay set with no tombstone",
        );
        assert_ne!(
            fp_ab, fp_a_tomb_b,
            "an overlay {{a}} + tombstone {{b}} must not collide with an \
             overlay {{a, b}} (the tombstone domain separator distinguishes \
             them)",
        );

        // Changing an overlay's CONTENT hash (same canonical, different
        // body) MUST change the fingerprint.
        let mut overlays_a_changed: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays_a_changed.insert("/a.ts".to_string(), Arc::from("export const a = 777;"));
        let mut hashes_a_changed: FxHashMap<String, Hash16> = FxHashMap::default();
        hashes_a_changed.insert(
            "/a.ts".to_string(),
            crate::hash::hash_16(b"export const a = 777;"),
        );
        let view_a_changed = OverlaidViewRef::new(
            &host,
            &overlays_a_changed,
            &hashes_a_changed,
            &no_tombstones,
        );
        assert_ne!(
            fp_a,
            view_a_changed.fingerprint(),
            "changing an overlay's content hash must change the fingerprint",
        );
    }

    #[test]
    fn host_view_and_overlaid_view_are_send_sync() {
        // Compile-time check (negative-by-construction): a `Send +
        // Sync` impl is required for trait-object use across the
        // resolver-tier thread pools.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HostView>();
        assert_send_sync::<OverlaidView>();

        // Reference the assertions from runtime so the test body
        // is non-empty — guards against the assertions silently
        // becoming dead code.
        let host = fresh_host();
        let view: Box<dyn SessionView + Send + Sync> = Box::new(HostView::new(host));
        let _ = view.project_identity();
    }
}
