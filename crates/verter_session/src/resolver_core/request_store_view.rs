//! Per-request shadowing wrapper around the immutable
//! [`HostStoreView`].
//!
//! ## Why
//!
//! [`crate::resolver_core::ReadSetSignature`].`facts` is the sole
//! cache-validity rail. Fact validation requires a live
//! [`HostStoreView`]; that view is an immutable snapshot of the
//! workspace's per-canonical facts at request-entry time.
//!
//! `HostStoreView::from_host` does 5-7 full workspace sweeps and
//! allocates ~5 hashmaps + an artifact `Vec` per call. The per-request hoist
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

use std::sync::atomic::{AtomicBool, Ordering};
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
///
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
    /// Per-map monotonic "non-empty" flags (codex shape 1 read-path
    /// hygiene). Set to `true` (Release) when the corresponding map
    /// receives its first insert; never flip back to `false` within a
    /// request (the overlay is append-only). Readers `load(Acquire)`
    /// and skip the `RwLock::read` + map lookup when the flag is
    /// `false` — a hot-path optimisation for the very common case of an
    /// empty overlay (validations that fire before any
    /// `complete_canonical` has run for the request).
    ///
    /// **Strict ordering (codex re-review B6.C-rfx2):** the writer
    /// sets the flag under the same write lock as the corresponding
    /// map insert (see [`Self::write_completion_entry`]). The lock is
    /// released only AFTER the flag has been stored. This pairs with
    /// the reader's `load(Acquire)` to guarantee that if a reader
    /// observes `_nonempty == false`, the writer has not yet inserted
    /// into the underlying map (otherwise the writer would also have
    /// stored `true` before releasing the lock). A reader can
    /// therefore safely return `None` on the fast path without
    /// falling into a stale base-view validation; reordering the
    /// store before the lock release would have left a window in
    /// which the map was populated but the flag still read `false`,
    /// causing concurrent readers to skip a real overlay entry and
    /// optimistically accept a stale cached dependency via the base
    /// view's untracked-canonical accept rule. Strict shadowing
    /// correctness is preserved.
    whole_hashes_nonempty: AtomicBool,
    derived_hashes_nonempty: AtomicBool,
    file_facts_nonempty: AtomicBool,
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
            whole_hashes_nonempty: AtomicBool::new(false),
            derived_hashes_nonempty: AtomicBool::new(false),
            file_facts_nonempty: AtomicBool::new(false),
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
    ///
    /// This is the BASE-only variant — used by [`HostResolverContext`]
    /// where no session view is present. Session-bearing contexts must
    /// call [`Self::complete_canonical_with_session_view`] instead so
    /// the completion overlay records the session-overlay hash (not the
    /// base scheduler hash) for canonicals the session has masked. See
    /// the codex review fix (B6.C-rfx) for the contract.
    pub(crate) fn complete_canonical(
        &self,
        host: &crate::VerterHost,
        base: &HostStoreView,
        canonical: &str,
    ) {
        self.complete_canonical_inner(host, base, canonical, None);
    }

    /// Session-overlay-aware variant of [`Self::complete_canonical`].
    ///
    /// When `view` carries an explicit overlay-Upsert for `canonical`
    /// the overlay hash (and overlay-published artifacts) is the
    /// authoritative source for the completion overlay's entries, NOT
    /// the base host's scheduler-rooted state. Without this routing the
    /// completion overlay's `whole_hashes` would shadow the session-
    /// overlay's hash with the base hash, breaking the 6.B session-
    /// overlay validation contract (`096e124a2`): a session-overlaid
    /// canonical's facts would mis-validate against the base hash on
    /// subsequent reads inside the same request.
    ///
    /// Resolution order (mirrors [`SessionResolverContext::authoritative_current_content_hash`]):
    /// 1. `view.overlay_content_hash_for(canonical)` returns `Some` →
    ///    write the overlay hash; load the overlay's
    ///    [`FileArtifacts`] (not the base) for `file_facts` / derived
    ///    hashes.
    /// 2. `view.is_tombstoned(canonical)` → skip entirely (the session
    ///    deleted the file; there is no current content to promote).
    /// 3. Otherwise → fall through to the base-only logic.
    ///
    /// Epoch-guarded identically to [`Self::complete_canonical`].
    pub(crate) fn complete_canonical_with_session_view(
        &self,
        host: &crate::VerterHost,
        base: &HostStoreView,
        view: &dyn crate::session_view::SessionView,
        canonical: &str,
    ) {
        self.complete_canonical_inner(host, base, canonical, Some(view));
    }

    fn complete_canonical_inner(
        &self,
        host: &crate::VerterHost,
        base: &HostStoreView,
        canonical: &str,
        view: Option<&dyn crate::session_view::SessionView>,
    ) {
        if host.current_store_view_epoch() != base.mutation_epoch() {
            // The base view is already superseded. Skip the write; the
            // outer executor will retry against a fresh view.
            return;
        }

        // Session-overlay precedence (codex review B6.C-rfx fix):
        // if a session view is present and carries explicit overlay
        // state for the canonical, that state is the request-scoped
        // authority — NOT the base scheduler. Without this branch the
        // completion overlay would shadow the session-rooted base
        // view's overlay hash with the scheduler's base hash, breaking
        // the 6.B session-overlay validation contract (096e124a2).
        if let Some(view) = view {
            if view.is_tombstoned(canonical) {
                // The session deleted the file; there is no current
                // content. The `with_session_overlay` re-rooting on
                // `base` has already dropped any base per-canonical
                // snapshot, so the completion overlay must not promote
                // a stale state on top.
                return;
            }
            if let Some(overlay_hash) = view.overlay_content_hash_for(canonical) {
                self.write_completion_entry_from_overlay(host, view, canonical, overlay_hash);
                return;
            }
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

        self.write_completion_entry(host, canonical, whole_hash, None);
    }

    /// Write the completion-overlay entries for a session-overlaid
    /// canonical. Reads the overlay [`FileArtifacts`] (not the base)
    /// so `file_facts` and the derived hashes match the OVERLAY
    /// content version — the same authority `HostStoreView::with_session_overlay`
    /// re-rooted the base view's per-canonical snapshots from.
    fn write_completion_entry_from_overlay(
        &self,
        host: &crate::VerterHost,
        view: &dyn crate::session_view::SessionView,
        canonical: &str,
        overlay_hash: crate::types::Hash16,
    ) {
        let overlay_identity = host.overlay_artifact_identity(canonical);
        let file_artifacts = overlay_identity.lookup_overlay_artifacts(host, view);
        self.write_completion_entry(host, canonical, overlay_hash, file_artifacts);
    }

    /// Write the completion-overlay entries for `(canonical, whole_hash)`.
    /// When `file_artifacts` is `None`, falls back to a content-hash-
    /// keyed `get_artifacts_for_content` read on the project store (the
    /// base-only path used when no session view is in play).
    fn write_completion_entry(
        &self,
        host: &crate::VerterHost,
        canonical: &str,
        whole_hash: crate::types::Hash16,
        file_artifacts: Option<Arc<crate::file_artifact_store::FileArtifacts>>,
    ) {
        // Flag-after-insert race fix (codex re-review P2 of b30005ed0):
        // the `_nonempty` flag MUST be set BEFORE the write lock is
        // released, otherwise a concurrent reader can observe
        // `_nonempty == false` (and skip the overlay via the
        // `lookup_*` fast paths) AFTER the map insert has already
        // taken effect, falling through to the base view whose
        // `FileWholeHash` validator optimistically accepts a stale
        // cached dependency for an untracked canonical. Holding the
        // lock across the flag-store closes that window: if a reader
        // observes `false`, the writer has not yet inserted into the
        // map (the writer's insert + flag-store + lock-release happen
        // as one critical section).
        {
            let mut whole = self.whole_hashes.write();
            whole.insert(canonical.to_owned(), whole_hash);
            self.whole_hashes_nonempty.store(true, Ordering::Release);
            drop(whole);
        }

        // Per-canonical `IndexedReady` snapshot — populates `file_facts`
        // and the `Route` / `ImportRoute` derived-hash entries. For a
        // session-overlaid canonical the caller passes the overlay
        // artifacts directly; for the base-only path we look them up by
        // content hash here.
        let file_artifacts = file_artifacts.or_else(|| {
            host.project_type_store()
                .indexed()
                .get_artifacts_for_content(canonical, whole_hash)
        });
        if let Some(file_artifacts) = file_artifacts {
            {
                let mut facts = self.file_facts.write();
                facts.insert(canonical.to_owned(), Arc::clone(&file_artifacts.facts));
                self.file_facts_nonempty.store(true, Ordering::Release);
                drop(facts);
            }

            let indexed = &file_artifacts.indexed;
            let route_hash = if indexed.shallow_state.has_resolvable_surface() {
                Some(crate::resolver_store::hash_route_surface(
                    &indexed.shallow_state,
                ))
            } else {
                None
            };
            // For session-overlaid canonicals the import-route hash is
            // covered by the overlay `IndexedReady`'s own
            // `import_route_hash` (the authority `with_session_overlay`
            // also reads). For the base-only path we read it from the
            // host's generation-current map. Both produce the same
            // value for non-overlaid canonicals; for overlaid
            // canonicals the indexed authority is the overlay one.
            let import_route_hash = indexed
                .import_route_hash
                .or_else(|| host.generation_current_import_route_hash(canonical));
            if route_hash.is_some() || import_route_hash.is_some() {
                // Single write-lock acquisition for both derived-hash
                // variants per canonical. Flag-set is performed under
                // the same lock so a reader observing `_nonempty == false`
                // is guaranteed to also observe the map as still empty
                // for the canonical (codex re-review P2 fix — same race
                // as the `whole_hashes` / `file_facts` paths above).
                let mut derived = self.derived_hashes.write();
                let entry = derived.entry(canonical.to_owned()).or_default();
                if let Some(h) = route_hash {
                    entry.route = Some(h);
                }
                if let Some(h) = import_route_hash {
                    entry.import_route = Some(h);
                }
                self.derived_hashes_nonempty.store(true, Ordering::Release);
                drop(derived);
            }
        }
    }

    fn lookup_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        // Fast path: if no completion has written to this map within
        // the request, skip the `RwLock::read` + map lookup entirely.
        if !self.whole_hashes_nonempty.load(Ordering::Acquire) {
            return None;
        }
        self.whole_hashes.read().get(canonical_id).copied()
    }

    fn lookup_derived_hash(&self, canonical_id: &str, kind: DerivedFactKind) -> Option<Hash16> {
        // Fast path: see `lookup_whole_hash`.
        if !self.derived_hashes_nonempty.load(Ordering::Acquire) {
            return None;
        }
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
        // Fast path: see `lookup_whole_hash`.
        if !self.file_facts_nonempty.load(Ordering::Acquire) {
            return None;
        }
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

    /// Test-only: directly insert a `(canonical, whole_hash)` entry into
    /// the overlay's `whole_hashes` map and toggle the `_nonempty`
    /// flag. Bypasses [`Self::complete_canonical`]'s host-state
    /// lookups + the epoch guard so a test can stage the exact
    /// overlay shape it needs without driving the full
    /// `ensure_loaded` flow. Used by the codex re-review B6.C-rfx2
    /// P2 #2 discriminating test in `block_6c_view_hoist_tests`.
    #[cfg(test)]
    pub(crate) fn insert_whole_hash_for_tests(&self, canonical: &str, whole_hash: Hash16) {
        let mut whole = self.whole_hashes.write();
        whole.insert(canonical.to_owned(), whole_hash);
        self.whole_hashes_nonempty.store(true, Ordering::Release);
        drop(whole);
    }

    /// Test-only: directly insert a `(canonical, kind, hash)` derived-hash
    /// entry into the overlay's `derived_hashes` map and toggle the
    /// `_nonempty` flag. Same bypass rationale as
    /// [`Self::insert_whole_hash_for_tests`]: lets a test stage the
    /// exact overlay-only derived-hash shape (no base-view snapshot)
    /// without driving `complete_canonical`. Used by the discriminating
    /// test for `RequestStoreView::derived_hash_for` overlay coverage.
    #[cfg(test)]
    pub(crate) fn insert_derived_hash_for_tests(
        &self,
        canonical: &str,
        kind: DerivedFactKind,
        hash: Hash16,
    ) {
        let mut derived = self.derived_hashes.write();
        let entry = derived.entry(canonical.to_owned()).or_default();
        match kind {
            DerivedFactKind::Route => entry.route = Some(hash),
            DerivedFactKind::ImportRoute => entry.import_route = Some(hash),
            // `DirectSource` is handled by the whole-hash arm; the
            // overlay never records a derived hash for it (matches the
            // `RouteDerivedHashes::get` contract).
            DerivedFactKind::DirectSource => {}
        }
        self.derived_hashes_nonempty.store(true, Ordering::Release);
        drop(derived);
    }

    /// Whether the overlay tracks `canonical_id` (any per-canonical
    /// entry).
    fn tracks_file(&self, canonical_id: &str) -> bool {
        // Fast path: if both maps are empty (their `_nonempty` flags
        // are still `false`), no lock acquisition is required.
        let whole_nonempty = self.whole_hashes_nonempty.load(Ordering::Acquire);
        let file_nonempty = self.file_facts_nonempty.load(Ordering::Acquire);
        if !whole_nonempty && !file_nonempty {
            return false;
        }
        (whole_nonempty && self.whole_hashes.read().contains_key(canonical_id))
            || (file_nonempty && self.file_facts.read().contains_key(canonical_id))
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

    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        // Mirror the overlay-shadowing pattern used by `validates` for
        // `DerivedFactHash`: the overlay's `derived_hashes` (populated
        // by mid-request `complete_canonical`) is authoritative; if the
        // overlay has no entry for the pair, fall through to the base
        // view's snapshotted derived hashes.
        //
        // Without this override, per-rejection attribution helpers
        // (`attribute_prepared_decl_bundle_rejection`) call this on a
        // `RequestStoreView` and hit the default `None` arm — every
        // real `ImportRoute` hash mismatch then reclassifies as
        // `_absent` and the discriminating counter loses its meaning.
        if let Some(overlay_hash) = self.overlay.lookup_derived_hash(canonical_id, kind) {
            return Some(overlay_hash);
        }
        self.base.derived_hash_for(canonical_id, kind)
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
        // shared DB directly.
        //
        // Overlay-promoted canonicals (codex re-review B6.C-rfx2): when
        // `ensure_loaded` / `ensure_indexed_ready` promotes a canonical
        // that the request-entry base view did not track, the overlay
        // carries the authoritative whole hash but the base view's
        // `whole_hashes` snapshot does not. Falling straight through to
        // the base validator would then compose the
        // `ResolvedImportFactsKey` against the absent snapshot entry
        // and reject every real `ResolveImports` fact on the promoted
        // canonical — even when the shared DB has been populated under
        // the overlay's content hash. Compose the key using the
        // overlay's whole hash via the
        // `validates_resolve_imports_domain_for_content_hash` helper
        // for any canonical the overlay tracks, so warm hits on
        // freshly-promoted dependencies validate correctly inside the
        // same request.
        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(fact.canonical_id.as_str()) {
            return self
                .base
                .validates_resolve_imports_domain_for_content_hash(fact, overlay_hash);
        }
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
