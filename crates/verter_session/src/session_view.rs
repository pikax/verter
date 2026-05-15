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
//! onto this trait: `ResolverContext` exposes a `view()` accessor,
//! [`HostFenceValidator`](crate::host_manage::HostFenceValidator) is
//! view-aware, and the host's overlay-mutation machinery is gone
//! (R17). See `/type-cache-architecture` and `/host-session`
//! skills for the architectural rules (R17–R20).
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

    /// Content hash of `canonical` under this view, if known.
    ///
    /// For [`OverlaidView`] this returns the hash of the overlay
    /// source when an overlay covers the canonical; otherwise it
    /// returns the base host's content hash (or `None`).
    fn content_hash_for(&self, canonical: &str) -> Option<Hash16>;

    /// Return cached file artifacts (indexed-ready + facts +
    /// parsed-edges + parse_stable_hash + augmentations) for a
    /// canonical id, if a content-matching artifact bundle is
    /// already in the file-artifact store.
    ///
    /// Returns `None` if the artifacts have not been parsed yet
    /// under the relevant `(content_hash, parse_env_hash)` key.
    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>>;

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
    fn overlay_canonicals(&self) -> Vec<String> {
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
/// scheduler-cached `parse.whole_hash` so the producer's admission
/// key and the view's lookup key reach the same `DashMap` slot
/// immediately after `upsert`, without waiting for the lazy
/// `IndexedReady` materialization through `ensure_indexed_ready`.
/// When the scheduler has no source snapshot for the canonical (the
/// file was never `upsert`-ed or has been closed), falls back to the
/// file-artifact store's `content_hash_for_canonical` so legacy
/// callers that materialize through `ensure_indexed_ready` first
/// still resolve.
///
/// Returns `None` when neither the scheduler nor the artifact store
/// has a content hash for `canonical`, or when the cache has not been
/// populated for the resolved quintuple.
fn resolved_import_facts_via_host(
    base: &VerterHost,
    canonical: &str,
    env_hashes: &EnvHashes,
) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
    let content_hash = content_hash_from_scheduler_or_artifacts(base, canonical)?;
    let key = crate::resolved_import_facts::ResolvedImportFactsKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash: env_hashes.parse_env_hash,
        resolve_env_hash: env_hashes.resolve_env_hash,
        resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
    };
    base.project_type_store().resolved_import_facts().get(&key)
}

/// Resolve `canonical`'s content hash from the most authoritative
/// source available.
///
/// Prefers the scheduler-cached `HostSourceData.parse.whole_hash`
/// (the sole parse authority — available immediately post-`upsert`)
/// and falls back to the file-artifact store's
/// `content_hash_for_canonical` (the lazy view of the same value
/// once `IndexedReady` has been materialized via
/// `ensure_indexed_ready`). Both sources record the same hash for a
/// given canonical's current bytes; the scheduler is just the
/// earlier-available view.
///
/// Used by `resolved_import_facts_via_host` to match the producer
/// (`admit_resolved_import_facts_for_owner`) on cache-key
/// `content_hash` composition.
fn content_hash_from_scheduler_or_artifacts(
    base: &VerterHost,
    canonical: &str,
) -> Option<verter_semantic::analysis::Hash16> {
    if let Some(snap) = base.scheduler().try_get_source(canonical) {
        if let Some(hd) = snap.downcast_data::<crate::host_executor::HostSourceData>() {
            return Some(hd.parse.whole_hash);
        }
    }
    base.project_type_store()
        .indexed()
        .content_hash_for_canonical(canonical)
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
        // Use the file-artifact store as the authoritative
        // content-hash source; falls back to None for canonicals
        // not yet ingested.
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        // Strict by `(canonical, content_hash)` — base view only
        // accepts the artifact bundle that matches the host's
        // current content hash. Returning `None` here forces
        // resolver-tier consumers to materialise rather than
        // observe a sibling overlay candidate published by a
        // concurrent session under a different content hash.
        let content_hash = self.content_hash_for(canonical)?;
        let key =
            crate::file_artifact_store::FileArtifactKey::legacy(Arc::from(canonical), content_hash);
        self.base.project_type_store().indexed().get_artifacts(&key)
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
/// through to the base host. Overlay artifacts are produced on
/// demand under the overlay's content hash when the artifact
/// store learns to key on the overlay content hash; until that
/// wiring lands the artifact-side path falls through to the
/// base host.
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
        if let Some(hash) = self.overlay_hashes.get(canonical) {
            return Some(*hash);
        }
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        // Strict by `(canonical, content_hash_for(canonical))`. An
        // overlay candidate is published under the overlay's
        // content hash by `materialize_overlay_indexed_ready`; the
        // base candidate lives under the base host's content hash.
        // Reading via the strict `(canonical, hash)` key prevents
        // an overlay-bearing view from observing the base
        // candidate (which would be the wrong artifact bundle for
        // the overlay's source).
        let content_hash = self.content_hash_for(canonical)?;
        let key =
            crate::file_artifact_store::FileArtifactKey::legacy(Arc::from(canonical), content_hash);
        self.base.project_type_store().indexed().get_artifacts(&key)
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
        // therefore reads the overlay's content hash when an
        // overlay covers the canonical and the base host's
        // otherwise.
        let content_hash = self.content_hash_for(canonical)?;
        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        };
        self.base
            .project_type_store()
            .resolved_import_facts()
            .get(&key)
    }

    fn fingerprint(&self) -> u64 {
        overlay_set_fingerprint(self.overlay_hashes.as_ref(), None)
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
/// threads. That ownership shape is wrong for the
/// [`ResolverContext::view`](crate::resolver_core::resolver_context::ResolverContext::view)
/// call path, which has a `&self: &VerterHost` already in scope and
/// would otherwise need a self-referential `Arc<VerterHost>` cycle.
/// `HostViewRef<'a>` borrows the host instead and is the value
/// `VerterHost`'s `view()` impl returns. The trait method exposes it
/// as `Box<dyn SessionView + '_>` so the dyn-compatibility of
/// `ResolverContext` is preserved.
///
/// The impl mirrors `HostView`; once real env-hash + project-identity
/// plumbing lands, both shapes consume the workspace config the same
/// way.
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
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        // Strict by `(canonical, content_hash)` — base view only
        // accepts the artifact bundle matching the host's current
        // content hash (see `HostView::parse_artifacts` for the
        // same rationale).
        let content_hash = self.content_hash_for(canonical)?;
        let key =
            crate::file_artifact_store::FileArtifactKey::legacy(Arc::from(canonical), content_hash);
        self.base.project_type_store().indexed().get_artifacts(&key)
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
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        if let Some(hash) = self.overlay_hashes.get(canonical) {
            return Some(*hash);
        }
        self.base
            .project_type_store()
            .indexed()
            .content_hash_for_canonical(canonical)
    }

    fn is_tombstoned(&self, canonical: &str) -> bool {
        self.overlay_tombstones.contains(canonical)
    }

    fn parse_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        // Strict by `(canonical, content_hash_for(canonical))`. The
        // overlay candidate is published into `FileArtifactStore`
        // by `materialize_overlay_indexed_ready` under the overlay's
        // content hash; this read goes through the same strict key
        // so the overlay-bearing view observes the overlay bundle
        // (never the base candidate). Tombstoned canonicals return
        // `None` regardless of what the base says.
        if self.overlay_tombstones.contains(canonical) {
            return None;
        }
        let content_hash = self.content_hash_for(canonical)?;
        let key =
            crate::file_artifact_store::FileArtifactKey::legacy(Arc::from(canonical), content_hash);
        self.base.project_type_store().indexed().get_artifacts(&key)
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
        let content_hash = self.content_hash_for(canonical)?;
        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
        };
        self.base
            .project_type_store()
            .resolved_import_facts()
            .get(&key)
    }

    fn fingerprint(&self) -> u64 {
        overlay_set_fingerprint(self.overlay_hashes, Some(self.overlay_tombstones))
    }

    fn overlay_canonicals(&self) -> Vec<String> {
        self.overlays.keys().cloned().collect()
    }
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
/// TODO(follow-up — fix-agent P1.3 / substrate-reviewer P1.3): the
/// fingerprint is recomputed on every `SessionView::fingerprint()`
/// call. For overlay-bearing views in hot LSP loops the O(N log N)
/// sort + hash runs once per cache-key construction. Cache the
/// fingerprint on `OverlaidView` / `OverlaidViewRef` construction
/// (one `OnceCell<u64>` or one pre-computed field) so subsequent
/// reads are O(1). Owned by the follow-up substrate-hygiene block.
fn overlay_set_fingerprint(
    overlay_hashes: &FxHashMap<String, Hash16>,
    tombstones: Option<&std::collections::HashSet<String>>,
) -> u64 {
    if overlay_hashes.is_empty() && tombstones.is_none_or(std::collections::HashSet::is_empty) {
        return 0;
    }
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
    use crate::types::{FileKind, UpsertRequest};
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
                file_kind: FileKind::NonSfc,
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
