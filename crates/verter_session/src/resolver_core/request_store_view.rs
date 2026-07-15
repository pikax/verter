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
//! pipeline. But `ensure_loaded` and `ensure_indexed_ready_serve` deliberately
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
//! guarded**: if the host's
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
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
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
    /// Per-map monotonic "non-empty" flags (read-path
    /// hygiene). Set to `true` (Release) when the corresponding map
    /// receives its first insert; never flip back to `false` within a
    /// request (the overlay is append-only). Readers `load(Acquire)`
    /// and skip the `RwLock::read` + map lookup when the flag is
    /// `false` — a hot-path optimisation for the very common case of an
    /// empty overlay (validations that fire before any
    /// `complete_canonical` has run for the request).
    ///
    /// **Strict ordering:** the writer
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
    /// Request-scoped, SUCCESS-ONLY memo of session-overlay prepared-decl
    /// bundles, keyed per raw overlay owner by `(overlay content hash,
    /// store-view compat token)`.
    ///
    /// R17 forbids admitting an overlay-bearing bundle to the host's
    /// shared `prepared_decl_bundles` cache (the shared slot is keyed by
    /// canonical alone and would alias the base bundle), so pre-memo the
    /// session-tier resolver re-ran
    /// `materialize_prepared_decl_bundle_via_ctx` — including the full
    /// per-import re-export-chain walk
    /// (`build_prepared_import_canonicalization`) — on EVERY bundle touch.
    /// This memo is the request-scoped home for that value: it lives and
    /// dies with this overlay (one top-level request), never writes to any
    /// host/shared/store-level cache, and is NOT a request-local mirror of
    /// host state — the value it holds is exactly the one R17 keeps OUT of
    /// host state.
    ///
    /// Key semantics:
    /// - the overlay content hash pins entries to the session view's
    ///   frozen overlay bytes (the view is request-bound; its overlay maps
    ///   never change within the request);
    /// - the [`StoreViewCompatToken`] pins entries to ONE
    ///   externally-coherent base-world snapshot — the SAME complete
    ///   validity oracle singleflight lanes coalesce on (external
    ///   supersession dimensions folded, the request's own additive
    ///   artifact/load generations excluded). A `run_stable_request` retry
    ///   attempt re-snapshots the base view while SHARING this overlay, so
    ///   without the token a bundle whose import canonicalization walked
    ///   the superseded world could serve the fresh attempt; with it the
    ///   fresh attempt misses and re-materialises. The token also folds
    ///   the session-overlay identity (`with_session_overlay` recomputes
    ///   it from the overlay fingerprint), so two different session views
    ///   can never collide on a memo entry even if an overlay object were
    ///   ever shared between them.
    ///
    /// Success-only admission is enforced at the single producer call site
    /// (`prepared_decl_bundle_with_context`): a materialisation whose
    /// cacheability scope observed a NON-CACHEABLE read (fenced overlay
    /// serve, unrootable route, broken decl-body lease) is served to its
    /// caller but never inserted, preserving per-call re-materialisation —
    /// and the per-call non-cacheable fan-out into enclosing tracer
    /// scopes — for exactly the class where that fan-out is load-bearing.
    overlay_bundle_memo: RwLock<FxHashMap<String, OverlayBundleMemoEntry>>,
    overlay_bundle_memo_nonempty: AtomicBool,
}

/// Per-owner memo slot: the overlay content hash + view compat token the
/// bundle was materialised under, and the bundle itself. One slot per
/// raw overlay owner — a request observes ONE overlay content per owner
/// (the view is frozen) and ONE compat token per attempt, so a
/// hash/token move simply replaces the superseded entry.
struct OverlayBundleMemoEntry {
    overlay_hash: Hash16,
    token: StoreViewCompatToken,
    bundle: Arc<PreparedDeclBundle>,
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
            overlay_bundle_memo: RwLock::new(FxHashMap::default()),
            overlay_bundle_memo_nonempty: AtomicBool::new(false),
        }
    }

    /// Read the memoised session-overlay prepared-decl bundle for
    /// `(canonical, overlay_hash, token)`, if this request already
    /// materialised it under exactly that overlay content and view
    /// identity. See the [`Self::overlay_bundle_memo`] field docs for the
    /// key semantics.
    pub(crate) fn overlay_bundle_memo_get(
        &self,
        canonical: &str,
        overlay_hash: Hash16,
        token: StoreViewCompatToken,
    ) -> Option<Arc<PreparedDeclBundle>> {
        // Fast path: see `lookup_whole_hash` — skip the lock while the
        // request has memoised nothing.
        if !self.overlay_bundle_memo_nonempty.load(Ordering::Acquire) {
            return None;
        }
        let memo = self.overlay_bundle_memo.read();
        let entry = memo.get(canonical)?;
        (entry.overlay_hash == overlay_hash && entry.token == token)
            .then(|| Arc::clone(&entry.bundle))
    }

    /// Memoise a successfully-materialised session-overlay prepared-decl
    /// bundle for the rest of this request. Replaces a superseded entry
    /// for the same owner (an earlier attempt's hash/token). The caller
    /// owns the success-only gate — only a materialisation whose
    /// cacheability scope stayed clean may be inserted.
    pub(crate) fn overlay_bundle_memo_insert(
        &self,
        canonical: &str,
        overlay_hash: Hash16,
        token: StoreViewCompatToken,
        bundle: Arc<PreparedDeclBundle>,
    ) {
        // Same flag-after-insert ordering discipline as
        // `write_completion_entry` (flag stored under the write lock). The
        // race is benign here — a reader observing `false` just
        // re-materialises — but the overlay's writer pattern stays
        // uniform.
        let mut memo = self.overlay_bundle_memo.write();
        memo.insert(
            canonical.to_owned(),
            OverlayBundleMemoEntry {
                overlay_hash,
                token,
                bundle,
            },
        );
        self.overlay_bundle_memo_nonempty
            .store(true, Ordering::Release);
        drop(memo);
    }

    /// Test-only: number of memoised overlay bundles. The discriminating
    /// tests assert base-path / tombstone / non-cacheable reads never
    /// populate the memo.
    #[cfg(test)]
    pub(crate) fn overlay_bundle_memo_len_for_tests(&self) -> usize {
        self.overlay_bundle_memo.read().len()
    }

    /// Idempotently promote a freshly-loaded canonical's facts into the
    /// overlay.
    ///
    /// Called after a successful `ensure_loaded` / `ensure_indexed_ready_serve`
    /// on a canonical the request-entry base view does not track.
    /// Walking the host's currently-published per-canonical state is
    /// cheap (one `FileArtifactStore` lookup + a few scheduler reads);
    /// re-running the call after a prior completion is a no-op when the
    /// state has not changed.
    ///
    /// **Epoch guard:** if the host's current
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
    /// [`Self::write_completion_entry`] for the strict-ordering contract.
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

    /// Promote a producer-known canonical's facts into the overlay
    /// without consulting `host.scheduler` /
    /// `host.effective_file_state` / `host.project_type_store().indexed()`.
    ///
    /// The base [`Self::complete_canonical`] / `_inner` path resolves
    /// the canonical's `whole_hash` from the scheduler then loads
    /// `FileArtifacts` from the indexed authority to populate
    /// `file_facts` + derived hashes. A canonical the scheduler does
    /// not track and the indexed authority cannot answer for at
    /// completion time has NO artifact to read, so the
    /// indexed-authority fallback inside
    /// [`Self::write_completion_entry`] returns `None` and the route /
    /// import-route derived-hash entries never enter the overlay. The
    /// next warm-read validation of such a bundle's stored
    /// `ImportRoute` fact therefore falls through to the base view's
    /// `derived_hashes` snapshot — which itself does not see entries
    /// published after the snapshot was built — and rejects the
    /// bundle as `untracked`, causing a fresh cold rebuild every
    /// time.
    ///
    /// This method writes the producer-known `(whole_hash, route_hash,
    /// import_route_hash)` triple directly into the overlay. Each
    /// flag-after-insert ordering matches
    /// [`Self::write_completion_entry`] (lock held across the map
    /// insert + `_nonempty` flag store + release).
    ///
    /// The epoch guard against `host.current_store_view_epoch !=
    /// base.mutation_epoch()` lives at the producer-side call site
    /// (the host-tier prepared-decl-bundle materialiser holds the
    /// concrete `&VerterHost` and the base view, and can short-circuit
    /// before invoking this overlay write). Keeping `host` out of the
    /// resolver-tier API surface preserves the resolver-context seal
    /// (`no_concrete_verter_host_in_seal_scope` architecture guard).
    pub(crate) fn complete_route_canonical(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        route_hash: Option<Hash16>,
        import_route_hash: Option<Hash16>,
    ) {
        // Mirror the flag-after-insert ordering of
        // `write_completion_entry`: hold the lock across the map
        // insert + `_nonempty` flag store + release so a concurrent
        // reader observing `_nonempty == false` is guaranteed to also
        // observe the map as still empty for the canonical.
        {
            let mut whole = self.whole_hashes.write();
            whole.insert(canonical.to_owned(), whole_hash);
            self.whole_hashes_nonempty.store(true, Ordering::Release);
            drop(whole);
        }

        if route_hash.is_some() || import_route_hash.is_some() {
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

    fn complete_canonical_inner(
        &self,
        host: &crate::VerterHost,
        base: &HostStoreView,
        canonical: &str,
        view: Option<&dyn crate::session_view::SessionView>,
    ) {
        // The base view is superseded — skip the overlay write; the outer
        // executor retries against a fresh view. Gated on the COMPLETE
        // external-supersession dimensions (epoch / project-generation /
        // env / identity), NOT `store_view_epoch` alone: an env-hash shift
        // that moves no epoch still supersedes the base snapshot, so the
        // epoch-only check would let a stale overlay write through
        // (view-liveness). The compute's own artifact / route /
        // load-generation advances are deliberately EXCLUDED (the base
        // view stays live across its own dependency loads).
        //
        // The `overlay_identity` dimension is normalised OUT of the
        // comparison: `base` may be a session-overlaid view (its overlay
        // identity is `Some(_)`) while the host's live base token carries
        // `None`, and the request's frozen overlay is not an external
        // mutation — so an overlay-identity difference must NOT read as a
        // supersession here.
        let base_external = crate::resolver_store::StoreViewValidationToken {
            overlay_identity: None,
            ..base.validation_token()
        };
        if base_external.externally_superseded_by(&host.current_validation_token()) {
            return;
        }

        // Session-overlay precedence:
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
        // Flag-after-insert race fix:
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
            // Edge-currency gate. A wildcard-bearing artifact bakes its
            // `export *` edge `canonical_id`s from the dependency file set;
            // once `content_generation` advances past its edge generation BOTH
            // its route-surface hash and its baked import-route hash are stale.
            // Suppress both derived hashes so an entry rooted on them fails
            // warm validation and recomputes through the edge-gated readers
            // (which re-materialise the surface) rather than validating against
            // a stale hash recorded in the completion overlay.
            let edge_current = host.indexed_surface_is_current(canonical, indexed);
            let route_hash = if indexed.shallow_state.has_resolvable_surface() && edge_current {
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
            // canonicals the indexed authority is the overlay one. An
            // edge-stale wildcard surface suppresses it (same rail as the
            // route hash above) so the baked stale edge is never recorded.
            let import_route_hash = if edge_current {
                indexed
                    .import_route_hash
                    .or_else(|| host.generation_current_import_route_hash(canonical))
            } else {
                None
            };
            if route_hash.is_some() || import_route_hash.is_some() {
                // Single write-lock acquisition for both derived-hash
                // variants per canonical. Flag-set is performed under
                // the same lock so a reader observing `_nonempty == false`
                // is guaranteed to also observe the map as still empty
                // for the canonical (same race as the `whole_hashes` /
                // `file_facts` paths above).
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
    /// `ensure_loaded` flow. Used by the overlay-shape discriminating
    /// test in `block_6c_view_hoist_tests`.
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
    /// Whether the base view was proven current by the
    /// [`crate::resolver_store::StoreViewManager`] at request entry.
    ///
    /// When `false` (the base came from a known-stale
    /// [`crate::resolver_store::ColdSeedHostStoreView`] / `ReturnOnly`
    /// read), EVERY `validates*` method on this view fails CLOSED —
    /// returns `false` — so every nested warm-cache probe inside the
    /// dispatch MISSES rather than validating a cache entry against the
    /// stale seed. This is the construction-grade enforcement of the
    /// consult's "if cold-seeded, internal warm-cache probes must
    /// MISS/BYPASS rather than validate": no individual nested validator
    /// has to know about currentness — the single view they all read
    /// through refuses to validate. Structural reads (`tracks_file`,
    /// `derived_hash_for`, `compat_token`) and completion writes
    /// (`promote_route_completion`) are NOT validation-promotion
    /// and stay live so the cold compute can still observe additive loads.
    base_is_current: bool,
}

impl<'a> RequestStoreView<'a> {
    /// Construct a wrapper over `(base, overlay)` rooted on a
    /// proven-CURRENT base view. Every nested warm-cache probe may
    /// validate against it.
    #[must_use]
    pub(crate) fn new(base: &'a HostStoreView, overlay: Arc<CanonicalCompletionOverlay>) -> Self {
        Self {
            base,
            overlay,
            base_is_current: true,
        }
    }

    /// Construct a wrapper over a cold-seed `(base, overlay)`.
    ///
    /// `base_is_current` carries the seed's currentness
    /// ([`crate::resolver_store::ColdSeedHostStoreView::is_current`]): a
    /// cold builder that seeded from a freshly-current read still lets
    /// nested probes validate (`true`), while a `ReturnOnly` seed
    /// (`false`) fails every `validates*` closed so nothing stale can be
    /// warm-served through the derived context.
    #[must_use]
    pub(crate) fn new_cold_seed(
        base: &'a HostStoreView,
        overlay: Arc<CanonicalCompletionOverlay>,
        base_is_current: bool,
    ) -> Self {
        Self {
            base,
            overlay,
            base_is_current,
        }
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
        // Cold-seed fail-closed: a non-current base means the manager
        // could not prove the snapshot coherent, so no warm-cache entry
        // may validate through this view. Every nested probe misses and
        // falls to its own cold path (whose promotion fence rejects a
        // stale result).
        if !self.base_is_current {
            return false;
        }
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
            FactVersionRef::FileSourceEnv {
                canonical_id,
                parse_env_hash,
                parser_version,
                file_language_id,
            } => self.validates_file_source_env(
                canonical_id,
                *parse_env_hash,
                *parser_version,
                file_language_id,
            ),
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
        if !self.base_is_current {
            return false;
        }
        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
            // Shadowing: overlay is authoritative for the self-root
            // identity.
            return &overlay_hash == hash;
        }
        self.base.validates_self_root_whole_hash(canonical_id, hash)
    }

    /// Strict contributor source-env identity validation on the
    /// request-bound view: cold-seed fail-closed first, then the base
    /// view's snapshot comparison. The per-request completion overlay
    /// carries no source-env identities (`complete_canonical` promotes
    /// whole-hash / route facts only), so the base snapshot stays the
    /// sole identity authority here; an overlay-completed canonical
    /// with no base identity rejects strictly and recomputes.
    fn validates_file_source_env(
        &self,
        canonical_id: &str,
        parse_env_hash: crate::locator_identity::ParseEnvHash,
        parser_version: u32,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        if !self.base_is_current {
            return false;
        }
        self.base.validates_file_source_env(
            canonical_id,
            parse_env_hash,
            parser_version,
            file_language_id,
        )
    }

    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        if !self.base_is_current {
            return false;
        }
        const ZERO_HASH: Hash16 = [0u8; 16];
        if let Some(overlay_facts) = self.overlay.lookup_file_facts(fact.canonical_id.as_str()) {
            // Shadowing: overlay `FileFacts` are authoritative for the
            // canonical. A registered fact must match; an absent fact
            // matches only the zero-sentinel observation.
            match overlay_facts.lookup_or_compute(&fact.key) {
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
        if !self.base_is_current {
            return false;
        }
        // The base view's `ResolvedImportFactsDb` is content-addressed
        // by `(content_hash, known_miss_generation, env_hashes,
        // resolver_version)` and is an `Arc` shared between the base
        // and overlay paths. Concurrent completion writers update the
        // shared DB directly.
        //
        // Overlay-promoted canonicals: when
        // `ensure_loaded` / `ensure_indexed_ready_serve` promotes a canonical
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
        if !self.base_is_current {
            return false;
        }
        // The augmentation-index is a project-wide structural index
        // populated only by the base view's `snapshot_augmentation_index`
        // (a single one-shot pass during `HostStoreView::build`); it is
        // NOT a per-canonical fact, so `complete_canonical` cannot
        // meaningfully populate an overlay entry for it. Delegating
        // directly to the base view eliminates the dead probe + lock
        // acquire on every `ModuleAugmentationIndexShape` validation.
        self.base.validates_route_surface_domain(fact)
    }

    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        route_hash: Option<Hash16>,
        import_route_hash: Option<Hash16>,
    ) {
        // Route the call through the overlay's
        // `complete_route_canonical` writer — it writes
        // `whole_hashes` + `derived_hashes` entries with the same
        // flag-after-insert ordering as the standard
        // `write_completion_entry` path. The epoch guard lives at
        // the producer-side call site (where the concrete host is
        // available) so this trait method stays off the
        // `VerterHost` type and the resolver-context seal
        // (`no_concrete_verter_host_in_seal_scope` architecture
        // guard) keeps holding.
        self.overlay
            .complete_route_canonical(canonical, whole_hash, route_hash, import_route_hash);
    }
}
