//! Dependency-neutral store-view identity/comparison value types.
//!
//! Only dependency-neutral comparison and identity types live here.
//! `HostStoreView` — the live view over `verter_session`'s
//! cache-storage roots (`ProjectTypeStore`, `FileArtifactRoot`,
//! `WorkspaceAccess`, and friends) — stays in `verter_session` alongside
//! `StoreViewManager` and the rest of the cache-retention machinery.
//!
//! `StoreViewValidationToken::capture(host)` does NOT move — Rust's orphan
//! rule forbids a downstream crate from adding inherent `impl` blocks to a
//! type it does not define, and `capture` needs `&VerterHost`, which this
//! crate cannot name. `verter_session::resolver_store` keeps a free
//! function (`capture_store_view_validation_token`) that reads the host and
//! constructs this crate's [`StoreViewValidationToken`] instead.

use crate::analysis::types::Hash16;

/// Dependency-neutral mirror of `verter_session::file_artifact_store::
/// ProjectIdentity` (a `Hash16` newtype: the workspace + tsconfig +
/// provider-root discriminator), scoped to this token. Deliberately a
/// separate type from the session cache-key dimension: `
/// ProjectIdentity` is a pervasive cache-key dimension used far beyond the
/// store-view token (R21). `verter_session` converts at the one point it
/// constructs a `StoreViewValidationToken`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewProjectIdentity(pub Hash16);

/// Frozen identity of a session overlay folded into a
/// [`StoreViewValidationToken`]. Distinguishes a base view from a
/// session-overlaid one and distinguishes two sessions whose overlay
/// shapes differ. Request-completion identity lives on the fact-signature
/// population rail, not here.
///
/// Named distinctly from `verter_session::cache_runtime::world_snapshot::
/// OverlayIdentity` (an unrelated type); `verter_session::resolver_store`
/// re-exports this as `OverlayIdentity` within its own module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewOverlayIdentity {
    /// Raw session id (session-view-scoped).
    pub session_id: Option<u64>,
    /// Structural fold of the overlay's masked canonicals (count +
    /// per-canonical content hashes XOR-folded). Any change to the set
    /// of overlaid/tombstoned canonicals — or their content — changes
    /// this fold.
    pub overlay_fingerprint: Hash16,
}

/// Complete validity oracle for a store-view snapshot.
///
/// The token is the SOLE signal `verter_session`'s `StoreViewManager` uses
/// to decide whether a cached base view is still safe to hand back, and the
/// SOLE signal the publish fence rechecks before promoting a cold result.
/// Two tokens compare equal iff every validation-affecting by-value
/// dimension of the store view is identical.
///
/// ## Why this set is COMPLETE (the soundness argument)
///
/// A `HostStoreView` caches two classes of state:
///
/// 1. **By-value snapshots** captured at build time — `whole_hashes`,
///    `derived_hashes`, `file_facts`, `route_surface_index_fingerprints`,
///    env hashes, project identity, project generation, and the session
///    tombstone/overlay deltas. A stale by-value snapshot would
///    mis-validate, so the token MUST advance whenever any of these can
///    change. Every host mutation that alters one of these advances
///    `VerterHost::store_view_epoch` (source/content, evict, reload,
///    `clear_compile_cache`, `close`, `set_import_dependencies`, scheduler
///    node membership) and/or `ProjectTypeStore::project_generation`
///    (project-shape / config / env / identity changes route through
///    `bump_project_generation_and_evict`). The env-hash fold + project
///    identity are folded in directly so the oracle is self-contained even
///    if a future workspace mutator changed env without bumping a
///    generation.
///
/// 2. **By-live-Arc-handle** dimensions — the `resolved_import_facts`
///    `Arc<ResolvedImportFactsDb>` and the `route_db` `Arc<RouteDb>`
///    handles. Both stay OUT of the token, but for two DIFFERENT reasons:
///    - `ResolvedImportFactsDb` is content-addressed: its key includes
///      `content_hash`, so a new content version is a NEW key and a fixed
///      handle reads a correct value without a rebuild (immutable-by-key).
///    - `RouteDb` is NOT content-addressed — its keys carry no content
///      hash, and evict/clear/replace reuse the same key. It stays out of
///      the token because every value it hands out is validated per
///      candidate against THIS view through the candidate's own recorded
///      `fact_dep_signature`: an evicted/replaced entry yields a
///      conservative fail-closed MISS (the consumer recomputes through the
///      cold path), never a stale positive. The token therefore does not
///      need a `RouteDb` generation to stay sound — the per-candidate
///      signature comparison IS the validity rail. Note the route-surface
///      FACT domain does not read `RouteDb` at all: its sole arm
///      (`StoreView::validates_route_surface_domain`) compares the
///      consumer's recorded `expected_hash` against the augmentation-index
///      fingerprint snapshot captured on this view's artifact root.
///
/// Additive lazy loads observed mid-request (a dependency
/// `FileArtifactStore` publication that lands AFTER the snapshot was built
/// and does NOT bump the epoch) are NOT a soundness hole: for an untracked
/// canonical the snapshot stays untracked → the request-scoped
/// `CanonicalCompletionOverlay` shadows it; for a tracked canonical the
/// content change already advanced the epoch.
///
/// `store_view_epoch` is an INPUT to the token, never the oracle by itself
/// — the token (epoch + generation + env fold + identity + overlay
/// identity) is the oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreViewValidationToken {
    /// Coarse semantic-mutation epoch (`VerterHost::current_store_view_epoch`).
    /// Advances on every host mutation that can change a by-value snapshot
    /// dimension.
    pub store_view_epoch: u64,
    /// Project generation (`ProjectTypeStore::project_generation`).
    /// Advances on project-shape / config / env / identity changes via
    /// `bump_project_generation_and_evict`.
    pub project_generation: u64,
    /// Indexed-artifact publication generation
    /// (`FileArtifactStore::artifact_generation`). Advances on every
    /// artifact insert / replace / evict / GC and augmentation-index
    /// mutation. This covers the BY-VALUE snapshot dimensions
    /// (`file_facts`, `derived_hashes`, `route_surface_index_fingerprints`)
    /// that a lazy `ensure_indexed_ready_serve` publication changes
    /// WITHOUT bumping `store_view_epoch` — without it a manager-cached
    /// base view would go stale after a lazy publication and warm-hit
    /// validation would false-miss (a steady-state warm-cache regression).
    /// The lazy-publication burst during a cold compute is bounded, so the
    /// cache rebuilds once then stays warm.
    pub artifact_generation: u64,
    /// Additive derived-state generation (`VerterHost::current_load_generation`).
    /// Advances on additive `derived_raw_cache` mutations the base view
    /// snapshots BY VALUE but that do NOT publish into `FileArtifactStore`
    /// (so `artifact_generation` does not cover them) and are NOT a
    /// content/project/env mutation (so `store_view_epoch` does not cover
    /// them). Two producers advance it:
    ///
    /// * a successful first-time `ensure_loaded` — a load that adds a
    ///   scheduler node + `derived_raw_cache` state (`whole_hashes`
    ///   membership / known-miss tags);
    /// * a resolved dependency-EDGE registration
    ///   (`VerterHost::record_resolved_dependency_edge`) — which writes
    ///   `DependencyState.dependencies`, the reverse-dependency
    ///   bookkeeping.
    ///
    /// Included in the `StoreViewManager` REUSE oracle (either mutation
    /// invalidates the cached base view) but EXCLUDED from
    /// [`Self::externally_superseded_by`] — a cold compute's OWN
    /// dependency loads / route resolutions are its own work, not an
    /// external mutation, so they must not self-fence result promotion
    /// (same treatment as `artifact_generation`).
    pub load_generation: u64,
    /// Workspace content/file-set generation
    /// (`WorkspaceAccess::content_generation`). Advances on every file-set
    /// mutation the workspace observes — inject / delete / overlay batch
    /// application, an OS-watcher recovery (`DirectoryTreeDirty`), and a
    /// resolve-extension change — WITHOUT any host-side epoch or
    /// generation necessarily moving (no `verter_session` handler observes
    /// `DirectoryTreeDirty`).
    ///
    /// A file-set mutation can supersede the captured source and
    /// resolution roots even while an owner's bytes stay unchanged. A
    /// cached view must therefore MISS once this advances; path-precise
    /// resolution validity itself remains owned by the captured resolution
    /// world and its facts.
    ///
    /// Included in BOTH the `StoreViewManager` REUSE oracle and
    /// [`Self::externally_superseded_by`]: unlike the two additive
    /// generations above, a cold compute's OWN work (loads,
    /// `ensure_indexed_ready_serve`, store-view builds) NEVER advances it —
    /// only a real external file-set mutation does — so folding it into the
    /// supersession fingerprint cannot self-fence promotion.
    pub content_generation: u64,
    /// Dedicated strict-self-root authority. It participates in manager
    /// reuse so a live trackedness transition rebuilds the sealed roots,
    /// but stays outside external supersession because a cold compute may
    /// create its own derived-state membership.
    pub strict_self_root_generation: Option<u64>,
    /// Monotonic count of resolution FACT VERSIONS the workspace's
    /// resolution world has minted
    /// (`WorkspaceRead::resolution_fact_generation`).
    ///
    /// The snapshot RETAINS an `Arc<CapturedResolutionWorld>` and answers
    /// every `ResolveImportsFactRef::Resolution` validation out of that
    /// frozen capture. Without this dimension a manager-cached view could
    /// be reused indefinitely across a resolution-visible mutation that
    /// moved no other dimension — the reader-driven evidence refresh and
    /// the observed-value fold both advance fact versions inside the
    /// resolution-world write gate without touching content, project,
    /// artifact, load, env or overlay state — and it would keep validating
    /// witnesses those advances had just invalidated.
    ///
    /// This is NOT world identity and NOT a validity oracle: validity
    /// stays fact-precise inside the captured world. The counter decides
    /// only whether the RETAINED capture is still the right snapshot to
    /// answer from. A first-observation baseline fill mints no version, so
    /// a cold compute's own discovery does not churn it.
    ///
    /// Included in BOTH the `StoreViewManager` REUSE oracle and
    /// [`Self::externally_superseded_by`], and so in the lane identity and
    /// the compat token that derive from it. Every mint is an EXTERNAL
    /// change entering the world, never a compute's own discovery:
    ///
    /// * a first-observation baseline fill mints NOTHING — both the
    ///   observed-value fold and the reader-driven evidence refresh record
    ///   an unseen value without advancing a version, which is exactly the
    ///   "own work" case `artifact_generation` / `load_generation` are
    ///   excluded for;
    /// * the fold advances a version only on a CONFLICT with the recorded
    ///   baseline — state newer than the captured root;
    /// * the evidence refresh advances one only when a re-read value
    ///   MOVED;
    /// * exact-table, project-publication and mutation-protocol advances
    ///   are external by construction.
    ///
    /// So the criterion stated for `content_generation` above holds here
    /// too, and excluding it would leave the fence, the lane and the compat
    /// token blind to an exact retarget: `set_exact_resolutions` moves this
    /// dimension and no other, so two views straddling a retarget would
    /// fold to the same `u64`, a leader would promote a pre-retarget
    /// result, and the request-scoped bundle memo would re-serve
    /// pre-retarget edges to the very stability retry that exists to
    /// escape them.
    pub resolution_fact_generation: u64,
    /// Folded env-hash bundle (R21). Self-contained defence: even if a
    /// future workspace mutator changed env without bumping a generation,
    /// the fold would still distinguish the views.
    pub env_hash_fold: Hash16,
    /// Workspace-default project identity (R21).
    pub project_identity: StoreViewProjectIdentity,
    /// Frozen session-overlay identity.
    ///
    /// `None` for a base view. `Some(_)` carries the session id plus the
    /// structural overlay fold. Request-completion overlays do not enter
    /// this token: they can advance within one request and are identified
    /// separately by `ViewPopulation::RequestCompletion` in fact
    /// signatures. `RequestStoreView::compat_token` intentionally remains
    /// the durable base/session coalescing identity.
    pub overlay_identity: Option<StoreViewOverlayIdentity>,
}

impl StoreViewValidationToken {
    /// Constructs a token from its raw dimensions. The sole construction
    /// entry point available outside this crate. `verter_session` reads
    /// `&VerterHost` and calls this from its own
    /// `capture_store_view_validation_token` free function because this crate
    /// cannot name `VerterHost`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store_view_epoch: u64,
        project_generation: u64,
        artifact_generation: u64,
        load_generation: u64,
        content_generation: u64,
        strict_self_root_generation: Option<u64>,
        resolution_fact_generation: u64,
        env_hash_fold: Hash16,
        project_identity: StoreViewProjectIdentity,
        overlay_identity: Option<StoreViewOverlayIdentity>,
    ) -> Self {
        Self {
            store_view_epoch,
            project_generation,
            artifact_generation,
            load_generation,
            content_generation,
            strict_self_root_generation,
            resolution_fact_generation,
            env_hash_fold,
            project_identity,
            overlay_identity,
        }
    }

    /// Returns a copy of `self` with `overlay_identity` replaced — the
    /// external-supersession comparison at `verter_session`'s
    /// `CanonicalCompletionOverlay::complete_canonical_inner` normalises a
    /// session-overlaid base view's token back to `None` before comparing,
    /// since a request's frozen overlay is not itself an external
    /// mutation.
    #[must_use]
    pub const fn with_overlay_identity(
        mut self,
        overlay_identity: Option<StoreViewOverlayIdentity>,
    ) -> Self {
        self.overlay_identity = overlay_identity;
        self
    }

    /// Whether `self` was SUPERSEDED by an EXTERNAL mutation relative to
    /// `later` — i.e. a `store_view_epoch` / `project_generation` /
    /// `content_generation` / `resolution_fact_generation` / env / identity
    /// change happened between the two captures.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation`: a
    /// cold compute legitimately publishes `IndexedReady` artifacts AND
    /// loads its dependencies (advancing those generations) as part of its
    /// own work. The publish fence must NOT treat the compute's OWN
    /// artifact publications or dependency loads as a supersession — only
    /// an external content/project/env/identity mutation invalidates the
    /// snapshot the result was produced against. (Those two generations
    /// remain in the full token for the `StoreViewManager` REUSE oracle,
    /// where a post-build publication / load SHOULD trigger a rebuild on
    /// the next request.)
    #[must_use]
    pub fn externally_superseded_by(&self, later: &Self) -> bool {
        self.store_view_epoch != later.store_view_epoch
            || self.project_generation != later.project_generation
            || self.content_generation != later.content_generation
            || self.resolution_fact_generation != later.resolution_fact_generation
            || self.env_hash_fold != later.env_hash_fold
            || self.project_identity != later.project_identity
            || self.overlay_identity != later.overlay_identity
    }

    /// A `u64` fingerprint folding ONLY the EXTERNAL-supersession
    /// dimensions ([`Self::externally_superseded_by`]: `store_view_epoch`,
    /// `project_generation`, `content_generation`,
    /// `resolution_fact_generation`, `env_hash_fold`, `project_identity`,
    /// `overlay_identity`).
    ///
    /// Two tokens fold to the same value iff neither externally supersedes
    /// the other (up to hash collision); they fold to different values iff
    /// one externally superseded the other. This is the seal-respecting
    /// `u64` the resolver-tier request executors compare to gate stable
    /// promotion: a snapshot whose external fingerprint no longer matches
    /// the live host fingerprint was externally superseded mid-compute (an
    /// epoch / project / env / identity / overlay change — e.g. a
    /// `set_default_resolve_extensions` env-hash shift that moves NO
    /// epoch) and its result MUST NOT be promoted to the shared cache.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation` for
    /// the SAME reason [`Self::externally_superseded_by`] does: a cold
    /// compute advances those generations as its OWN work (publishing
    /// artifacts, loading its dependencies, admitting its own routes), and
    /// folding them here would make the executor self-fence its own
    /// promotion.
    #[must_use]
    pub fn external_supersession_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        self.store_view_epoch.hash(&mut hasher);
        self.project_generation.hash(&mut hasher);
        self.content_generation.hash(&mut hasher);
        self.resolution_fact_generation.hash(&mut hasher);
        self.env_hash_fold.hash(&mut hasher);
        self.project_identity.hash(&mut hasher);
        self.overlay_identity.hash(&mut hasher);
        hasher.finish()
    }

    /// A `u64` fingerprint for the singleflight / stability coalescing-lane
    /// identity (`StoreViewCompatToken::validity_fingerprint`).
    ///
    /// Folds the EXTERNAL-supersession dimensions ONLY (`store_view_epoch`,
    /// `project_generation`, `content_generation`,
    /// `resolution_fact_generation`, `env_hash_fold`, `project_identity`,
    /// `overlay_identity`) — identical to
    /// [`Self::external_supersession_fingerprint`]. This is the SAME oracle
    /// the request executors' promotion fence (`is_stable`) compares, and it
    /// MUST be: the coalescing lane hands a LEADER's stable result to
    /// FOLLOWERS without per-follower revalidation, and the leader only
    /// promotes a result as `stable` when its snapshot's external
    /// fingerprint still matches the live host fingerprint. Two requests
    /// that share an external-supersession lane are therefore
    /// validation-equivalent for the promoted result: the leader's result
    /// is admissible exactly when the external dimensions are coherent, and
    /// a follower on the same lane shares those dimensions.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation` for
    /// the SAME reason [`Self::external_supersession_fingerprint`] does: a
    /// cold compute advances those generations as its OWN work (publishing
    /// `IndexedReady` artifacts, loading its dependencies), so two
    /// concurrent identical cold requests that snapshot at slightly
    /// different points in the load sweep observe DIFFERENT additive
    /// generations. Folding them into the lane identity would split those
    /// identical requests across distinct lanes and spawn multiple cold
    /// winners instead of one leader + N-1 dedup-joining followers — the
    /// exact self-fencing the promotion oracle already avoids.
    #[must_use]
    pub fn lane_fingerprint(&self) -> u64 {
        self.external_supersession_fingerprint()
    }
}

#[cfg(test)]
#[path = "store_view_identity_tests.rs"]
mod store_view_identity_tests;
