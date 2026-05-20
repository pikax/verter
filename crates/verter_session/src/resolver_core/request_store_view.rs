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
use crate::resolver_store::HostStoreView;
use crate::types::Hash16;

/// Per-request append-only side maps recording additive loads that the
/// request-entry [`HostStoreView`] does not track.
///
/// Overlay shape (post-iter3 bug audit):
/// - `whole_hashes`
/// - `derived_hashes` (per-canonical bundled `RouteDerivedHashes`)
/// - `file_facts`
///
/// `import_routes`, `resolved_import_facts` handle, and `route_db`
/// handle stay OUT — they are `Arc` clones of project-wide `DashMap`s
/// and are already up-to-date through host-side concurrent writers.
///
/// The `route_surface_index_fingerprints` and
/// `resolved_import_facts_known_miss_tags` fields that previously lived
/// here were dead:
/// - `route_surface_index_fingerprints` — `complete_canonical` never
///   populated it (the augmentation index is a project-wide
///   structural index, not a per-canonical fact), so the read site at
///   `validates_route_surface_domain` always fell through to the base.
/// - `resolved_import_facts_known_miss_tags` — `validates_resolve_imports_domain`
///   probed `whole_hashes` + `known_miss_tags` and then
///   unconditionally delegated to `self.base` regardless of the result.
///   The shared `ResolvedImportFactsDb` is concurrently updated by
///   writers on both base and overlay paths, so the base validator
///   already sees mid-request additive entries.
/// Removing both eliminates hot-path probes + lock acquires per
/// validation.
///
/// Reads are wait-free against concurrent writers within the request
/// because [`RwLock`] readers do not block each other, and the
/// overlay's writers are scoped to
/// [`CanonicalCompletionOverlay::complete_canonical`] (one short
/// critical section per first-cold canonical).
pub(crate) struct CanonicalCompletionOverlay {
    whole_hashes: RwLock<FxHashMap<String, Hash16>>,
    /// Per-canonical derived hashes bundled by `DerivedFactKind` so a
    /// read can locate the entry with a `&str` lookup (no per-read
    /// owned tuple allocation).
    derived_hashes: RwLock<FxHashMap<String, RouteDerivedHashes>>,
    file_facts: RwLock<FxHashMap<String, Arc<FileFacts>>>,
}

/// Per-canonical derived hashes captured by the overlay. The fields
/// match the populated `DerivedFactKind` variants in
/// `CanonicalCompletionOverlay::complete_canonical`.
#[derive(Default)]
struct RouteDerivedHashes {
    route: Option<Hash16>,
    import_route: Option<Hash16>,
}

impl RouteDerivedHashes {
    fn get(&self, kind: DerivedFactKind) -> Option<Hash16> {
        match kind {
            DerivedFactKind::Route => self.route,
            DerivedFactKind::ImportRoute => self.import_route,
            // `DirectSource` is handled by the whole-hash arm in
            // `validates`; no overlay-derived hash is recorded for it.
            DerivedFactKind::DirectSource => None,
        }
    }
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
            let route_hash = if indexed.shallow_state.has_resolvable_surface() {
                Some(crate::resolver_store::hash_route_surface(
                    &indexed.shallow_state,
                ))
            } else {
                None
            };
            let import_route_hash = host.generation_current_import_route_hash(canonical);
            if route_hash.is_some() || import_route_hash.is_some() {
                // Single write-lock acquisition for both derived-hash
                // variants per canonical.
                let mut derived = self.derived_hashes.write();
                let entry = derived
                    .entry(canonical.to_owned())
                    .or_insert_with(RouteDerivedHashes::default);
                if let Some(h) = route_hash {
                    entry.route = Some(h);
                }
                if let Some(h) = import_route_hash {
                    entry.import_route = Some(h);
                }
            }
        }
    }

    fn lookup_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.read().get(canonical_id).copied()
    }

    fn lookup_derived_hash(&self, canonical_id: &str, kind: DerivedFactKind) -> Option<Hash16> {
        // `&str` lookup against `FxHashMap<String, _>` — no per-read
        // allocation (the previous `(String, DerivedFactKind)` keying
        // forced a `to_owned()` on every probe of every cache validity
        // check on the hot path).
        self.derived_hashes
            .read()
            .get(canonical_id)
            .and_then(|h| h.get(kind))
    }

    fn lookup_file_facts(&self, canonical_id: &str) -> Option<Arc<FileFacts>> {
        self.file_facts.read().get(canonical_id).cloned()
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

    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
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
        // The base view's `ResolvedImportFactsDb` is content-addressed
        // by `(content_hash, known_miss_generation, env_hashes,
        // resolver_version)` and is an `Arc` shared between the base
        // and overlay paths. Concurrent completion writers update the
        // shared DB directly, so the base validator already sees
        // mid-request additive entries.
        //
        // The previous overlay-probe arm acquired two RwLock reads
        // (`whole_hashes` + `known_miss_tags`) on every call and then
        // unconditionally delegated to `self.base` regardless. That was
        // pure overhead on the hot path — the shadowing contract is
        // already preserved by the `FileWholeHash` / `Parse` /
        // `DirectSource` arms above (a stale fact observed the wrong
        // content hash will fail the `Parse` / `FileWholeHash` fact it
        // depends on first).
        self.base.validates_resolve_imports_domain(fact)
    }

    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        // The augmentation-index is a project-wide structural index
        // populated only by the base view's `snapshot_augmentation_index`
        // (a single one-shot pass during `HostStoreView::build`); it is
        // NOT a per-canonical fact, so `complete_canonical` cannot
        // meaningfully populate an overlay entry for it. Delegating
        // directly to the base view eliminates the dead probe + lock
        // acquire on every `ModuleAugmentationIndexShape` validation.
        self.base.validates_route_surface_domain(fact)
    }
}
