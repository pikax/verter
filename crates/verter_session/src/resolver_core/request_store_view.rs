//! Per-request shadowing wrapper around the immutable
//! [`HostStoreView`].
//!
//! ## Why
//!
//! Block 6.B made [`crate::resolver_core::ReadSetSignature`].`facts` the
//! sole cache-validity rail. Fact validation requires a live
//! [`HostStoreView`]; that view is an immutable snapshot of the
//! workspace's per-canonical facts at request-entry time.
//!
//! `HostStoreView::from_host` does 5-7 full workspace sweeps and
//! allocates ~5 hashmaps + an artifact `Vec` per call. Block 6.c hoists
//! the build to once-per-request, then threads a borrow through the
//! pipeline. But `ensure_loaded` and `ensure_indexed_ready` deliberately
//! do not bump `store_view_epoch` on first-time additive loads — so a
//! request-entry view built BEFORE dependency discovery does not track
//! later-loaded self-root canonicals. Without the overlay below the
//! self-root validator (`validates_self_root_whole_hash`) rejects every
//! such freshly-loaded canonical forever inside the request, creating a
//! new first-cold regression.
//!
//! ## What this file owns
//!
//! - [`CanonicalCompletionOverlay`]: the request-scoped append-only
//!   side maps that record additive loads observed mid-request.
//!   Constructed once, shared across cooperative-admission lanes via
//!   `Arc`, dropped at request end.
//!
//! - [`RequestStoreView`]: a wrapper that owns the overlay and borrows
//!   the request-entry [`HostStoreView`]. Implements
//!   [`crate::resolver_core::StoreView`] with **shadowing-first**
//!   semantics — if the overlay has a canonical/fact key, the overlay
//!   value is authoritative and a mismatch is REJECTED (not retried
//!   against the base view). If the overlay is absent for a key, reads
//!   fall through to the base view.
//!
//! ## Identity / epoch contract
//!
//! The overlay does NOT participate in
//! [`crate::resolver_core::StoreView::compat_token`]: the wrapper
//! reports the base's compat token unchanged. Two concurrent requests
//! with the same base epoch must still coalesce on singleflight lanes,
//! and the additive loads the overlay records are a per-request
//! optimisation that does not change project-wide identity.
//!
//! [`CanonicalCompletionOverlay::complete_canonical`] is **epoch-
//! guarded** (codex refinement #5): if the host's
//! `current_store_view_epoch()` no longer matches the base view's
//! `mutation_epoch()` at the time of the completion call, the call
//! returns without writing to the overlay. The outer stable executor
//! then retries with a fresh base view, and the old overlay is dropped
//! along with the retried context.
//!
//! ## 6.B preservation
//!
//! - **Session-overlay validation**: the wrapper chains in front of an
//!   already session-rooted [`HostStoreView`] (constructed via
//!   `with_session_overlay` once at request entry). Completion runs
//!   ATOP that view; the overlay does not try to model session
//!   overlay/tombstone state.
//! - **`validated_at_generation`**: unaffected. The
//!   `ProjectGeneration` fact validator routes through the base
//!   view's project-generation snapshot; completion never alters it.
//! - **Family memo gating + FIFO prune**: unaffected. The overlay
//!   changes validation visibility for facts already observed; it
//!   does not change what `traced_facts`, `dispatch_dep_signature`,
//!   `canonical_ids()`, or FIFO prune register.

use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::file_artifact_store::FileFacts;
use crate::resolver_core::{
    DerivedFactKind, FactVersionRef, ParseFactRef, ResolveImportsFactRef, ResolverHash16,
    RouteSurfaceFactRef, StoreView, StoreViewCompatToken,
};
use crate::resolver_store::{HostStoreView, RouteSurfaceIndexShapeKey};
use crate::types::Hash16;

/// Per-request append-only side maps recording additive loads that the
/// request-entry [`HostStoreView`] does not track.
///
/// Codex refinement #3 specifies the required overlay shape:
/// - `whole_hashes`
/// - `derived_hashes`
/// - `file_facts`
/// - `resolved_import_facts_known_miss_tags`
/// - `route_surface_index_fingerprints`
///
/// `import_routes`, `resolved_import_facts` handle, and `route_db`
/// handle stay OUT — they are `Arc` clones of project-wide `DashMap`s
/// and are already up-to-date through host-side concurrent writers.
///
/// Reads are wait-free against concurrent writers within the request
/// because [`RwLock`] readers do not block each other, and the
/// overlay's writers are scoped to
/// [`CanonicalCompletionOverlay::complete_canonical`] (one short
/// critical section per first-cold canonical).
pub(crate) struct CanonicalCompletionOverlay {
    whole_hashes: RwLock<FxHashMap<String, Hash16>>,
    derived_hashes: RwLock<FxHashMap<(String, DerivedFactKind), Hash16>>,
    file_facts: RwLock<FxHashMap<String, Arc<FileFacts>>>,
    /// Known-miss generation tags keyed by canonical id (matches the
    /// base view's `resolved_import_facts_known_miss_tags` shape).
    resolved_import_facts_known_miss_tags: RwLock<FxHashMap<String, Hash16>>,
    /// Route-surface augmentation-index fingerprints keyed by the
    /// structural shape (matches the base view's
    /// `route_surface_index_fingerprints` shape).
    route_surface_index_fingerprints: RwLock<FxHashMap<RouteSurfaceIndexShapeKey, Hash16>>,
}

impl Default for CanonicalCompletionOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl CanonicalCompletionOverlay {
    /// Construct an empty overlay.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            whole_hashes: RwLock::new(FxHashMap::default()),
            derived_hashes: RwLock::new(FxHashMap::default()),
            file_facts: RwLock::new(FxHashMap::default()),
            resolved_import_facts_known_miss_tags: RwLock::new(FxHashMap::default()),
            route_surface_index_fingerprints: RwLock::new(FxHashMap::default()),
        }
    }

    /// Idempotently promote a freshly-loaded canonical's facts into the
    /// overlay.
    ///
    /// Called after a successful `ensure_loaded` / `ensure_indexed_ready`
    /// on a canonical the request-entry base view does not track.
    /// Walking the host's currently-published per-canonical state is
    /// cheap (one `FileArtifactStore` lookup + a few scheduler reads);
    /// re-running the call after a prior completion is a no-op when the
    /// state has not changed.
    ///
    /// **Epoch guard (codex refinement #5):** if the host's current
    /// `store_view_epoch` no longer matches `base.mutation_epoch()`, the
    /// call returns without writing to the overlay. The outer stable
    /// executor will retry the request with a fresh view; mutating an
    /// already-superseded overlay would risk steering validation toward
    /// stale data.
    pub(crate) fn complete_canonical(
        &self,
        host: &crate::VerterHost,
        base: &HostStoreView,
        canonical: &str,
    ) {
        if host.current_store_view_epoch() != base.mutation_epoch() {
            // The base view is already superseded. Skip the write; the
            // outer executor will retry against a fresh view.
            return;
        }

        // Resolve the canonical's currently-tracked whole hash via the
        // same authority chain `HostStoreView::build` consults: the
        // scheduler first, then the host's `effective_file_state`
        // fallback for canonicals that exist only in the artifact
        // store.
        let whole_hash = host
            .scheduler()
            .try_get_source(canonical)
            .map(|source| source.whole_hash)
            .or_else(|| {
                host.effective_file_state(canonical, None)
                    .map(|state| state.whole_hash)
            });

        let Some(whole_hash) = whole_hash else {
            // No tracked content for this canonical — nothing to
            // promote. A consumer that observed a fact against an
            // unloaded canonical will fail the base view's untracked
            // path (or accept the optimistic-accept rule), exactly as
            // it did before the overlay was introduced.
            return;
        };

        self.whole_hashes
            .write()
            .insert(canonical.to_owned(), whole_hash);

        // Known-miss generation tag — captured under the same authority
        // the base view's `build` consults.
        if let Some(entry) = host.derived_raw_cache().get(canonical) {
            let tag = crate::resolved_import_facts::compute_known_miss_generation_tag(
                &entry.import_routes_known_miss_recorded_at_generation,
            );
            self.resolved_import_facts_known_miss_tags
                .write()
                .insert(canonical.to_owned(), tag);
        }

        // Per-canonical `IndexedReady` snapshot — populates `file_facts`
        // and the `Route` / `ImportRoute` derived-hash entries.
        if let Some(file_artifacts) = host
            .project_type_store()
            .indexed()
            .get_artifacts_for_content(canonical, whole_hash)
        {
            self.file_facts
                .write()
                .insert(canonical.to_owned(), Arc::clone(&file_artifacts.facts));

            let indexed = &file_artifacts.indexed;
            if indexed.shallow_state.has_resolvable_surface() {
                self.derived_hashes.write().insert(
                    (canonical.to_owned(), DerivedFactKind::Route),
                    crate::resolver_store::hash_route_surface(&indexed.shallow_state),
                );
            }
            if let Some(hash) = host.generation_current_import_route_hash(canonical) {
                self.derived_hashes.write().insert(
                    (canonical.to_owned(), DerivedFactKind::ImportRoute),
                    hash,
                );
            }
        }
    }

    fn lookup_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.read().get(canonical_id).copied()
    }

    fn lookup_derived_hash(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<Hash16> {
        self.derived_hashes
            .read()
            .get(&(canonical_id.to_owned(), kind))
            .copied()
    }

    fn lookup_file_facts(&self, canonical_id: &str) -> Option<Arc<FileFacts>> {
        self.file_facts.read().get(canonical_id).cloned()
    }

    fn lookup_known_miss_tag(&self, canonical_id: &str) -> Option<Hash16> {
        self.resolved_import_facts_known_miss_tags
            .read()
            .get(canonical_id)
            .copied()
    }

    fn lookup_route_surface_index_fingerprint(
        &self,
        key: &RouteSurfaceIndexShapeKey,
    ) -> Option<Hash16> {
        self.route_surface_index_fingerprints
            .read()
            .get(key)
            .copied()
    }

    /// Test-only: peek the overlay's `whole_hashes` entry for a
    /// canonical id. The discriminating tests for the epoch guard
    /// inspect this to assert that `complete_canonical` is a no-op
    /// when `host.current_store_view_epoch() != base.mutation_epoch()`.
    #[cfg(test)]
    pub(crate) fn peek_whole_hash_for_tests(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.read().get(canonical_id).copied()
    }

    /// Whether the overlay tracks `canonical_id` (any per-canonical
    /// entry).
    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.whole_hashes.read().contains_key(canonical_id)
            || self.file_facts.read().contains_key(canonical_id)
    }
}

/// Per-request wrapper around a base [`HostStoreView`] with a
/// shadowing [`CanonicalCompletionOverlay`].
///
/// The wrapper owns the overlay via `Arc` and borrows the base view.
/// Constructed once at request entry; the
/// `HostResolverContext` / `SessionResolverContext` owns the wrapper as
/// a field so [`crate::resolver_core::ResolverContext::store_view`]
/// returns a borrow into the owned field — there is no temporary view
/// built per call.
///
/// ### Shadowing semantics
///
/// For every read, the overlay is consulted FIRST. If the overlay has
/// a key:
/// - A match against the queried fact → accept.
/// - A mismatch → REJECT (no fallthrough to the base view). The
///   overlay represents the request's observed truth for those keys;
///   any cached value that differs is stale.
///
/// If the overlay does not have a key, the read falls through to the
/// base view, which applies its own validation (untracked-file
/// optimistic-accept, tombstoned-canonical rejection, etc.).
pub(crate) struct RequestStoreView<'a> {
    base: &'a HostStoreView,
    overlay: Arc<CanonicalCompletionOverlay>,
}

impl<'a> RequestStoreView<'a> {
    /// Construct a wrapper over `(base, overlay)`.
    #[must_use]
    pub(crate) fn new(base: &'a HostStoreView, overlay: Arc<CanonicalCompletionOverlay>) -> Self {
        Self { base, overlay }
    }

    /// Borrow the request-scoped overlay.
    pub(crate) fn overlay(&self) -> &Arc<CanonicalCompletionOverlay> {
        &self.overlay
    }

    /// Borrow the base [`HostStoreView`].
    pub(crate) fn base(&self) -> &HostStoreView {
        self.base
    }

    /// Test-only: peek the overlay's `whole_hashes` entry for a
    /// canonical id. The discriminating tests for the epoch guard
    /// inspect this to assert that `complete_canonical` is a no-op
    /// when `host.current_store_view_epoch() != base.mutation_epoch()`.
    #[cfg(test)]
    pub(crate) fn peek_whole_hash_for_tests(&self, canonical_id: &str) -> Option<Hash16> {
        self.overlay.peek_whole_hash_for_tests(canonical_id)
    }
}

impl<'a> StoreView for RequestStoreView<'a> {
    #[inline]
    fn compat_token(&self) -> StoreViewCompatToken {
        // The overlay does not change project-wide identity; report the
        // base view's token unchanged so singleflight lanes still
        // coalesce.
        self.base.compat_token()
    }

    fn validates(&self, fact: &FactVersionRef) -> bool {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => {
                if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
                    // Shadowing: overlay value is authoritative; mismatch
                    // rejects without falling through.
                    return &overlay_hash == hash;
                }
                self.base.validates(fact)
            }
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => {
                match kind {
                    DerivedFactKind::DirectSource => {
                        // `DirectSource` is a content-hash alias for
                        // `FileWholeHash` — same shadowing arm as above.
                        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
                            return &overlay_hash == hash;
                        }
                        self.base.validates(fact)
                    }
                    _ => {
                        if let Some(overlay_hash) =
                            self.overlay.lookup_derived_hash(canonical_id, *kind)
                        {
                            return &overlay_hash == hash;
                        }
                        self.base.validates(fact)
                    }
                }
            }
            FactVersionRef::Parse(p) => self.validates_parse_domain(p),
            FactVersionRef::ResolveImports(r) => self.validates_resolve_imports_domain(r),
            FactVersionRef::RouteSurface(r) => self.validates_route_surface_domain(r),
            FactVersionRef::ProjectGeneration { .. } => {
                // ProjectGeneration validation is rooted on the base
                // view's snapshot; the overlay never alters project-wide
                // generation.
                self.base.validates(fact)
            }
        }
    }

    #[inline]
    fn tracks_file(&self, canonical_id: &str) -> bool {
        // Per-request overlay or base view tracks the canonical.
        self.overlay.tracks_file(canonical_id) || self.base.tracks_file(canonical_id)
    }

    fn validates_self_root_whole_hash(
        &self,
        canonical_id: &str,
        hash: &ResolverHash16,
    ) -> bool {
        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
            // Shadowing: overlay is authoritative for the self-root
            // identity.
            return &overlay_hash == hash;
        }
        self.base.validates_self_root_whole_hash(canonical_id, hash)
    }

    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];
        if let Some(overlay_facts) = self.overlay.lookup_file_facts(fact.canonical_id.as_str()) {
            // Shadowing: overlay `FileFacts` are authoritative for the
            // canonical. A registered fact must match; an absent fact
            // matches only the zero-sentinel observation.
            match overlay_facts.lookup(&fact.key) {
                Some(stored) => {
                    let stored_hash = match fact.lane {
                        verter_semantic::facts::registry::FactLane::Semantic => {
                            stored.semantic_hash
                        }
                        verter_semantic::facts::registry::FactLane::Display => stored.display_hash,
                    };
                    return stored_hash == fact.expected_hash;
                }
                None => return fact.expected_hash == ZERO_HASH,
            }
        }
        self.base.validates_parse_domain(fact)
    }

    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool {
        // The resolve-imports validator composes its
        // `ResolvedImportFactsKey` from
        // `(content_hash, known_miss_generation, env_hashes,
        // resolver_version)`. The overlay can refine
        // `content_hash` (via `whole_hashes`) and
        // `known_miss_generation` (via `resolved_import_facts_known_miss_tags`),
        // but the resolve-imports DB itself is an `Arc` shared with the
        // base view. So the shadowing behaviour is: if the overlay
        // refines a key dimension, route the lookup through the
        // refined key; if it does not, delegate to the base view.
        let overlay_whole_hash = self.overlay.lookup_whole_hash(fact.canonical_id.as_str());
        let overlay_known_miss = self
            .overlay
            .lookup_known_miss_tag(fact.canonical_id.as_str());

        if overlay_whole_hash.is_none() && overlay_known_miss.is_none() {
            return self.base.validates_resolve_imports_domain(fact);
        }

        // The base view's resolve-imports validator is private — it
        // composes the lookup key from base-view state we cannot access
        // directly here. The simplest correct approach is to delegate
        // to the base view: the overlay's whole-hash/known-miss refines
        // a per-canonical fact dimension that the resolve-imports lane
        // does not consume on a per-canonical-only basis (the lookup
        // depends on a project-wide `ResolvedImportFactsDb` that holds
        // both base and overlay candidates). The base validator will
        // either find the matching candidate via its content-addressed
        // key composition or fail closed.
        //
        // The shadowing contract is preserved by the `FileWholeHash` and
        // `Parse` arms above: if a stale fact observed the wrong
        // content hash, the underlying `Parse` / `FileWholeHash` facts
        // it depends on will fail validation first.
        self.base.validates_resolve_imports_domain(fact)
    }

    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        use verter_semantic::facts::FactKey;
        if let FactKey::ModuleAugmentationIndexShape {
            target_kind_tag,
            external_specifier,
            resolved_relative_canonical,
            wildcard_pattern,
        } = &fact.key
        {
            let key = RouteSurfaceIndexShapeKey {
                target_kind_tag: *target_kind_tag,
                external_specifier: external_specifier.as_ref().map(|s| s.as_ref().to_owned()),
                resolved_relative_canonical: resolved_relative_canonical
                    .as_ref()
                    .map(|s| s.as_ref().to_owned()),
                wildcard_pattern: wildcard_pattern.as_ref().map(|s| s.as_ref().to_owned()),
            };
            if let Some(overlay_hash) = self.overlay.lookup_route_surface_index_fingerprint(&key) {
                return overlay_hash == fact.expected_hash;
            }
        }
        self.base.validates_route_surface_domain(fact)
    }
}
