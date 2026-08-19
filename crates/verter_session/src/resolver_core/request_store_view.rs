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
//! `HostStoreView::from_host` captures immutable store roots in O(1). The
//! per-request wrapper keeps one fixed view and threads a borrow through the
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
//! - [`CanonicalCompletionOverlay`]: request-scoped shadowing side maps
//!   that record additive loads observed mid-request. Their key sets only
//!   grow, but an effective value may be replaced; one bracketed revision
//!   identifies each stable shadowing state.
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
//! The completion overlay does NOT participate in
//! [`crate::resolver_core::StoreView::compat_token`]: the wrapper
//! reports the base's compat token unchanged. Two concurrent requests
//! with the same base epoch must still coalesce on singleflight lanes,
//! while fact signatures distinguish completion states through their
//! [`verter_workspace::ViewPopulation`]. This separation is deliberate:
//! the compat token is the frozen base/session lane identity, whereas the
//! completion population may advance inside one request.
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
//! - **Family memo gating + FIFO prune**: the overlay
//!   changes validation visibility for facts already observed; it
//!   does not change what `traced_facts`, `dispatch_dep_signature`,
//!   `canonical_ids()`, or FIFO prune register. Consequently the generic
//!   FIFO still cannot prefer durable Base/Session candidates over a
//!   request-completion candidate; the population is visible only inside
//!   `fact_dep_signature`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use verter_workspace::{CompletionOverlayState, OverlayId};

use crate::file_artifact_store::FileFacts;
use crate::resolver_core::bracketed_generation::BracketedGeneration;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
use crate::resolver_core::reuse::ReuseClass;
use crate::resolver_core::{
    DerivedFactKind, FactVersionRef, ParseFactRef, ResolveImportsFactRef, ResolverHash16,
    RouteSurfaceFactRef, StoreView, StoreViewCompatToken,
};
use crate::resolver_store::HostStoreView;
use crate::types::Hash16;

/// Per-request shadowing side maps recording additive loads that the
/// request-entry [`HostStoreView`] does not track. Keys are retained for
/// the request lifetime, while equal replacement is a no-op and changed
/// replacement advances [`Self::revision`].
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
    /// Process-unique identity plus a bracketed revision of the exact
    /// shadowing state. The memo map below is deliberately excluded.
    overlay_id: OverlayId,
    revision: BracketedGeneration,
    whole_hashes: RwLock<FxHashMap<String, Hash16>>,
    /// Per-canonical derived hashes bundled by `DerivedFactKind` so a
    /// read can locate the entry with a `&str` lookup (no per-read
    /// owned tuple allocation).
    derived_hashes: RwLock<FxHashMap<String, RouteDerivedHashes>>,
    file_facts: RwLock<FxHashMap<String, Arc<FileFacts>>>,
    /// Per-map monotonic "non-empty" flags (read-path
    /// hygiene). Set to `true` (Release) when the corresponding map
    /// receives its first insert; never flip back to `false` within a
    /// request (the maps never become empty again). Readers `load(Acquire)`
    /// and skip the `RwLock::read` + map lookup when the flag is
    /// `false` — a hot-path optimisation for the very common case of an
    /// empty overlay (validations that fire before any
    /// `complete_canonical` has run for the request).
    ///
    /// **Strict ordering:** after acquiring the map's write lock, the
    /// writer sets the flag BEFORE inserting (see the `write_*` helpers).
    /// A reader that still observes `false` therefore precedes the insert;
    /// a reader that observes `true` takes the read lock and cannot inspect
    /// the map until the insertion completes. Setting the flag after the
    /// insert would leave a false-negative window even if both operations
    /// occurred under the same lock, because the false fast path skips that
    /// lock entirely. A reader can therefore safely return `None` without
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
    /// Request-scoped memo of prepared-decl bundles — the ONE
    /// request-world memo covering the base, session-overlay and
    /// `RequestOnly` worlds. See [`RequestBundleMemo`].
    bundle_memo: RequestBundleMemo,
    #[cfg(test)]
    verify_write_protocol: AtomicBool,
}

/// Which world a memoised prepared-decl bundle was materialised for.
///
/// Base and session-overlay bundles for the SAME canonical are different
/// values — the overlay one is built from the session's frozen bytes, the
/// base one from the store-current artifact — so they occupy DISTINCT
/// namespaces. Collapsing them would serve a base consumer the session's
/// edit (and vice versa) inside the same request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BundleMemoWorld {
    /// The base (non-overlaid) bundle.
    Base,
    /// The bundle built from the session view's overlay content, keyed by
    /// that overlay's content hash so a request that re-snapshots under
    /// different overlay bytes cannot reuse the previous one.
    Overlay(Hash16),
}

/// Per-`(canonical, world)` memo slot.
struct BundleMemoEntry {
    /// The store-view compat token the bundle was materialised under —
    /// the SAME complete external-coherence oracle singleflight lanes
    /// coalesce on. A stability-retry attempt re-snapshots the base view,
    /// so without the token a bundle whose import canonicalization walked
    /// the superseded world could serve the fresh attempt.
    token: StoreViewCompatToken,
    /// How far the memoised value may travel. `RequestOnly` entries
    /// replay their refusal on EVERY hit.
    reuse: ReuseClass,
    bundle: Arc<PreparedDeclBundle>,
}

/// The ONE request-world prepared-decl bundle memo.
///
/// ## What it is for
///
/// A prepared-decl bundle that cannot be SHARED still costs a full cold
/// materialisation — including the per-import re-export-chain walk — on
/// every touch. Two classes hit this:
///
/// * an overlay-bearing bundle, which R17 keeps out of the shared
///   `prepared_decl_bundles` cache because that slot is keyed by
///   canonical alone and would alias the base bundle;
/// * a `RequestOnly` bundle, whose materialisation consumed a
///   deterministic non-cacheable read (a FENCED serve, an unrootable
///   import-route witness), so the shared admission gate declines it.
///
/// Both are COMPLETE and deterministic under the request's immutable
/// view. This memo is the request-scoped home for exactly those values:
/// it lives and dies with one `CanonicalCompletionOverlay` (one top-level
/// request), never writes to any host / shared / persistent cache, and is
/// NOT a request-local mirror of host state — the values it holds are
/// precisely the ones host state must not hold.
///
/// ## Identity
///
/// The key is `(canonical, world)` and the entry carries the
/// [`StoreViewCompatToken`]; a token mismatch is a MISS that replaces the
/// superseded entry. The token folds the external-supersession
/// dimensions AND the session-overlay identity, so it supplies the
/// store-view validation token, the resolution-world identity, the
/// population and the session/overlay identity in one comparison, while
/// [`BundleMemoWorld`] supplies the base/overlay namespace split.
///
/// ## Admission is structural, not by convention
///
/// [`Self::insert`] itself refuses anything that is not
/// [`ReuseClass::is_request_reusable`] — a cancelled, partial,
/// lease-missed, mutation-unstable or overflow-refused materialisation
/// cannot be memoised even by a caller that asks. That keeps the rule at
/// ONE place instead of at every producer.
#[derive(Default)]
pub(crate) struct RequestBundleMemo {
    entries: RwLock<FxHashMap<(String, BundleMemoWorld), BundleMemoEntry>>,
    /// Monotonic "non-empty" flag with the same flag-after-insert
    /// ordering discipline as `write_completion_entry`: readers skip the
    /// lock entirely while the request has memoised nothing.
    nonempty: AtomicBool,
}

impl RequestBundleMemo {
    /// Read the memoised bundle for `(canonical, world)` if this request
    /// already materialised it under exactly this view identity.
    ///
    /// Returns the bundle together with its [`ReuseClass`]; the caller
    /// must [`ReuseClass::replay_refusal`] before returning the value, or
    /// the reuse launders the taint the cold return carried.
    pub(crate) fn get(
        &self,
        canonical: &str,
        world: BundleMemoWorld,
        token: StoreViewCompatToken,
    ) -> Option<(Arc<PreparedDeclBundle>, ReuseClass)> {
        if !self.nonempty.load(Ordering::Acquire) {
            return None;
        }
        let entries = self.entries.read();
        // One owned key per miss is the price of a tuple key; the
        // `nonempty` fast path keeps it off the empty-memo hot path.
        let entry = entries.get(&(canonical.to_owned(), world))?;
        (entry.token == token).then(|| (Arc::clone(&entry.bundle), entry.reuse))
    }

    /// Memoise a request-reusable bundle for the rest of this request.
    /// Replaces a superseded entry for the same `(canonical, world)` (an
    /// earlier attempt's token). A non-request-reusable class is REFUSED
    /// here rather than at the call site.
    pub(crate) fn insert(
        &self,
        canonical: &str,
        world: BundleMemoWorld,
        token: StoreViewCompatToken,
        reuse: ReuseClass,
        bundle: Arc<PreparedDeclBundle>,
    ) {
        if !reuse.is_request_reusable() {
            return;
        }
        let mut entries = self.entries.write();
        entries.insert(
            (canonical.to_owned(), world),
            BundleMemoEntry {
                token,
                reuse,
                bundle,
            },
        );
        self.nonempty.store(true, Ordering::Release);
        drop(entries);
    }

    /// Test-only: number of memoised bundles across all worlds.
    #[cfg(test)]
    pub(crate) fn len_for_tests(&self) -> usize {
        self.entries.read().len()
    }

    /// Test-only: number of memoised bundles in ONE world — the
    /// discriminator for base/overlay namespace isolation.
    #[cfg(test)]
    pub(crate) fn len_in_world_for_tests(&self, world: BundleMemoWorld) -> usize {
        self.entries
            .read()
            .keys()
            .filter(|(_, entry_world)| *entry_world == world)
            .count()
    }
}

/// Per-canonical derived hashes captured by the overlay. The fields
/// match the populated `DerivedFactKind` variants in
/// `CanonicalCompletionOverlay::complete_canonical`.
#[derive(Default)]
struct RouteDerivedHashes {
    route: Option<Hash16>,
}

impl RouteDerivedHashes {
    fn get(&self, kind: DerivedFactKind) -> Option<Hash16> {
        match kind {
            DerivedFactKind::Route => self.route,
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
            overlay_id: OverlayId::fresh(),
            revision: BracketedGeneration::default(),
            whole_hashes: RwLock::new(FxHashMap::default()),
            derived_hashes: RwLock::new(FxHashMap::default()),
            file_facts: RwLock::new(FxHashMap::default()),
            whole_hashes_nonempty: AtomicBool::new(false),
            derived_hashes_nonempty: AtomicBool::new(false),
            file_facts_nonempty: AtomicBool::new(false),
            bundle_memo: RequestBundleMemo::default(),
            #[cfg(test)]
            verify_write_protocol: AtomicBool::new(false),
        }
    }

    /// Snapshot the overlay's exact validation-visible shadowing state.
    ///
    /// The revision is sampled on both sides of the non-empty flags. A
    /// writer that overlaps either sample makes the state unreadable;
    /// one that completes between them leaves a different revision. In
    /// both cases the caller receives `InFlight` rather than pairing a
    /// revision with flags from another state.
    fn completion_state(&self) -> CompletionOverlayState {
        let Some(before) = self.revision.stable() else {
            return CompletionOverlayState::InFlight;
        };
        let shadows = self.whole_hashes_nonempty.load(Ordering::Acquire)
            || self.derived_hashes_nonempty.load(Ordering::Acquire)
            || self.file_facts_nonempty.load(Ordering::Acquire);
        let Some(after) = self.revision.stable() else {
            return CompletionOverlayState::InFlight;
        };
        if before != after {
            return CompletionOverlayState::InFlight;
        }
        if shadows {
            CompletionOverlayState::Shadowing {
                overlay_id: self.overlay_id,
                revision: before,
            }
        } else {
            CompletionOverlayState::Empty
        }
    }

    #[cfg(test)]
    pub(crate) fn completion_state_for_tests(&self) -> CompletionOverlayState {
        self.completion_state()
    }

    #[cfg(test)]
    pub(crate) fn verify_write_protocol_for_tests(&self) {
        self.verify_write_protocol.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn hold_revision_in_flight_for_tests(
        &self,
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) {
        self.revision.mutate(|| {
            entered.send(()).expect("test reader must be waiting");
            release.recv().expect("test writer must be released");
            ((), false)
        });
    }

    fn write_whole_hash(&self, canonical: &str, whole_hash: Hash16) -> bool {
        let mut whole = self.whole_hashes.write();
        self.whole_hashes_nonempty.store(true, Ordering::Release);
        #[cfg(test)]
        if self.verify_write_protocol.load(Ordering::Relaxed) {
            assert!(self.whole_hashes_nonempty.load(Ordering::Acquire));
            assert!(
                self.whole_hashes.try_read().is_none(),
                "the presence flag must be published while the map write lock is held"
            );
        }
        whole.insert(canonical.to_owned(), whole_hash) != Some(whole_hash)
    }

    fn write_route_hash(&self, canonical: &str, route_hash: Hash16) -> bool {
        let mut derived = self.derived_hashes.write();
        self.derived_hashes_nonempty.store(true, Ordering::Release);
        #[cfg(test)]
        if self.verify_write_protocol.load(Ordering::Relaxed) {
            assert!(self.derived_hashes_nonempty.load(Ordering::Acquire));
            assert!(
                self.derived_hashes.try_read().is_none(),
                "the presence flag must be published while the map write lock is held"
            );
        }
        let entry = derived.entry(canonical.to_owned()).or_default();
        entry.route.replace(route_hash) != Some(route_hash)
    }

    fn write_file_facts(&self, canonical: &str, file_facts: Arc<FileFacts>) -> bool {
        let mut facts = self.file_facts.write();
        self.file_facts_nonempty.store(true, Ordering::Release);
        #[cfg(test)]
        if self.verify_write_protocol.load(Ordering::Relaxed) {
            assert!(self.file_facts_nonempty.load(Ordering::Acquire));
            assert!(
                self.file_facts.try_read().is_none(),
                "the presence flag must be published while the map write lock is held"
            );
        }
        facts
            .insert(canonical.to_owned(), Arc::clone(&file_facts))
            .is_none_or(|previous| previous.as_ref() != file_facts.as_ref())
    }

    /// The request's prepared-decl bundle memo — the request-world reuse
    /// tier every bundle producer consults. See [`RequestBundleMemo`].
    #[inline]
    pub(crate) fn bundle_memo(&self) -> &RequestBundleMemo {
        &self.bundle_memo
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
    /// `file_facts` + the authored route-surface hash. A canonical the scheduler does
    /// not track and the indexed authority cannot answer for at
    /// completion time has NO artifact to read, so the
    /// indexed-authority fallback inside
    /// [`Self::write_completion_entry`] returns `None` and the route
    /// derived-hash entry never enters the overlay. The next warm-read
    /// validation therefore falls through to the immutable base roots,
    /// which cannot see an artifact published after their capture, and
    /// rejects the bundle as untracked, causing a fresh cold rebuild.
    ///
    /// This method writes the producer-known `(whole_hash, route_hash)`
    /// pair directly into the overlay. Each
    /// presence-before-insert ordering matches the `write_*` helpers
    /// (lock held across flag publication and map insertion).
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
    ) {
        self.revision.mutate(|| {
            let mut changed = self.write_whole_hash(canonical, whole_hash);
            if let Some(route_hash) = route_hash {
                changed |= self.write_route_hash(canonical, route_hash);
            }
            ((), changed)
        });
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
        // Per-canonical `IndexedReady` projection — populates `file_facts`
        // and the authored `Route` derived-hash entry. For a
        // session-overlaid canonical the caller passes the overlay
        // artifacts directly; for the base-only path we look them up by
        // content hash here.
        let file_artifacts = file_artifacts.or_else(|| {
            let key = host.authoritative_current_artifact_key(canonical)?;
            if key.content_hash != whole_hash {
                return None;
            }
            host.project_type_store()
                .indexed()
                .get_artifacts_for_content(
                    canonical,
                    whole_hash,
                    &key.parse_key,
                    &key.file_language_id,
                )
        });
        let route_hash = file_artifacts.as_ref().and_then(|file_artifacts| {
            let indexed = &file_artifacts.indexed;
            // Parse-environment reuse gate. The route surface is authored
            // parse state, so an artifact built under a different parse
            // environment must not publish its hash into this overlay.
            let edge_current = host.indexed_surface_is_current(canonical, indexed);
            if indexed.shallow_state.has_resolvable_surface() && edge_current {
                Some(crate::resolver_store::hash_route_surface(
                    &indexed.shallow_state,
                ))
            } else {
                None
            }
        });

        // One revision brackets the whole logical promotion even though
        // its effective shadowing spans three maps. A reader can therefore
        // never name a population for a partially-published completion.
        self.revision.mutate(|| {
            let mut changed = self.write_whole_hash(canonical, whole_hash);
            if let Some(file_artifacts) = file_artifacts {
                changed |= self.write_file_facts(canonical, Arc::clone(&file_artifacts.facts));
            }
            // The owner's import-route dependency is not a completion-
            // overlay fact. Only the authored route surface is shadowed.
            if let Some(route_hash) = route_hash {
                changed |= self.write_route_hash(canonical, route_hash);
            }
            ((), changed)
        });
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
        self.revision
            .mutate(|| ((), self.write_whole_hash(canonical, whole_hash)));
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
        self.revision.mutate(|| {
            let changed = match kind {
                DerivedFactKind::Route => self.write_route_hash(canonical, hash),
                // `DirectSource` is handled by the whole-hash arm; the
                // overlay never records a derived hash for it.
                DerivedFactKind::DirectSource => false,
            };
            ((), changed)
        });
    }

    #[cfg(test)]
    pub(crate) fn insert_file_facts_for_tests(&self, canonical: &str, facts: Arc<FileFacts>) {
        self.revision
            .mutate(|| ((), self.write_file_facts(canonical, facts)));
    }

    #[cfg(test)]
    pub(crate) fn lookup_derived_hash_for_tests(
        &self,
        canonical: &str,
        kind: DerivedFactKind,
    ) -> Option<Hash16> {
        self.lookup_derived_hash(canonical, kind)
    }

    #[cfg(test)]
    pub(crate) fn tracks_file_for_tests(&self, canonical: &str) -> bool {
        self.tracks_file(canonical)
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
    #[cfg(test)]
    validation_step_hook: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    #[cfg(test)]
    validation_step: AtomicUsize,
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
            #[cfg(test)]
            validation_step_hook: None,
            #[cfg(test)]
            validation_step: AtomicUsize::new(0),
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
            #[cfg(test)]
            validation_step_hook: None,
            #[cfg(test)]
            validation_step: AtomicUsize::new(0),
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

    #[cfg(test)]
    pub(crate) fn with_validation_step_hook_for_tests(
        mut self,
        hook: Arc<dyn Fn(usize) + Send + Sync>,
    ) -> Self {
        self.validation_step_hook = Some(hook);
        self
    }

    #[inline]
    fn note_validation_step(&self) {
        #[cfg(test)]
        if let Some(hook) = &self.validation_step_hook {
            hook(self.validation_step.fetch_add(1, Ordering::Relaxed));
        }
    }

    fn population_for(
        &self,
        state: CompletionOverlayState,
    ) -> Option<verter_workspace::ViewPopulation> {
        use verter_workspace::{ViewPopulation, ViewPopulationParent};

        let parent = match self.base.view_population() {
            ViewPopulation::Base => ViewPopulationParent::Base,
            ViewPopulation::SessionOverlay(fingerprint) => {
                ViewPopulationParent::SessionOverlay(fingerprint)
            }
            // A HostStoreView is a durable parent, never another request
            // completion. Refuse if that construction invariant changes.
            ViewPopulation::RequestCompletion(_) => return None,
        };
        ViewPopulation::refined_by_completion(parent, state)
    }

    fn validates_at_completion_state(
        &self,
        fact: &FactVersionRef,
        state: CompletionOverlayState,
    ) -> bool {
        match fact {
            FactVersionRef::FileWholeHash { canonical_id, hash } => {
                if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
                    return &overlay_hash == hash;
                }
                self.base.validates(fact)
            }
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                DerivedFactKind::DirectSource => {
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
            },
            FactVersionRef::Parse(p) => self.validates_parse_domain(p),
            FactVersionRef::ResolveImports(r) => self.validates_resolve_imports_domain(r),
            FactVersionRef::RouteSurface(r) => self.validates_route_surface_domain(r),
            FactVersionRef::ProgramAnalysis(fact) => self.validates_program_analysis_domain(fact),
            FactVersionRef::FileSourceEnv {
                canonical_id,
                parse_env_hash,
                parse_key,
                file_language_id,
            } => self.validates_file_source_env(
                canonical_id,
                *parse_env_hash,
                parse_key,
                file_language_id,
            ),
            FactVersionRef::ProjectGeneration { .. } => self.base.validates(fact),
            FactVersionRef::DomainGeneration(aggregate) => {
                use verter_workspace::CompactionDomain;
                match aggregate.domain {
                    // These populations are independent of per-canonical
                    // completion shadowing.
                    CompactionDomain::WorkspaceShape | CompactionDomain::Resolution => {
                        self.base.validates(fact)
                    }
                    // Empty completion state genuinely is the parent
                    // world: no validation-visible key is shadowed. Once
                    // a completion lands, the id/revision population makes
                    // the base helper compare the identical stamp under a
                    // distinct world rather than refusing the domain
                    // unconditionally.
                    CompactionDomain::Content
                    | CompactionDomain::SourceEnv
                    | CompactionDomain::SemanticImports
                    | CompactionDomain::RouteSurface => {
                        self.population_for(state).is_some_and(|population| {
                            self.base.validates_domain_aggregate_in_population(
                                aggregate, fact, population,
                            )
                        })
                    }
                }
            }
            FactVersionRef::StrictSelfRootWorld(world) => self
                .strict_self_root_world_at_completion_state(state)
                .is_some_and(|current| current == *world),
        }
    }

    fn strict_self_root_world_at_completion_state(
        &self,
        state: CompletionOverlayState,
    ) -> Option<verter_workspace::StrictSelfRootWorld> {
        let mut world = self.base.strict_self_root_world_identity()?;
        world.population = self.population_for(state)?;
        Some(world)
    }

    fn strict_self_root_is_witnessable_at_completion_state(&self, canonical_id: &str) -> bool {
        self.overlay.lookup_whole_hash(canonical_id).is_some()
            || self.base.strict_self_root_is_witnessable(canonical_id)
    }

    fn validates_self_root_at_completion_state(
        &self,
        canonical_id: &str,
        hash: &ResolverHash16,
    ) -> bool {
        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
            return &overlay_hash == hash;
        }
        self.base.validates_self_root_whole_hash(canonical_id, hash)
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

    /// Reuse the base view's captured composites while replacing its
    /// durable population with this request view's exact completion-
    /// overlay refinement. Vouch for nothing when the base was not
    /// proven current.
    ///
    /// The currentness gate is the same fail-closed rule every
    /// `validates*` method below applies: a scope seeded from a stale
    /// base must not detect movement against stamps that base could not
    /// vouch for, and must not compact. The overlay identity is a
    /// population discriminant, not a stamp: empty overlays project to
    /// the parent, shadowing overlays carry their own id and revision,
    /// and an in-flight writer supplies no population.
    #[inline]
    fn aggregate_basis_seed(&self) -> verter_workspace::AggregateBasisSeed {
        if !self.base_is_current {
            return verter_workspace::AggregateBasisSeed::Unvouched;
        }
        let verter_workspace::AggregateBasisSeed::Vouched {
            view_domains,
            semantic_imports,
            route_surface,
            ..
        } = crate::resolver_core::StoreView::aggregate_basis_seed(self.base)
        else {
            return verter_workspace::AggregateBasisSeed::Unvouched;
        };
        verter_workspace::AggregateBasisSeed::Vouched {
            view_population: self.population_for(self.overlay.completion_state()),
            view_domains,
            semantic_imports,
            route_surface,
        }
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
        self.note_validation_step();
        let state = self.overlay.completion_state();
        if state == CompletionOverlayState::InFlight {
            return false;
        }
        self.validates_at_completion_state(fact, state) && self.overlay.completion_state() == state
    }

    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        if sig.is_empty() {
            return Ok(());
        }
        if !self.base_is_current {
            return Err(0);
        }

        let carries_resolution_aggregate = sig.iter().any(|fact| {
            matches!(
                fact.attribution(),
                verter_workspace::FactAttribution::DomainAggregate(
                    verter_workspace::CompactionDomain::Resolution
                )
            )
        });
        if carries_resolution_aggregate {
            if let Some(index) = sig.iter().position(|fact| {
                matches!(
                    fact.attribution(),
                    verter_workspace::FactAttribution::DomainAggregate(
                        verter_workspace::CompactionDomain::Content
                            | verter_workspace::CompactionDomain::SourceEnv
                            | verter_workspace::CompactionDomain::SemanticImports
                            | verter_workspace::CompactionDomain::RouteSurface
                    )
                )
            }) {
                return Err(index);
            }
        }

        let state = self.overlay.completion_state();
        if state == CompletionOverlayState::InFlight {
            return Err(0);
        }

        for (index, fact) in sig.iter().enumerate() {
            self.note_validation_step();
            let valid = match fact {
                FactVersionRef::FileWholeHash { canonical_id, hash }
                    if self_root_canonicals.contains(&canonical_id.as_str()) =>
                {
                    self.validates_self_root_at_completion_state(canonical_id, hash)
                }
                other => self.validates_at_completion_state(other, state),
            };
            if !valid {
                return Err(index);
            }
        }

        if self.overlay.completion_state() != state {
            return Err(0);
        }
        Ok(())
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
        // view's immutable-root point lookup.
        //
        // Without this override, per-rejection attribution helpers
        // (`attribute_prepared_decl_bundle_rejection`) call this on a
        // `RequestStoreView` and hit the default `None` arm — every real
        // route-hash mismatch then reclassifies as `_absent` and the
        // discriminating counter loses its meaning.
        if let Some(overlay_hash) = self.overlay.lookup_derived_hash(canonical_id, kind) {
            return Some(overlay_hash);
        }
        self.base.derived_hash_for(canonical_id, kind)
    }

    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        if !self.base_is_current {
            return false;
        }
        let state = self.overlay.completion_state();
        if state == CompletionOverlayState::InFlight {
            return false;
        }
        self.validates_self_root_at_completion_state(canonical_id, hash)
            && self.overlay.completion_state() == state
    }

    fn strict_self_root_world_identity(&self) -> Option<verter_workspace::StrictSelfRootWorld> {
        if !self.base_is_current {
            return None;
        }
        let state = self.overlay.completion_state();
        if state == CompletionOverlayState::InFlight {
            return None;
        }
        let world = self.strict_self_root_world_at_completion_state(state)?;
        (self.overlay.completion_state() == state).then_some(world)
    }

    fn strict_self_root_is_witnessable(&self, canonical_id: &str) -> bool {
        self.strict_self_root_is_witnessable_at_completion_state(canonical_id)
    }

    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<verter_workspace::StrictSelfRootWorld> {
        if !self.base_is_current {
            return None;
        }
        let state = self.overlay.completion_state();
        if state == CompletionOverlayState::InFlight {
            return None;
        }
        let before = self.strict_self_root_world_at_completion_state(state)?;
        if !roots.iter().all(|(canonical, hash)| {
            self.strict_self_root_is_witnessable_at_completion_state(canonical)
                && self.validates_self_root_at_completion_state(canonical, hash)
        }) {
            return None;
        }
        let after = self.strict_self_root_world_at_completion_state(state)?;
        (before == after && self.overlay.completion_state() == state).then_some(before)
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
        parse_key: &verter_language::ParseKey,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        if !self.base_is_current {
            return false;
        }
        self.base.validates_file_source_env(
            canonical_id,
            parse_env_hash,
            parse_key,
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
        // Resolution-currency facts validate against the base view's
        // CAPTURED resolution world. The per-request completion overlay
        // carries promoted parse/content state, never resolver
        // observations, so it neither shadows nor re-roots this arm — and
        // the overlay-keyed detour below would misroute a fact whose entry
        // is an explicit project (no canonical id at all) into a blanket
        // rejection.
        if fact.resolution_fact().is_some() {
            return self.base.validates_resolve_imports_domain(fact);
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
        let Some(canonical_id) = fact.canonical_id() else {
            return false;
        };
        if let Some(overlay_hash) = self.overlay.lookup_whole_hash(canonical_id) {
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

    fn validates_program_analysis_domain(
        &self,
        fact: &crate::resolver_core::ProgramAnalysisFactRef,
    ) -> bool {
        if !self.base_is_current {
            return false;
        }
        // The `FunctionProgramIndex` authority is the base view's
        // per-canonical snapshot: an overlay-only function body has no
        // base artifact to validate against, so an overlay-recorded
        // ProgramAnalysis fact can never validate against the base
        // (overlay results never populate base-only caches).
        self.base.validates_program_analysis_domain(fact)
    }

    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        route_hash: Option<Hash16>,
    ) {
        // Route the call through the overlay's
        // `complete_route_canonical` writer — it writes
        // `whole_hashes` + `derived_hashes` entries with the same
        // presence-before-insert ordering as the standard completion
        // path. The epoch guard lives at
        // the producer-side call site (where the concrete host is
        // available) so this trait method stays off the
        // `VerterHost` type and the resolver-context seal
        // (`no_concrete_verter_host_in_seal_scope` architecture
        // guard) keeps holding.
        self.overlay
            .complete_route_canonical(canonical, whole_hash, route_hash);
    }
}
