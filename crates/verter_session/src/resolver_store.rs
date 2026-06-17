use crate::types::Hash16;
use crate::VerterHost;
use dashmap::DashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Per-call-site counter for [`HostStoreView::from_host_read`] invocations.
///
/// **Per-call-site instrumentation.** `HostStoreView::from_host` rebuilds
/// the entire workspace snapshot on every call; the dominant cost
/// surfaces as `host_store_view_from_host_builds` per-Button counts in
/// the audit-record diagnostic counters. To attribute those builds
/// back to specific warm-hit validator call sites (the Bug 2 hypothesis
/// the 3-way consult identified), every entry into `from_host`
/// records `std::panic::Location::caller()` and bumps a per-site
/// counter. The `#[track_caller]` rail on `from_host`,
/// `VerterHost::resolver_store_view`, the
/// `impl ResolverContext::resolver_store_view` trait impls, and the
/// `fact_signature_helpers::validate_fact_signature*` helpers
/// propagates the location all the way back to the warm-hit cache
/// validator that triggered the build — so the dump attributes builds
/// to the actual cache layer paying for them, not to the deepest
/// `from_host` body call site.
///
/// **Cost is negligible:** each call performs one `DashMap` lookup
/// (sub-µs) vs the multi-ms workspace sweep `from_host` itself does,
/// so the counter stays production-on. The map is keyed by
/// `&'static Location<'static>` — `track_caller` locations are
/// `'static` by language guarantee, so pointer identity is stable and
/// the key set is bounded by the number of distinct call sites in the
/// linked binary.
///
/// Read via [`dump_from_host_call_sites`] (sorted descending by count).
static FROM_HOST_BY_SITE: OnceLock<DashMap<&'static Location<'static>, AtomicU64>> =
    OnceLock::new();

#[inline]
fn from_host_site_table() -> &'static DashMap<&'static Location<'static>, AtomicU64> {
    FROM_HOST_BY_SITE.get_or_init(DashMap::new)
}

/// Process-wide count of ACTUAL base-view sweeps — incremented once per
/// `HostStoreView::build_coherent` entry, NOT per `from_host` call.
///
/// Distinct from [`FROM_HOST_BY_SITE`] / the per-request
/// `host_store_view_from_host_builds` diagnostic, which count every
/// `from_host` call INCLUDING the cheap token-stable
/// `Arc<StoreViewSnapshot>`-clone hits that the [`StoreViewManager`]
/// serves without sweeping. A batch-saturation gate keys off THIS
/// counter (full-workspace sweeps) to assert that a warm batch performs
/// ~O(1) sweeps rather than O(N) — the call-count counter cannot make
/// that distinction because a manager hit and a manager miss both bump
/// it.
static STORE_VIEW_COHERENT_BUILD_SWEEPS: AtomicU64 = AtomicU64::new(0);

/// Number of actual full-workspace base-view sweeps performed since the
/// last [`reset_store_view_coherent_build_sweeps`]. A batch-saturation
/// gate reads this to verify the [`StoreViewManager`] collapses a warm
/// batch onto ~O(1) sweeps.
#[must_use]
pub fn store_view_coherent_build_sweeps() -> u64 {
    STORE_VIEW_COHERENT_BUILD_SWEEPS.load(Ordering::Relaxed)
}

/// Reset the actual-sweep counter — for tests / benches that want a
/// clean delta around a batch.
pub fn reset_store_view_coherent_build_sweeps() {
    STORE_VIEW_COHERENT_BUILD_SWEEPS.store(0, Ordering::Relaxed);
}

/// Test-only: live count of ENROLLED threads currently inside
/// `build_coherent`'s full-workspace sweep region. Incremented on entry,
/// decremented on exit — but ONLY for threads that opted in via
/// [`enroll_concurrent_sweep_gauge`]. Enrollment isolates the gauge from
/// unrelated parallel store-view tests (whose sweeper threads never enroll),
/// so the singleflight-claim regression reads a PEAK
/// ([`STORE_VIEW_PEAK_CONCURRENT_SWEEPS`]) that reflects ONLY its own
/// threads' concurrency.
#[cfg(test)]
static STORE_VIEW_LIVE_CONCURRENT_SWEEPS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Test-only: the maximum observed value of
/// [`STORE_VIEW_LIVE_CONCURRENT_SWEEPS`] since the last
/// [`reset_store_view_peak_concurrent_sweeps`]. With the singleflight
/// claim held across every build this is `1` (or `0` if no build ran); an
/// UNCLAIMED parallel sweep under contention drives it `> 1`.
#[cfg(test)]
static STORE_VIEW_PEAK_CONCURRENT_SWEEPS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    /// Test-only: `true` on a thread that opted into the concurrent-sweep
    /// gauge via [`enroll_concurrent_sweep_gauge`]. Only such a thread
    /// participates in the live/peak counters and the overlap hold, so an
    /// unrelated parallel store-view test's sweeper threads never perturb
    /// the gauge.
    static CONCURRENT_SWEEP_GAUGE_PARTICIPANT: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

/// Test-only: opt the CURRENT thread into the concurrent-sweep gauge (and the
/// overlap hold). The final-fallback singleflight regression calls this on
/// each of its OWN spawned sweeper threads so the global peak counts only
/// their concurrency, not unrelated parallel tests'.
#[cfg(test)]
pub(crate) fn enroll_concurrent_sweep_gauge() {
    CONCURRENT_SWEEP_GAUGE_PARTICIPANT.with(|c| c.set(true));
}

/// Test-only: the peak number of concurrent full-workspace sweeps observed
/// since the last reset. The final-fallback singleflight regression asserts
/// this stays `<= 1` (no parallel unclaimed sweeps under churn + contention).
#[cfg(test)]
pub(crate) fn store_view_peak_concurrent_sweeps() -> u64 {
    STORE_VIEW_PEAK_CONCURRENT_SWEEPS.load(Ordering::Relaxed)
}

#[cfg(test)]
thread_local! {
    /// Test-only PER-THREAD counter bumped each time a compile-path
    /// warm-validation site actually READS the base store view
    /// (`resolver_store_view_read`) to gate a warm hit. Shared by all
    /// three compile warm-validation sites (`ensure_compiled`,
    /// `compile_slot_is_warm`, the `get_virtual_file` Session arm): each
    /// threads the read through the `acquire_view` callback that
    /// [`crate::cache_runtime::CompileOutputNodeFactValidatedSession::lookup`]
    /// invokes ONLY after its cheap slot-present + carrier + hash
    /// predicates pass, and bumps this counter inside that callback.
    ///
    /// Thread-local so a single synchronous warm-validation call on the
    /// calling thread can snapshot it — immune to parallel-test
    /// contamination of any process-global table AND to source line
    /// shifts. A miss on the cheap predicates (no slot for the profile,
    /// overflowed carrier, or hash mismatch) never reaches `acquire_view`,
    /// so this counter stays flat; an eager read before the cheap checks
    /// would bump it once even on such a miss.
    pub(crate) static COMPILE_WARM_VALIDATION_VIEW_READS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
}

/// Test-only: bump the per-thread compile-path warm-validation store-view
/// read counter. Called from inside each warm-validation site's
/// `acquire_view` callback, so the count reflects reads that actually
/// happened after the cheap predicates passed.
#[cfg(test)]
pub(crate) fn record_compile_warm_validation_view_read() {
    COMPILE_WARM_VALIDATION_VIEW_READS.with(|c| c.set(c.get().saturating_add(1)));
}

/// Test-only: read the per-thread compile-path warm-validation store-view
/// read counter.
#[cfg(test)]
pub(crate) fn compile_warm_validation_view_reads() -> u64 {
    COMPILE_WARM_VALIDATION_VIEW_READS.with(std::cell::Cell::get)
}

/// Test-only: reset the per-thread compile-path warm-validation store-view
/// read counter.
#[cfg(test)]
pub(crate) fn reset_compile_warm_validation_view_reads() {
    COMPILE_WARM_VALIDATION_VIEW_READS.with(|c| c.set(0));
}

/// Test-only: reset the peak-concurrent-sweep gauge before a scenario.
#[cfg(test)]
pub(crate) fn reset_store_view_peak_concurrent_sweeps() {
    STORE_VIEW_LIVE_CONCURRENT_SWEEPS.store(0, Ordering::Relaxed);
    STORE_VIEW_PEAK_CONCURRENT_SWEEPS.store(0, Ordering::Relaxed);
}

/// Test-only: when `true`, each `build_coherent` sweep holds a short fixed
/// delay inside the concurrent-sweep gauge window. The delay widens the
/// window so genuinely-parallel UNCLAIMED sweeps reliably overlap and the
/// peak gauge observes them, without depending on incidental timing. Under
/// the singleflight claim only ONE sweep is ever live, so the delay merely
/// serialises (the peak stays 1); without the claim several final-fallback
/// sweeps overlap and the peak rises above 1.
#[cfg(test)]
static STORE_VIEW_SWEEP_OVERLAP_HOLD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Test-only: arm/disarm the per-sweep overlap hold.
#[cfg(test)]
pub(crate) fn arm_store_view_sweep_overlap_hold(armed: bool) {
    STORE_VIEW_SWEEP_OVERLAP_HOLD.store(armed, Ordering::SeqCst);
}

/// Test-only RAII guard that bumps the live concurrent-sweep gauge on
/// construction (updating the peak) and drops it on `Drop` — but ONLY on an
/// ENROLLED thread ([`enroll_concurrent_sweep_gauge`]). Armed for the
/// duration of each `build_coherent` full-workspace sweep so a parallel
/// UNCLAIMED sweep across enrolled threads is observable as a peak `> 1`.
/// When the overlap hold is armed, an enrolled `enter` sleeps briefly to
/// widen the overlap window. A non-enrolled thread (an unrelated parallel
/// store-view test's sweeper) is a no-op, so it never perturbs the gauge.
#[cfg(test)]
struct ConcurrentSweepGauge {
    enrolled: bool,
}

#[cfg(test)]
impl ConcurrentSweepGauge {
    fn enter() -> Self {
        let enrolled = CONCURRENT_SWEEP_GAUGE_PARTICIPANT.with(std::cell::Cell::get);
        if !enrolled {
            return ConcurrentSweepGauge { enrolled };
        }
        let live = STORE_VIEW_LIVE_CONCURRENT_SWEEPS.fetch_add(1, Ordering::SeqCst) + 1;
        STORE_VIEW_PEAK_CONCURRENT_SWEEPS.fetch_max(live, Ordering::SeqCst);
        if STORE_VIEW_SWEEP_OVERLAP_HOLD.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            // Re-record the peak after any overlap accumulated during the hold.
            let live_now = STORE_VIEW_LIVE_CONCURRENT_SWEEPS.load(Ordering::SeqCst);
            STORE_VIEW_PEAK_CONCURRENT_SWEEPS.fetch_max(live_now, Ordering::SeqCst);
        }
        ConcurrentSweepGauge { enrolled }
    }
}

#[cfg(test)]
impl Drop for ConcurrentSweepGauge {
    fn drop(&mut self) {
        if self.enrolled {
            STORE_VIEW_LIVE_CONCURRENT_SWEEPS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Test-only recorder for the SPECIFIC snapshot the reset fence (Gate 1)
/// declined. Cross-thread (a gated builder thread declines; the test driver
/// thread reads), so it is a process-global `Mutex`, not a thread-local.
/// The reset-fence regression arms / clears it explicitly; production never
/// touches it.
#[cfg(test)]
static RESET_DECLINED_SNAPSHOT_WEAK: parking_lot::Mutex<
    Option<std::sync::Weak<StoreViewSnapshot>>,
> = parking_lot::Mutex::new(None);

/// Test-only flag: `true` once a Gate 1 reset-fence decline has fired since
/// the last [`clear_reset_declined_snapshot_for_tests`]. The recorded `Weak`
/// alone is unreliable (the declined snapshot is dropped once the bounded
/// re-loop publishes a fresh one), so this flag is the durable "Gate 1
/// fired" signal.
#[cfg(test)]
static RESET_FENCE_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Test-only: record the pre-reset snapshot a Gate 1 decline rejected.
#[cfg(test)]
fn record_reset_declined_snapshot_for_tests(view: &HostStoreView) {
    *RESET_DECLINED_SNAPSHOT_WEAK.lock() = Some(Arc::downgrade(&view.snapshot));
    RESET_FENCE_FIRED.store(true, Ordering::Relaxed);
}

/// Test-only: clear the recorded reset-declined snapshot + fired flag before
/// a test arms the scenario, so a stale recording from a prior test cannot
/// leak in.
#[cfg(test)]
pub(crate) fn clear_reset_declined_snapshot_for_tests() {
    *RESET_DECLINED_SNAPSHOT_WEAK.lock() = None;
    RESET_FENCE_FIRED.store(false, Ordering::Relaxed);
}

/// Test-only: whether a Gate 1 reset-fence decline has fired.
#[cfg(test)]
pub(crate) fn reset_fence_fired_for_tests() -> bool {
    RESET_FENCE_FIRED.load(Ordering::Relaxed)
}

/// Test-only: the snapshot the reset fence most recently declined, if it is
/// still alive. The reset-fence regression asserts this exact `Arc` is never
/// the manager's cached snapshot.
#[cfg(test)]
pub(crate) fn reset_declined_snapshot_for_tests() -> Option<std::sync::Arc<StoreViewSnapshot>> {
    RESET_DECLINED_SNAPSHOT_WEAK
        .lock()
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
}

/// Record one entry into [`HostStoreView::from_host_read`] under the
/// `#[track_caller]`-propagated call site. Bumped on every call;
/// thread-safe; no allocation when the site already has an entry
/// (the common case after the first call from each site).
#[inline]
fn record_from_host_call(loc: &'static Location<'static>) {
    let table = from_host_site_table();
    if let Some(entry) = table.get(loc) {
        entry.fetch_add(1, Ordering::Relaxed);
        return;
    }
    // First call from this site — insert a fresh counter at 1. Two
    // racing first-calls may both take the insert arm; the second's
    // entry overwrites the first's 1-count with another 1-count, which
    // is acceptable for diagnostic accounting (lost at most ~N
    // first-call counts where N = number of racing threads at startup).
    table.insert(loc, AtomicU64::new(1));
}

/// Reset the per-call-site counter table — only useful for tests / benches
/// that want a clean delta. Production callers never invoke this; the
/// table accumulates across the process lifetime.
pub fn reset_from_host_call_sites() {
    from_host_site_table().clear();
}

/// Snapshot the per-call-site counter table, sorted by count descending.
/// Each tuple is `(file_line, call_count)` where `file_line` is the
/// canonical `file:line:col` `Location` debug string.
///
/// **Diagnostic accessor.** The bench example dumps this at
/// the end of each pass to attribute `HostStoreView::from_host` builds
/// to specific warm-hit validator call sites. The `#[track_caller]`
/// rail on `from_host`, `VerterHost::resolver_store_view`, the trait
/// `resolver_store_view` impls, and the `validate_fact_signature*`
/// helpers reflects the location back to the cache layer triggering
/// the build.
#[must_use]
pub fn dump_from_host_call_sites() -> Vec<(String, u64)> {
    let table = from_host_site_table();
    let mut rows: Vec<(String, u64)> = table
        .iter()
        .map(|entry| {
            let loc = *entry.key();
            let count = entry.value().load(Ordering::Relaxed);
            let formatted = format!("{}:{}:{}", loc.file(), loc.line(), loc.column());
            (formatted, count)
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));
    rows
}

/// Per-request component-meta store counters captured by
/// [`VerterHost::component_meta_audit_store_snapshot`]. The fields
/// live on [`crate::component_meta_audit::ComponentMetaPayload`]
/// rather than the generic
/// [`crate::component_meta_audit::RequestStoreAudit`] envelope; this
/// struct is the cross-call carrier between the snapshot site and
/// the audit-builder finalisation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ComponentMetaStoreCounters {
    pub materialize_structure_calls: u64,
    pub materialize_structure_cache_hits: u64,
    pub node_arena_lock_acquisitions: u64,
    pub family_map_lock_acquisitions: u64,
    pub dep_signature_merges: u64,
    pub dep_signature_intern_hits: u64,
}
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

// WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

/// Bound on the number of times the no-torn-return snapshot builder
/// retries when a mutation lands mid-build. Exceeding the bound is a
/// genuinely contended host; the builder then reports the build as
/// [`SnapshotBuildOutcome::Superseded`] rather than publishing a view
/// whose per-canonical snapshots could be torn across a mutation.
const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

/// Complete validity oracle for a [`StoreViewSnapshot`].
///
/// The token is the SOLE signal [`StoreViewManager`] uses to decide
/// whether a cached `Arc<HostStoreView>` is still safe to hand back, and
/// the SOLE signal the publish fence rechecks before promoting a cold
/// result. Two tokens compare equal iff every validation-affecting
/// by-value dimension of the store view is identical.
///
/// ## Why this set is COMPLETE (the soundness argument)
///
/// A [`HostStoreView`] caches two classes of state:
///
/// 1. **By-value snapshots** captured at build time — `whole_hashes`,
///    `derived_hashes`, `file_facts`, `route_surface_index_fingerprints`,
///    env hashes, project identity, project generation, and the
///    session tombstone/overlay deltas. A stale by-value snapshot would
///    mis-validate, so the token MUST advance whenever any of these can
///    change. Every host mutation that alters one of these advances
///    [`VerterHost::store_view_epoch`] (source/content, evict, reload,
///    `clear_compile_cache`, `close`, `set_import_dependencies`,
///    scheduler node membership) and/or
///    [`crate::project_type_store::ProjectTypeStore::project_generation`]
///    (project-shape / config / env / identity changes route through
///    `bump_project_generation_and_evict`). The env-hash fold +
///    project identity are folded in directly so the oracle is
///    self-contained even if a future workspace mutator changed env
///    without bumping a generation.
///
/// 2. **By-live-Arc-handle** dimensions — the `resolved_import_facts`
///    `Arc<ResolvedImportFactsDb>` and the `route_db`
///    `Arc<RouteDb>` handles. Both stay OUT of the token, but for two
///    DIFFERENT reasons:
///    - `ResolvedImportFactsDb` is content-addressed: its key includes
///      `content_hash`, so a new content version is a NEW key and a
///      fixed handle reads a correct value without a rebuild
///      (immutable-by-key).
///    - `RouteDb` is NOT content-addressed — `EffectiveExportSetKey` is
///      `(provider_canonical, project_identity, resolve_env_hash,
///      lib_env_hash)` with no content hash, and evict/clear/replace
///      reuse the same key. It stays out of the token because its
///      route-surface validator
///      ([`StoreView::validates_route_surface_domain`]) compares the
///      consumer's recorded `expected_hash` fingerprint against the live
///      `RouteDb` slot: an evicted/replaced entry yields a conservative
///      fail-closed MISS (the consumer recomputes through the cold
///      path), never a stale positive. The token therefore does not need
///      a `RouteDb` generation to stay sound — the fingerprint
///      comparison IS the validity rail.
///
/// Additive lazy loads observed mid-request (a dependency `FileArtifactStore`
/// publication that lands AFTER the snapshot was
/// built and does NOT bump the epoch) are NOT a soundness hole: for an
/// untracked canonical the snapshot stays untracked → the request-scoped
/// [`crate::resolver_core::CanonicalCompletionOverlay`] shadows it; for a
/// tracked canonical the content change already advanced the epoch.
///
/// `store_view_epoch` is an INPUT to the token, never the oracle by
/// itself — the token (epoch + generation + env fold + identity +
/// overlay identity) is the oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StoreViewValidationToken {
    /// Coarse semantic-mutation epoch
    /// ([`VerterHost::current_store_view_epoch`]). Advances on every
    /// host mutation that can change a by-value snapshot dimension.
    pub(crate) store_view_epoch: u64,
    /// Project generation
    /// ([`crate::project_type_store::ProjectTypeStore::project_generation`]).
    /// Advances on project-shape / config / env / identity changes via
    /// `bump_project_generation_and_evict`.
    pub(crate) project_generation: u64,
    /// Indexed-artifact publication generation
    /// ([`crate::file_artifact_store::FileArtifactStore::artifact_generation`]).
    /// Advances on every artifact insert / replace / evict / GC and
    /// augmentation-index mutation. This covers the BY-VALUE snapshot
    /// dimensions (`file_facts`, `derived_hashes`,
    /// `route_surface_index_fingerprints`) that a lazy
    /// `ensure_indexed_ready_serve` publication changes WITHOUT bumping
    /// `store_view_epoch` — without it a manager-cached base view would
    /// go stale after a lazy publication and warm-hit validation would
    /// false-miss (a steady-state warm-cache regression). The
    /// lazy-publication burst during a cold compute is bounded, so the
    /// cache rebuilds once then stays warm.
    pub(crate) artifact_generation: u64,
    /// Additive derived-state generation
    /// ([`VerterHost::current_load_generation`]). Advances on additive
    /// `derived_raw_cache` mutations the base view snapshots BY VALUE but
    /// that do NOT publish into `FileArtifactStore` (so
    /// `artifact_generation` does not cover them) and are NOT a
    /// content/project/env mutation (so `store_view_epoch` does not cover
    /// them). Two producers advance it:
    ///
    /// * a successful first-time `ensure_loaded` — a load that adds a
    ///   scheduler node + `derived_raw_cache` state (`whole_hashes`
    ///   membership / known-miss tags);
    /// * a positive import-route admission
    ///   ([`VerterHost::cache_positive_import_route_result`]) — which
    ///   writes `DerivedRawState.import_routes`, the source the base view
    ///   snapshots as the `ImportRoute` derived-hash domain (via
    ///   `generation_current_import_route_hash`).
    ///
    /// Included in the `StoreViewManager` REUSE oracle (either mutation
    /// invalidates the cached base view) but EXCLUDED from
    /// [`Self::externally_superseded_by`] — a cold compute's OWN
    /// dependency loads / route resolutions are its own work, not an
    /// external mutation, so they must not self-fence result promotion
    /// (same treatment as `artifact_generation`).
    pub(crate) load_generation: u64,
    /// Workspace content/file-set generation
    /// ([`verter_workspace::WorkspaceAccess::content_generation`]).
    /// Advances on every file-set mutation the workspace observes —
    /// inject / delete / overlay batch application, an OS-watcher
    /// recovery (`DirectoryTreeDirty`), and a resolve-extension change —
    /// WITHOUT any host-side epoch or generation necessarily moving
    /// (no `verter_session` handler observes `DirectoryTreeDirty`).
    ///
    /// The snapshot build is edge-currency-dependent on this LIVE value:
    /// `route_surface_is_edge_current` gates every Route/ImportRoute
    /// derived hash (base build, overlay re-root, completion overlay)
    /// against it at BUILD time. A cached snapshot whose gates were
    /// evaluated pre-mutation must therefore MISS once it advances —
    /// without this dimension the manager would keep validating warm
    /// entries across a watcher recovery or an edge-staleness transition
    /// (a dependency appeared / retargeted while the owner's content
    /// stayed put) for the snapshot's lifetime.
    ///
    /// Included in BOTH the `StoreViewManager` REUSE oracle and
    /// [`Self::externally_superseded_by`]: unlike the two additive
    /// generations above, a cold compute's OWN work (loads,
    /// `ensure_indexed_ready_serve`, store-view builds) NEVER advances it —
    /// only a real external file-set mutation does — so folding it into
    /// the supersession fingerprint cannot self-fence promotion.
    pub(crate) content_generation: u64,
    /// Folded env-hash bundle (R21). Self-contained defence: even if a
    /// future workspace mutator changed env without bumping a
    /// generation, the fold would still distinguish the views.
    pub(crate) env_hash_fold: Hash16,
    /// Workspace-default project identity (R21).
    pub(crate) project_identity: crate::file_artifact_store::ProjectIdentity,
    /// Frozen request-overlay / session identity.
    ///
    /// `None` for a base (non-session, no-completion-overlay) view.
    /// `Some(_)` carries the session id plus the count of canonicals the
    /// session has overlaid / tombstoned, so two requests with DIFFERENT
    /// completion overlays get DISTINCT token identities and a later
    /// block's proof memo never crosses an overlay boundary.
    pub(crate) overlay_identity: Option<OverlayIdentity>,
}

/// Frozen identity of a session / completion overlay folded into a
/// [`StoreViewValidationToken`]. Distinguishes a
/// base view from a session-overlaid one and distinguishes two sessions
/// whose overlay shapes differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OverlayIdentity {
    /// Raw session id (`SessionView`-scoped) — `None` for a
    /// completion-overlay-only view with no session.
    pub(crate) session_id: Option<u64>,
    /// Structural fold of the overlay's masked canonicals (count +
    /// per-canonical content hashes XOR-folded). Any change to the set
    /// of overlaid/tombstoned canonicals — or their content — changes
    /// this fold.
    pub(crate) overlay_fingerprint: Hash16,
}

impl StoreViewValidationToken {
    /// Whether `self` was SUPERSEDED by an EXTERNAL mutation relative to
    /// `later` — i.e. a `store_view_epoch` / `project_generation` /
    /// `content_generation` / env / identity change happened between the
    /// two captures.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation`:
    /// a cold compute
    /// legitimately publishes `IndexedReady` artifacts AND loads
    /// its dependencies (advancing those generations) as part of its own
    /// work. The publish fence must NOT treat the compute's OWN artifact
    /// publications or dependency loads as a supersession — only an
    /// external content/project/env/identity mutation invalidates the
    /// snapshot the result was produced against. (Those two generations
    /// remain in the full token for the `StoreViewManager` REUSE oracle,
    /// where a post-build publication / load SHOULD trigger a rebuild on
    /// the next request.)
    pub(crate) fn externally_superseded_by(&self, later: &Self) -> bool {
        self.store_view_epoch != later.store_view_epoch
            || self.project_generation != later.project_generation
            || self.content_generation != later.content_generation
            || self.env_hash_fold != later.env_hash_fold
            || self.project_identity != later.project_identity
            || self.overlay_identity != later.overlay_identity
    }

    /// A `u64` fingerprint folding ONLY the EXTERNAL-supersession
    /// dimensions ([`Self::externally_superseded_by`]: `store_view_epoch`,
    /// `project_generation`, `content_generation`, `env_hash_fold`,
    /// `project_identity`, `overlay_identity`).
    ///
    /// Two tokens fold to the same value iff neither externally
    /// supersedes the other (up to hash collision); they fold to
    /// different values iff one externally superseded the other. This is
    /// the seal-respecting `u64` the resolver-tier request executors
    /// compare to gate stable promotion: a snapshot whose external
    /// fingerprint no longer matches the live host fingerprint was
    /// externally superseded mid-compute (an epoch / project / env /
    /// identity / overlay change — e.g. a `set_default_resolve_extensions`
    /// env-hash shift that moves NO epoch) and its result MUST NOT be
    /// promoted to the shared cache.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation`
    /// for the SAME reason
    /// [`Self::externally_superseded_by`] does: a cold compute advances
    /// those generations as its OWN work (publishing artifacts, loading
    /// its dependencies, admitting its own routes), and folding them here
    /// would make the executor self-fence its own promotion.
    pub(crate) fn external_supersession_fingerprint(&self) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        self.store_view_epoch.hash(&mut hasher);
        self.project_generation.hash(&mut hasher);
        self.content_generation.hash(&mut hasher);
        self.env_hash_fold.hash(&mut hasher);
        self.project_identity.hash(&mut hasher);
        self.overlay_identity.hash(&mut hasher);
        hasher.finish()
    }

    /// Capture the current token from the host (base, no session
    /// overlay). The `overlay_identity` is `None`; a session-overlaid
    /// view stamps its own overlay identity in
    /// [`HostStoreView::with_session_overlay`].
    fn capture(host: &VerterHost) -> Self {
        let env_hashes = host.host_view_env_hashes();
        Self {
            store_view_epoch: host.current_store_view_epoch(),
            project_generation: host.project_type_store.project_generation(),
            artifact_generation: host.project_type_store.indexed().artifact_generation(),
            load_generation: host.current_load_generation(),
            content_generation: host.ws().content_generation(),
            env_hash_fold: fold_env_hashes(&env_hashes),
            project_identity: host.host_view_project_identity(),
            overlay_identity: None,
        }
    }

    /// A `u64` fingerprint for the singleflight / stability coalescing-lane
    /// identity
    /// ([`crate::resolver_core::StoreViewCompatToken::validity_fingerprint`]).
    ///
    /// Folds the EXTERNAL-supersession dimensions ONLY (`store_view_epoch`,
    /// `project_generation`, `content_generation`, `env_hash_fold`,
    /// `project_identity`, `overlay_identity`) — identical to
    /// [`Self::external_supersession_fingerprint`]. This is the SAME oracle
    /// the request executors' promotion fence (`is_stable`) compares, and it
    /// MUST be: the coalescing lane hands a LEADER's stable result to
    /// FOLLOWERS without per-follower revalidation, and the leader only
    /// promotes a result as `stable` when its snapshot's external fingerprint
    /// still matches the live host fingerprint. Two requests that share an
    /// external-supersession lane are therefore validation-equivalent for the
    /// promoted result: the leader's result is admissible exactly when the
    /// external dimensions are coherent, and a follower on the same lane
    /// shares those dimensions.
    ///
    /// Deliberately EXCLUDES `artifact_generation` / `load_generation`
    /// for the SAME reason
    /// [`Self::external_supersession_fingerprint`] does: a cold compute
    /// advances those generations as its OWN work (publishing
    /// `IndexedReady` artifacts, loading its dependencies), so two
    /// concurrent identical
    /// cold requests that snapshot at slightly different points in the load
    /// sweep observe DIFFERENT additive generations. Folding them into the
    /// lane identity would split those identical requests across distinct
    /// lanes and spawn multiple cold winners instead of one leader + N-1
    /// dedup-joining followers — the exact self-fencing the promotion oracle
    /// already avoids.
    pub(crate) fn lane_fingerprint(&self) -> u64 {
        self.external_supersession_fingerprint()
    }
}

/// Token-relevant raw inputs captured ONCE, before any per-canonical
/// snapshotting begins, so the entire snapshot is built under a single
/// coherent token.
///
/// **No-torn-snapshot contract.** `HostStoreView::build` populates the
/// per-canonical / per-domain snapshot maps (`whole_hashes`,
/// `file_facts`, `derived_hashes`, …) one source at a time. Every
/// token-relevant by-value dimension (the two additive generations,
/// the env-hash bundle, the project identity, the project generation)
/// MUST be read BEFORE that population window opens and stamped into the
/// view unchanged — never re-read live near the end of the build. If a
/// dimension is read late, a mid-build mutation that advances it WITHOUT
/// moving `store_view_epoch` (e.g. a resolve-extensions / env-hash
/// update, or a `project_generation` bump) would leave the view's
/// reconstructed token reflecting the NEW value while the snapshot maps
/// were captured under the OLD value, and the post-build coherence check
/// (`live_token == captured`) would accept a TORN view as coherent.
///
/// Capturing all inputs first closes that hole: the snapshot maps and the
/// token both derive from the SAME read window, so the post-build
/// comparison against the LIVE token detects any mid-build advance of any
/// dimension and forces a retry / `Superseded`.
#[derive(Clone, Copy)]
struct PreBuildTokenInputs {
    store_view_epoch: u64,
    project_generation: u64,
    artifact_generation: u64,
    load_generation: u64,
    content_generation: u64,
    env_hashes: crate::session_view::EnvHashes,
    project_identity: crate::file_artifact_store::ProjectIdentity,
}

impl PreBuildTokenInputs {
    /// Capture every token-relevant raw input from the host in one read
    /// window, before snapshotting begins.
    fn capture(host: &VerterHost) -> Self {
        Self {
            store_view_epoch: host.current_store_view_epoch(),
            project_generation: host.project_type_store.project_generation(),
            artifact_generation: host.project_type_store.indexed().artifact_generation(),
            load_generation: host.current_load_generation(),
            content_generation: host.ws().content_generation(),
            env_hashes: host.host_view_env_hashes(),
            project_identity: host.host_view_project_identity(),
        }
    }

    /// The complete base [`StoreViewValidationToken`] these inputs
    /// reconstruct (no overlay identity — a base view).
    fn token(&self) -> StoreViewValidationToken {
        StoreViewValidationToken {
            store_view_epoch: self.store_view_epoch,
            project_generation: self.project_generation,
            artifact_generation: self.artifact_generation,
            load_generation: self.load_generation,
            content_generation: self.content_generation,
            env_hash_fold: fold_env_hashes(&self.env_hashes),
            project_identity: self.project_identity,
            overlay_identity: None,
        }
    }
}

/// Fold the four R21 env hashes into one [`Hash16`] for the validation
/// token. Order-stable; each lane contributes a distinct domain tag so
/// two bundles that swap two equal-length lanes do not collide.
fn fold_env_hashes(env: &crate::session_view::EnvHashes) -> Hash16 {
    hash16_from_sorted(|hasher| {
        0u8.hash(hasher);
        env.parse_env_hash.hash(hasher);
        1u8.hash(hasher);
        env.resolve_env_hash.hash(hasher);
        2u8.hash(hasher);
        env.type_env_hash.hash(hasher);
        3u8.hash(hasher);
        env.lib_env_hash.hash(hasher);
    })
}

/// Outcome of a no-torn-return snapshot build. When the host mutates
/// faster than the builder can complete a coherent capture, the
/// builder reports [`Self::Superseded`] instead of treating the build as
/// publishable — its per-canonical snapshots straddle a mutation.
///
/// `Superseded` still carries the freshest built view. That view is NOT
/// coherent against the live host (it was stamped under the stale
/// pre-build token capture), so it is NEVER published into the
/// [`StoreViewManager`] cache; but on bounded-retry exhaustion the manager
/// hands it back to the caller return-only (the caller's `is_stable` /
/// publish fence then rejects it for promotion). Carrying the freshest
/// view is what lets [`StoreViewManager::base_view`] terminate in bounded
/// time under sustained token churn instead of re-claiming a
/// never-coherent build forever.
///
/// The size gap between the two variants is intentional: this is a
/// transient by-value return matched immediately at the build site and
/// never stored in a collection, so boxing the common `Coherent` variant
/// would add a heap allocation on the hot coherent path to shrink a value
/// that only ever lives on the stack for one move.
#[allow(clippy::large_enum_variant)]
enum SnapshotBuildOutcome {
    /// A coherent snapshot whose pre-build and post-build tokens match.
    Coherent {
        view: HostStoreView,
        token: StoreViewValidationToken,
    },
    /// The host mutated on every attempt; no coherent snapshot was
    /// produced. Carries the FRESHEST built view (stamped under the stale
    /// pre-build token capture). The manager retries (a concurrent winner
    /// may publish a coherent view in the meantime); on retry-cap
    /// exhaustion it returns this view WITHOUT caching it, so an incoherent
    /// view is never published and the caller degrades to return-only.
    Superseded { view: HostStoreView },
}

/// Result of attempting to publish a freshly-built coherent view into the
/// manager cache.
///
/// `base_view` MUST NOT hand a [`Self::Declined`] view to a warm-cache
/// validator: a declined view is one the manager has already determined is
/// stale (a `clear()`/reset raced the build, or the live token moved past
/// the build's token), and direct callers such as
/// `try_component_meta_cache_hit` validate a cached entry's
/// `ReadSetSignature.facts` against the returned view with NO outer
/// freshness fence. A declined view drives a bounded re-loop in
/// `base_view` (rebuild against the freshly-read live token), exactly like
/// [`SnapshotBuildOutcome::Superseded`]; only on retry-cap exhaustion is it
/// handed back return-only.
#[allow(clippy::large_enum_variant)]
enum PublishOutcome {
    /// The view is current: it was published (or coalesced onto an
    /// already-published same-token entry) and is safe to return to a warm
    /// validator. The carried view's token equals the live host token.
    Published { view: HostStoreView },
    /// A reset raced the build, or the live token moved past the build's
    /// token between build completion and publish. The carried view is
    /// KNOWN-STALE; it must drive a bounded re-loop, never be returned to a
    /// warm validator as if it were current.
    Declined { view: HostStoreView },
}

/// Why a base/resolver store-view read could not produce a view the
/// manager can prove is current.
///
/// Mirrors the established `CacheAdmission::ReturnOnly { reason }` /
/// `ComputeAdmission::ReturnOnly` vocabulary: a return-only value is
/// handed back so a cold builder can still seed its own fenced compute,
/// but it is KNOWN non-current and MUST NOT validate a warm cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreViewReturnOnlyReason {
    /// The bounded cooperative retry budget was exhausted while every
    /// claimed build was declined (a reset raced it, or the live token
    /// moved past the build's token) — the freshest built view is
    /// known-stale.
    PublishDeclined,
    /// The bounded cooperative retry budget was exhausted while every
    /// build attempt observed a mid-build mutation — `build_coherent`
    /// reported supersession on every round.
    Superseded,
}

/// A [`HostStoreView`] the [`StoreViewManager`] proved current at
/// handoff: it was published (or coalesced onto an already-published
/// same-token entry) through [`PublishOutcome::Published`], and its
/// token matched the live host token under the manager lock at that
/// moment.
///
/// This wrapper is the type-level proof that a view is safe for warm-
/// cache validation. The fact-validation entry points
/// ([`crate::component_meta_result_db::ComponentMetaResultDb::get_with_view`]
/// and the imported-root / route warm validators) accept ONLY a
/// `&CurrentHostStoreView`, so a known-stale [`StoreViewReturnOnlyReason`]
/// snapshot CANNOT reach fact validation by construction. A raw
/// `HostStoreView` is reserved for cold-builder seeding, whose own
/// `is_stable` / publish fence rejects a superseded result before it can
/// warm the shared cache.
#[derive(Debug, Clone)]
pub(crate) struct CurrentHostStoreView(HostStoreView);

impl CurrentHostStoreView {
    /// Borrow the proven-current view for warm-cache validation.
    pub(crate) fn view(&self) -> &HostStoreView {
        &self.0
    }

    /// Re-root this proven-current base view through a session overlay
    /// WITHOUT laundering currentness: the overlay re-roots per-canonical
    /// snapshots and recomputes the coalescing fingerprint, but the base
    /// was already proven current, so the overlaid view is current too.
    /// A non-current base never reaches this method — it stays
    /// [`StoreViewRead::ReturnOnly`].
    pub(crate) fn with_session_overlay(
        self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Self {
        CurrentHostStoreView(self.0.with_session_overlay(host, view))
    }
}

/// A [`HostStoreView`] the [`StoreViewManager`] could NOT prove current at
/// handoff, exposed ONLY for fenced cold-builder seeding.
///
/// This wrapper is the type-level counterpart of [`CurrentHostStoreView`]:
/// it deliberately exposes NO `validates*` surface, so a cold-seed view can
/// never reach a warm-cache fact validator by construction. A cold builder
/// threads it into a [`HostResolverContext`](crate::resolver_core::HostResolverContext)
/// via [`HostResolverContext::from_cold_seed`](crate::resolver_core::HostResolverContext::from_cold_seed)
/// — that constructor marks the request-bound view non-current so every
/// nested warm-cache probe inside the dispatch MISSES rather than validating
/// against the stale seed. The builder's own `is_stable` / publish fence
/// (or the bounded-retry-then-supersede contract on a query-returner) is what
/// guards a result computed from this seed; a [`ColdSeedHostStoreView`] is
/// never an admission of currentness.
#[derive(Debug, Clone)]
pub(crate) struct ColdSeedHostStoreView {
    view: HostStoreView,
    /// Whether the underlying read was actually [`StoreViewRead::Current`]
    /// (a cold builder that legitimately seeds from a freshly-current view)
    /// or [`StoreViewRead::ReturnOnly`] (a known-stale seed). The publish
    /// fence and the request-bound nested-probe gate read this to decide
    /// whether warm-cache probes through the derived context may validate.
    current: bool,
}

impl ColdSeedHostStoreView {
    /// Borrow the seed view for COLD-BUILDER context construction.
    ///
    /// The borrow feeds [`HostResolverContext::from_cold_seed`](crate::resolver_core::HostResolverContext::from_cold_seed);
    /// it MUST NOT be used to validate a warm cache entry — this type
    /// exposes no `validates*` method precisely so that path does not exist.
    pub(crate) fn view(&self) -> &HostStoreView {
        &self.view
    }

    /// Whether the seed originated from a proven-current read. A cold
    /// builder that seeds from a current read may still let its nested
    /// warm-cache probes validate (the seed is coherent); a `ReturnOnly`
    /// seed forces every nested probe to miss.
    pub(crate) fn is_current(&self) -> bool {
        self.current
    }

    /// Re-root the seed view through a session overlay WITHOUT dropping
    /// currentness.
    ///
    /// The session-bound cold-compute path
    /// ([`crate::resolver_core::SessionResolverContext::from_cold_seed`])
    /// needs the overlay-rooted form of the seed (per-canonical snapshots
    /// re-rooted, the coalescing fingerprint recomputed) while preserving
    /// the seed's `current` flag — so a `ReturnOnly` seed stays non-current
    /// after overlaying and its derived request-bound view fails every
    /// `validates*` closed. This is the ONLY currentness-preserving
    /// overlay route for a seed: overlaying through
    /// `.into_inner().with_session_overlay(..)` discards the flag at the
    /// `into_inner` boundary, and the overlaid raw view would validate
    /// warm cache entries against a stale seed.
    #[must_use]
    pub(crate) fn with_session_overlay(
        self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Self {
        Self {
            view: self.view.with_session_overlay(host, view),
            current: self.current,
        }
    }

    /// Consume into the raw [`HostStoreView`], for the few cold builders
    /// and driver-snapshot accessors that own the view outright and do NOT
    /// validate a warm cache entry against it (e.g. the request-driver
    /// `snapshot_store_view` accessors, whose currentness is gated
    /// separately by `snapshot_view_is_current`, and `#[cfg(test)]`
    /// direct-`host` fixtures). Currentness is DROPPED on this boundary —
    /// any path that builds a resolver context performing nested
    /// warm-cache validation MUST instead carry the seed via
    /// [`Self::with_session_overlay`] /
    /// [`crate::resolver_core::HostResolverContext::from_cold_seed`] /
    /// [`crate::resolver_core::SessionResolverContext::from_cold_seed`] so
    /// the flag reaches the request-bound view. The static guard
    /// `cold_seed_into_inner_confined_to_non_validating_allowlist` confines
    /// this escape hatch to the non-validating allowlist.
    pub(crate) fn into_inner(self) -> HostStoreView {
        self.view
    }
}

/// One coherent store-view snapshot captured ONCE for a whole
/// component-meta batch, with the session overlay applied ONCE and every
/// per-capability view derived from that single overlaid read.
///
/// Without a fixed view, every per-job closure in a warm batch calls
/// `resolver_store_view_read()` at least twice (the warm-cache probe and
/// the extraction-context cold-seed), each a lock + validate + clone, AND
/// re-applies the session overlay (a full `StoreViewSnapshot` COW + O(overlay)
/// re-rooting) per job. Capturing one [`BatchFixedView`] per batch collapses
/// both to O(1): the overlay is applied once at capture, the warm probe
/// borrows the OVERLAID [`Self::current_view`], the executor pins to the
/// OVERLAID [`Self::executor_view`] + [`Self::captured_fingerprint`], and
/// the extraction context seeds from a clone of the OVERLAID
/// [`Self::cold_seed`].
///
/// Currentness is INTRINSIC to the read: the warm-probe view, the
/// cold-seed, and the captured fingerprint all come from the SAME
/// [`StoreViewRead`], so there is no loose `is_current` flag to re-pair
/// with a different read's view. The overlay is applied to that one read
/// via a single copy-on-write, so the overlaid wrappers all share one
/// overlaid snapshot `Arc`.
///
/// FENCE CONSISTENCY: [`Self::captured_fingerprint`] /
/// [`Self::captured_token`] are the BASE-external token (overlay identity
/// normalised out), so they compare like-for-like against the host's live
/// BASE token in the captured-vs-live fences — the frozen overlay identity
/// never false-supersedes, while a real external mutation still does. See
/// [`crate::VerterHost::capture_batch_fixed_view`] for the full rationale.
///
/// SOUNDNESS: the captured fingerprint + token are the snapshot's
/// external-supersession proof at capture time. The executor's promotion
/// fence ([`crate::resolver_core::run_component_meta_request`] with a
/// fixed view) and the payload-write fence
/// ([`Self::payload_promotion_admissible`]) both compare captured-vs-live
/// before warming any shared cache, so a mid-batch external mutation
/// (epoch / project-generation / env-hash / identity / overlay) makes the
/// affected jobs return-only rather than promoting a stale result. The
/// fixed view is a coherent single-token snapshot; correctness for
/// mid-batch invalidation comes from these captured-vs-live fences, not
/// from assuming the view stays current.
#[derive(Clone)]
pub(crate) struct BatchFixedView {
    /// The proven-current OVERLAID view for warm-cache probes, or `None`
    /// when the captured read was [`StoreViewRead::ReturnOnly`] (known-stale
    /// under sustained churn). A `None` here forces every per-job warm probe
    /// to miss to the cold path — never validating a cache entry against a
    /// stale snapshot. The session overlay is already applied (once, at
    /// capture), so a per-job probe reads it directly with no further COW.
    current_view: Option<CurrentHostStoreView>,
    /// The OVERLAID cold-seed form of the captured read, for the extraction
    /// context (and any cold compute). Carries the read's currentness so
    /// a non-current seed fails nested warm-cache probes closed. Already
    /// overlay-rooted — the cold compute seeds from it without re-applying
    /// the overlay.
    cold_seed: ColdSeedHostStoreView,
    /// The OVERLAID raw view to pin the request executor's fixed-view fast
    /// path to, paired with [`Self::captured_fingerprint`]. The executor
    /// threads this overlaid view into the cold compute's seed directly, so
    /// the cold compute does not re-apply the overlay per job.
    executor_view: HostStoreView,
    /// BASE-external-supersession fingerprint of the captured snapshot
    /// (overlay identity normalised out). The executor's fixed-view
    /// promotion fence compares this against the live host BASE fingerprint
    /// — like-for-like, so the frozen overlay identity never
    /// false-supersedes.
    captured_fingerprint: u64,
    /// BASE validation token of the captured snapshot (overlay identity
    /// normalised out), for the payload-write fence
    /// ([`Self::payload_promotion_admissible`]). Compared against the host's
    /// live BASE token.
    captured_token: StoreViewValidationToken,
    /// Whether the captured read was proven current. A non-current
    /// capture is never promotable (the payload fence declines).
    is_current: bool,
}

impl BatchFixedView {
    /// The proven-current OVERLAID view for a per-job warm-cache probe, or
    /// `None` when the captured read was known-stale (probe must miss to
    /// cold). The session overlay is already applied — the probe validates
    /// against the overlay-aware view directly, with no per-job COW.
    pub(crate) fn current_view(&self) -> Option<&CurrentHostStoreView> {
        self.current_view.as_ref()
    }

    /// Borrow the shared OVERLAID cold-seed for an extraction / cold-compute
    /// context. The same overlay-rooted seed is reused by every per-job
    /// closure in the batch (cheap `Arc`-backed clone when an owned seed is
    /// needed) — the cold compute does not re-apply the overlay.
    pub(crate) fn cold_seed(&self) -> &ColdSeedHostStoreView {
        &self.cold_seed
    }

    /// The `(overlaid_view, captured_fingerprint)` pair to pin the request
    /// executor's fenced fixed-view fast path to. The view is overlay-rooted;
    /// the fingerprint is the BASE-external one (overlay identity normalised
    /// out) so the executor's captured-vs-live fence compares like-for-like.
    pub(crate) fn executor_fixed_view(&self) -> (&HostStoreView, u64) {
        (&self.executor_view, self.captured_fingerprint)
    }

    /// Whether a payload computed under this fixed view may be PROMOTED to
    /// the shared payload cache.
    ///
    /// Two gates, mirroring the cold-path publish fence
    /// ([`crate::VerterHost::validation_token_still_live`]):
    ///
    /// 1. the capture was proven current (a non-current capture is
    ///    return-only by the [`StoreViewRead`] contract), and
    /// 2. no EXTERNAL mutation (epoch / project-generation / env-hash /
    ///    identity / overlay) landed since capture — i.e. the captured
    ///    token is not externally superseded by the live token.
    ///
    /// On a decline the caller still RETURNS the payload to the consumer;
    /// only the cache promotion is dropped.
    pub(crate) fn payload_promotion_admissible(&self, host: &VerterHost) -> bool {
        self.is_current
            && !self
                .captured_token
                .externally_superseded_by(&host.current_validation_token())
    }

    /// The validation token of the captured snapshot, for a cold-path
    /// publish fence that must recheck this exact snapshot against the live
    /// host (the analysis path's `ComponentMetaResultDb` promotion).
    pub(crate) fn captured_validation_token(&self) -> StoreViewValidationToken {
        self.captured_token
    }

    /// Whether the captured read was proven current. A non-current capture
    /// is never promotable; warm probes against it must miss to cold.
    pub(crate) fn is_current(&self) -> bool {
        self.is_current
    }
}

/// Result of a base/resolver store-view read.
///
/// The bounded cooperative retry loop in [`StoreViewManager::base_view`]
/// either proves a view current (publishes/coalesces it under a live-
/// matching token) or, on retry-budget exhaustion under sustained churn,
/// reports the freshest built view as KNOWN non-current. This typed split
/// makes that distinction unforgeable at the call site:
///
/// * [`Self::Current`] — safe for warm-cache fact validation.
/// * [`Self::ReturnOnly`] — known non-current; a warm validator MUST
///   treat it as a cache MISS and fall to the cold path, which runs its
///   own `is_stable` / publish fence and never promotes a superseded
///   result.
///
/// Replaces the prior "retry 3× then return the freshest stale view as a
/// plain [`HostStoreView`]" behaviour, which let a warm validator
/// false-positive a stale cache entry against an already-superseded
/// snapshot.
#[derive(Debug, Clone)]
pub(crate) enum StoreViewRead {
    /// The manager proved this view current at handoff.
    Current(CurrentHostStoreView),
    /// The manager could not prove the view current; it is returned only
    /// so a cold builder can seed its own fenced compute.
    ReturnOnly {
        view: HostStoreView,
        reason: StoreViewReturnOnlyReason,
    },
}

impl StoreViewRead {
    /// The proven-current view, or `None` when the read is
    /// [`Self::ReturnOnly`]. Warm-cache validators call this and treat
    /// `None` as a cache miss.
    pub(crate) fn current(self) -> Option<CurrentHostStoreView> {
        match self {
            StoreViewRead::Current(current) => Some(current),
            StoreViewRead::ReturnOnly { .. } => None,
        }
    }

    /// Whether this read is [`Self::Current`], for the cold-path PUBLISH
    /// fence.
    ///
    /// A cold compute MAY seed from either arm (via
    /// [`Self::into_cold_seed_view`]), but the result of a compute seeded
    /// by a non-current ([`Self::ReturnOnly`]) view must NEVER be promoted
    /// to the shared cache — the manager could not prove the snapshot
    /// current, so the result is return-only. The publish fence reads this
    /// from the SAME `StoreViewRead` it derives the cold-seed view from
    /// (see [`crate::VerterHost::cold_seed_view_and_fence`]) so the
    /// currentness and the seeded view cannot describe different snapshots.
    pub(crate) fn is_current_for_promotion(&self) -> bool {
        matches!(self, StoreViewRead::Current(_))
    }

    /// Re-bind a request-driver's executor snapshot `(view, is_current)`
    /// into the currentness-carrying typed read.
    ///
    /// This is the SOLE constructor that pairs a raw [`HostStoreView`] with
    /// a separately-named currentness bit, and it exists ONLY for the
    /// stable-request executor boundary, where the pair provably came from a
    /// SINGLE [`crate::VerterHost::resolver_store_view_with_currentness`]
    /// read: the executor's `snapshot_view` destructures one
    /// [`StoreViewRead`] into `(Self::View, snapshot_view_current)` and
    /// threads both — coherent by construction — into `compute`. The cold
    /// compute re-binds them here so currentness flows on through
    /// [`Self::into_cold_seed_view`] (intrinsic to the arm), never as a
    /// flag a downstream helper could re-pair with a DIFFERENT read's view.
    ///
    /// Every cold-compute helper that does its OWN fresh read MUST instead
    /// take the cold-seed straight from that read via
    /// [`Self::into_cold_seed_view`] — the view and its currentness then
    /// originate from one read with no flag to mismatch. Feeding this
    /// constructor a view from one read and an `is_current` from another is
    /// the exact currentness/view divergence the static guard
    /// `cold_seed_currentness_is_intrinsic_to_the_read` forbids outside the
    /// executor boundary.
    ///
    /// `is_current` MUST be the manager's currentness proof for `view`; a
    /// non-current snapshot re-binds to [`Self::ReturnOnly`] so the derived
    /// context fails its nested warm-cache probes closed.
    #[must_use]
    pub(crate) fn from_executor_snapshot(view: HostStoreView, is_current: bool) -> Self {
        if is_current {
            StoreViewRead::Current(CurrentHostStoreView(view))
        } else {
            // The executor proved the snapshot non-current (a `ReturnOnly`
            // read under sustained churn). The classified reason is the
            // manager's, recorded at the read; at this re-bind boundary the
            // executor carries only the bit, so attribute the supersession
            // generically — the cold-seed's `current=false` is what gates
            // nested probes, not the reason.
            StoreViewRead::ReturnOnly {
                view,
                reason: StoreViewReturnOnlyReason::Superseded,
            }
        }
    }

    /// Unwrap to a [`ColdSeedHostStoreView`] for COLD-BUILDER seeding only.
    ///
    /// Both arms carry a usable view; a cold builder threads it into a
    /// `HostResolverContext` whose `is_stable` / publish fence already
    /// rejects a result torn by a mid-flight change. The returned
    /// [`ColdSeedHostStoreView`] exposes NO `validates*` surface, so it
    /// CANNOT be used to validate a warm cache entry — that is what
    /// [`Self::current`] + [`CurrentHostStoreView`] enforce. It carries the
    /// read's currentness (`Current` vs `ReturnOnly`) so the derived
    /// context can fail-close nested warm-cache probes on a stale seed.
    pub(crate) fn into_cold_seed_view(self) -> ColdSeedHostStoreView {
        match self {
            StoreViewRead::Current(current) => ColdSeedHostStoreView {
                view: current.0,
                current: true,
            },
            StoreViewRead::ReturnOnly { view, reason } => {
                // A cold builder seeded with a known-stale view: its own
                // `is_stable` / publish fence will reject promotion, and the
                // derived context fails nested warm-cache probes closed.
                // Trace the classified reason so the cause of a degraded-to-
                // return-only request is observable.
                tracing::debug!(
                    target: "verter::store_view",
                    ?reason,
                    "store-view read fell back to a ReturnOnly cold-seed view",
                );
                ColdSeedHostStoreView {
                    view,
                    current: false,
                }
            }
        }
    }

    /// Extract the underlying raw [`HostStoreView`] regardless of arm, for
    /// the bare-host owned-view rail (`ResolverContext::resolver_store_view`,
    /// reachable only when no request-bound context was installed — a
    /// test/debug validation fallback). Production warm validators take a
    /// `&CurrentHostStoreView` via [`Self::current`]; production cold
    /// builders take a [`ColdSeedHostStoreView`] via
    /// [`Self::into_cold_seed_view`]. This escape hatch is NOT a
    /// warm-validation entry point — the static guard exempts only this
    /// single producer.
    pub(crate) fn into_owned_view(self) -> HostStoreView {
        match self {
            StoreViewRead::Current(current) => current.0,
            StoreViewRead::ReturnOnly { view, .. } => view,
        }
    }

    /// Test-only: the carried view regardless of arm, for assertions
    /// that inspect the view a read produced.
    #[cfg(test)]
    pub(crate) fn view_for_tests(&self) -> &HostStoreView {
        match self {
            StoreViewRead::Current(current) => current.view(),
            StoreViewRead::ReturnOnly { view, .. } => view,
        }
    }

    /// Test-only: whether this read is [`Self::Current`].
    #[cfg(test)]
    pub(crate) fn is_current_for_tests(&self) -> bool {
        matches!(self, StoreViewRead::Current(_))
    }

    /// Test-only: the [`StoreViewReturnOnlyReason`] when the read is
    /// [`Self::ReturnOnly`], else `None`.
    #[cfg(test)]
    pub(crate) fn return_only_reason_for_tests(&self) -> Option<StoreViewReturnOnlyReason> {
        match self {
            StoreViewRead::ReturnOnly { reason, .. } => Some(*reason),
            StoreViewRead::Current(_) => None,
        }
    }
}

/// Immutable, `Arc`-shareable per-view snapshot.
///
/// Holds every by-value snapshot dimension a validator reads. Wrapped in
/// an `Arc` by [`HostStoreView`] so cloning a view is a refcount bump
/// rather than a deep map copy — this is what lets
/// [`StoreViewManager`] hand the same workspace snapshot to every batch
/// job for the cost of an `Arc::clone`.
///
/// `with_session_overlay` re-roots a session's overlaid canonicals via
/// copy-on-write (`Arc::make_mut`): the SHARED base snapshot is never
/// mutated in place — the first session write clones the inner snapshot,
/// leaving the manager-cached base pristine for concurrent base readers.
#[derive(Debug, Clone)]
pub(crate) struct StoreViewSnapshot {
    /// Overlay-set fingerprint of the active session view (R29 +
    /// overlay isolation). `0` for a base / overlay-free view;
    /// non-zero once [`HostStoreView::with_session_overlay`] captures
    /// the session's [`crate::session_view::SessionView::fingerprint`].
    ///
    /// This is the CONTENT-ADDRESSED augmentation-index population
    /// identity for the view: the route-surface validator composes
    /// [`HostStoreView::augmentation_population`] from it so a session
    /// read validates against the `Session(fingerprint)` augmenter-set
    /// fingerprint and a base read against the `Base` one — the
    /// content-addressed `AugmentationTargetKey.population` slot can
    /// never be cross-validated. The QUERY-IDENTITY `EffectiveExportSetKey`
    /// is keyed instead by the CONTENT-FREE `session_scope`
    /// ([`crate::resolver_core::route_db::EffectiveExportSetScope`], derived
    /// from `session_id`, R6) — this overlay-set fingerprint never enters
    /// that key. Mirrors the SINGLE derivation in
    /// [`crate::session_view::augmentation_population_for_view`].
    session_overlay_fingerprint: u64,
    whole_hashes: FxHashMap<String, Hash16>,
    derived_hashes: FxHashMap<(String, crate::resolver_core::DerivedFactKind), Hash16>,
    /// Route-surface-domain snapshot — augmentation-index fingerprints
    /// keyed by a structural representation of the
    /// `(target_kind_tag, target_payload)` shape. Validation against
    /// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` consults
    /// this map (R29 + G1 + R26). An absent key means the
    /// augmentation-index entry has not yet been populated — the
    /// validator returns `false` so the downstream cache misses.
    route_surface_index_fingerprints: FxHashMap<RouteSurfaceIndexShapeKey, Hash16>,
    /// Parse-domain snapshot (R26): per-canonical `Arc<FileFacts>`
    /// captured at view-build time. The validator for `ParseFactRef`
    /// reads through this map; one `Arc::clone` per tracked file at
    /// build time, wait-free hash compares thereafter.
    file_facts: FxHashMap<String, std::sync::Arc<crate::file_artifact_store::FileFacts>>,
    /// Resolve-imports-domain handle (R26): `Arc` clone of the
    /// project store's `ResolvedImportFactsDb`. Immutable-by-key
    /// (content-addressed): a fixed handle reads correct values
    /// without a snapshot rebuild, so it does NOT enter the validation
    /// token. The validator composes `ResolvedImportFactsKey` from the
    /// fact + this view's tracked `whole_hashes` / known-miss tags /
    /// `env_hashes`.
    resolved_import_facts:
        Option<std::sync::Arc<crate::resolved_import_facts::ResolvedImportFactsDb>>,
    /// Per-canonical known-miss generation tag captured at view-build
    /// time. Folds the owner's
    /// `DerivedRawState::import_routes_known_miss_recorded_at_generation`
    /// so the validator composes the same `known_miss_generation` key
    /// dimension the producer admitted under. Absent → `[0u8; 16]`.
    resolved_import_facts_known_miss_tags: FxHashMap<String, Hash16>,
    /// Route-surface-domain handle (R26): `Arc` clone of the project
    /// store's `RouteDb`. Immutable-by-key like `resolved_import_facts`;
    /// not in the validation token.
    route_db: Option<std::sync::Arc<crate::resolver_core::route_db::RouteDb>>,
    /// Env-hash bundle (R21) captured at view-build time.
    env_hashes: crate::session_view::EnvHashes,
    /// Project identity captured at view-build time (R21).
    project_identity: crate::file_artifact_store::ProjectIdentity,
    /// Monotonic project generation captured at view-build time. The
    /// validator for `FactVersionRef::ProjectGeneration` compares a
    /// fact's observed generation against this snapshot.
    project_generation: u64,
    /// Canonicals the active session has TOMBSTONED (overlay-Deleted).
    /// Empty on a base (non-session) view — only `with_session_overlay`
    /// populates it. Keeps a tombstoned canonical distinguishable from a
    /// genuinely-untracked one: the `FileWholeHash` / `DirectSource`
    /// validator arms reject a tombstoned canonical before the lazy
    /// untracked-accept rule.
    tombstoned_canonicals: std::collections::HashSet<String>,
}

impl Default for StoreViewSnapshot {
    fn default() -> Self {
        Self {
            session_overlay_fingerprint: 0,
            whole_hashes: FxHashMap::default(),
            derived_hashes: FxHashMap::default(),
            route_surface_index_fingerprints: FxHashMap::default(),
            file_facts: FxHashMap::default(),
            resolved_import_facts: None,
            resolved_import_facts_known_miss_tags: FxHashMap::default(),
            route_db: None,
            env_hashes: crate::session_view::EnvHashes::default(),
            project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
            project_generation: 0,
            tombstoned_canonicals: std::collections::HashSet::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: crate::resolver_core::StoreViewCompatToken,
    mutation_epoch: u64,
    session_id: Option<u64>,
    /// `Arc`-shared immutable snapshot. Cloning a [`HostStoreView`] is a
    /// refcount bump on this `Arc`; `with_session_overlay` re-roots
    /// overlaid canonicals via `Arc::make_mut` (copy-on-write) so the
    /// shared base is never mutated in place.
    snapshot: Arc<StoreViewSnapshot>,
    /// Frozen overlay identity for the validation token. `None` on a base view; `Some(_)` once
    /// [`Self::with_session_overlay`] has re-rooted overlay/tombstone
    /// canonicals — it carries the session id + a structural fingerprint
    /// of the masked canonical set so the [`StoreViewValidationToken`] of
    /// a session-overlaid view is distinct from the base token and from
    /// another session's token.
    overlay_identity: Option<OverlayIdentity>,
    /// Indexed-artifact publication + first-time-load
    /// generations captured at build time. View-level identity (not
    /// per-canonical content) so [`Self::validation_token`] can
    /// reconstruct the by-value-dimension generations without re-reading
    /// the host.
    artifact_generation: u64,
    load_generation: u64,
    /// Workspace content/file-set generation captured at build time
    /// (same single pre-build read window as the generations above) so
    /// [`Self::validation_token`] can reconstruct the token's
    /// `content_generation` dimension without re-reading the host.
    content_generation: u64,
}

/// Structural key for snapshotting `ModuleAugmentationIndexShape`
/// fingerprints into [`HostStoreView`]. Mirrors the parallel
/// optional fields of `FactKey::ModuleAugmentationIndexShape`, plus the
/// augmentation-index `population` dimension.
///
/// `population` keeps the base and session augmenter-set fingerprints in
/// DISTINCT snapshot slots: the store's augmentation index holds both a
/// `(target, Base) → base_fp` and a `(target, Session(fp)) → session_fp`
/// entry, and folding them into a population-blind key would collide
/// (last-writer-wins), letting a base fact validate against a session
/// fingerprint or vice versa. The route-surface validator composes the
/// active view's population ([`HostStoreView::augmentation_population`])
/// so warm validation is population-aware.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RouteSurfaceIndexShapeKey {
    pub target_kind_tag: verter_semantic::facts::registry::AugmentationTargetKindTag,
    pub external_specifier: Option<String>,
    pub resolved_relative_canonical: Option<String>,
    pub wildcard_pattern: Option<String>,
    pub population: crate::file_artifact_store::AugmentationPopulation,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: crate::resolver_core::StoreViewCompatToken {
                epoch: 0,
                session: None,
                validity_fingerprint: 0,
            },
            mutation_epoch: 0,
            session_id: None,
            snapshot: Arc::new(StoreViewSnapshot::default()),
            overlay_identity: None,
            artifact_generation: 0,
            load_generation: 0,
            content_generation: 0,
        }
    }
}

// Test-only thread-local counter incremented every time
// `HostStoreView::from_host` is called. The discriminating tests for
// The per-request hoist read this counter to assert that a
// single component-meta request builds the view exactly once instead
// of 8-12+ times. Thread-local so parallel `cargo test` execution
// does not cross-pollute counts. Production builds do not pay for
// the increment (gated under `#[cfg(test)]`).
#[cfg(test)]
thread_local! {
    pub(crate) static HOST_STORE_VIEW_FROM_HOST_BUILDS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };

    /// Test-only PER-THREAD sweep counter, incremented in lockstep with the
    /// process-wide [`STORE_VIEW_COHERENT_BUILD_SWEEPS`] on every
    /// `build_coherent` attempt that runs ON THIS THREAD. The
    /// singleflight regression sums this across its OWN spawned threads so
    /// its sweep-count assertion is robust against concurrent build
    /// activity from unrelated parallel tests (which inflates the
    /// process-wide counter but never this thread-local).
    pub(crate) static COHERENT_BUILD_SWEEPS_THIS_THREAD: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };

    /// Test-only knob: the number of leading `build_coherent` attempts
    /// that must force a mid-build mutation (so their post-build token
    /// differs and the attempt is treated as superseded). Set to
    /// `STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS` to exhaust every retry and
    /// drive `build_coherent` to `SnapshotBuildOutcome::Superseded`.
    pub(crate) static FORCE_SUPERSEDE_ATTEMPTS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };

    /// Test-only knob: when `true`, the next `build_coherent` call on
    /// this thread panics in the middle of the build (after the
    /// singleflight claim has been taken). Drives the
    /// builder-panic-must-not-leave-the-claim-stuck regression: a panic
    /// unwinding past the claim-clear statements must still release the
    /// `StoreViewManager` build claim (RAII guard), so subsequent
    /// callers do not block forever on the `built` condvar.
    pub(crate) static FORCE_BUILD_PANIC: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };

    /// Test-only knob: when armed, the next `build` call on this thread
    /// performs an env-hash mutation IN THE MIDDLE of the build —
    /// AFTER the per-canonical snapshot maps are populated but BEFORE the
    /// token-relevant env-hash / project-identity dimensions are stamped.
    /// The mutation advances `resolve_env_hash` WITHOUT bumping
    /// `store_view_epoch`. One-shot; disarmed on fire.
    ///
    /// Drives the build-coherence regression: with a LATE env read inside
    /// `build`, the view's reconstructed token would reflect the NEW
    /// (post-mutation) env while the snapshot maps were captured under the
    /// OLD env, and the post-build coherence check (which compared a token
    /// reconstructed from the same late reads) would accept the TORN view
    /// as coherent. With the pre-capture stamp the view's token reflects
    /// the OLD env, the post-build LIVE token reflects the NEW env, the
    /// comparison mismatches, and the attempt is treated as superseded.
    pub(crate) static FORCE_MID_BUILD_ENV_BUMP: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };

    /// Test-only knob: when `true`, EVERY `build_coherent` attempt on this
    /// thread forces a mid-build mutation (advancing `store_view_epoch`),
    /// so every attempt is superseded and `build_coherent` never produces a
    /// coherent view. PERSISTENT (not one-shot) — it models a host whose
    /// validation token churns on every snapshot attempt under sustained
    /// load/publication.
    ///
    /// Drives the bounded-retry liveness gate: the
    /// [`StoreViewManager::base_view`] outer loop must TERMINATE within its
    /// bounded retry cap and hand back the freshest built view WITHOUT
    /// caching it (return-only), rather than spinning forever re-claiming a
    /// build that is superseded every time. With an unbounded retry,
    /// `base_view` would loop indefinitely (a hang); with the bounded
    /// loop it returns.
    pub(crate) static FORCE_SUPERSEDE_ALWAYS: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };

    /// Test-only ONE-SHOT knob: when armed, the next
    /// [`StoreViewManager::base_view`] iteration advances `store_view_epoch`
    /// INSIDE the manager lock, immediately BEFORE the warm-probe re-reads
    /// the live token — modelling a host mutation that lands after a caller
    /// began its warm probe but before the comparison runs (a mutation that
    /// arrives while this thread was waiting to acquire `state`).
    ///
    /// Drives the warm-hit-revalidation regression: a manager that compared
    /// the cached entry against a token captured BEFORE the lock would match
    /// the stale token and return the now-superseded cached view. Re-reading
    /// the live token after the bump forces a miss → rebuild, so the returned
    /// view's token equals the live (post-mutation) token. Consumed on fire.
    pub(crate) static FORCE_WARM_PROBE_TOKEN_BUMP: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };

    /// Test-only ONE-SHOT knob: when armed, the next `publish_coherent`
    /// call on this thread advances `store_view_epoch` INSIDE the publish
    /// lock, immediately BEFORE the live-token fence (Gate 2) re-reads the
    /// live token — modelling a host mutation that landed between build
    /// completion and publish.
    ///
    /// Drives the publish-decline-must-not-return-stale regression: the
    /// freshly-built view's token no longer matches the live token, so
    /// `publish_coherent` declines it. A manager that RETURNED the declined
    /// (stale) view would hand a warm-cache validator a view the host has
    /// already superseded; the bounded re-loop instead rebuilds against the
    /// now-current token, so the returned view's token equals the live
    /// (post-mutation) token. Consumed on fire.
    pub(crate) static FORCE_PUBLISH_DECLINE_ONCE: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };

    /// Test-only PERSISTENT knob: when armed, EVERY `publish_coherent`
    /// call declines its build through the RESET-fence gate (Gate 1)
    /// WITHOUT advancing any token dimension. The build's snapshot is
    /// internally coherent and its token still equals the live token —
    /// only the reset-generation gate rejects it.
    ///
    /// This is the ONLY way to drive [`StoreViewManager::base_view`] to
    /// exhaust its bounded retry budget and hand back a
    /// [`StoreViewRead::ReturnOnly`] seed whose validation token is NOT
    /// externally superseded relative to the live host (the
    /// `FORCE_SUPERSEDE_*` knobs all bump the epoch, so their `ReturnOnly`
    /// seeds carry a drifted external token a token-only fence already
    /// rejects). It models a sustained `clear()`/reset race: a non-current
    /// seed coexisting with a still-matching external token — exactly the
    /// window where a publish fence that checks only the token, and not
    /// the seed's currentness, would WRONGLY promote a stale result. Stays
    /// armed until disarmed (the bounded retry guarantees termination).
    pub(crate) static FORCE_RESET_FENCE_DECLINE_ALWAYS: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

/// Test-only PROCESS-GLOBAL build gate, used by the woken-waiter
/// regression to deterministically hold a builder inside `build_coherent`
/// (holding the [`StoreViewManager`] singleflight claim) while the test
/// advances the live host token and lines a waiter up behind the claim.
/// Releasing the gate lets the builder publish a token that is ALREADY
/// stale; the waiter then wakes into a token-advanced world and must
/// re-capture the live token rather than returning the builder's
/// now-superseded view.
///
/// Cross-thread (the builder and the test driver are different threads),
/// so it is a `static` condvar, not a thread-local. Disarmed unless a
/// test arms it; production never touches it.
///
/// **Thread opt-in (cross-test safety).** Only a thread that called
/// [`Self::enroll_current_thread`] parks at the gate. Without the opt-in,
/// an unrelated parallel test whose own `build_coherent` happens to reach
/// the gate point while it is armed would be captured by mistake. The
/// regression's spawned builder thread enrolls itself; every other thread
/// in the process treats the gate as inert.
#[cfg(test)]
pub(crate) struct TestBuildGate {
    state: parking_lot::Mutex<TestBuildGateState>,
    cv: parking_lot::Condvar,
}

#[cfg(test)]
thread_local! {
    /// `true` on a thread that opted into the [`TestBuildGate`] via
    /// [`TestBuildGate::enroll_current_thread`]. Only such a thread parks
    /// at the gate; all other threads ignore it.
    static TEST_BUILD_GATE_PARTICIPANT: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
#[derive(Default)]
struct TestBuildGateState {
    /// `true` while the gate is armed to hold the next builder.
    armed: bool,
    /// `true` once a builder has reached the gate and parked.
    builder_parked: bool,
    /// `true` once the test releases the held builder.
    released: bool,
}

#[cfg(test)]
impl TestBuildGate {
    const fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(TestBuildGateState {
                armed: false,
                builder_parked: false,
                released: false,
            }),
            cv: parking_lot::Condvar::new(),
        }
    }

    /// Opt the CURRENT thread into the gate. Only an enrolled thread parks
    /// at the gate; this keeps an unrelated parallel test's builder from
    /// being captured by mistake.
    pub(crate) fn enroll_current_thread(&self) {
        TEST_BUILD_GATE_PARTICIPANT.with(|c| c.set(true));
    }

    /// Arm the gate so the next ENROLLED builder to reach the gate point
    /// parks until [`Self::release`]. Resets the parked/released flags.
    pub(crate) fn arm(&self) {
        let mut s = self.state.lock();
        s.armed = true;
        s.builder_parked = false;
        s.released = false;
        self.cv.notify_all();
    }

    /// Called from inside `build_coherent` (the builder thread). If the
    /// gate is armed AND this thread enrolled, mark the builder parked and
    /// block until released. Consumes the arming (one-shot) so only the
    /// first enrolled builder is held. A non-enrolled thread returns
    /// immediately so unrelated parallel tests are never captured.
    fn wait_if_armed(&self) {
        if !TEST_BUILD_GATE_PARTICIPANT.with(std::cell::Cell::get) {
            return;
        }
        let mut s = self.state.lock();
        if !s.armed {
            return;
        }
        // Consume the arming so a subsequent builder is not held.
        s.armed = false;
        s.builder_parked = true;
        self.cv.notify_all();
        while !s.released {
            self.cv.wait(&mut s);
        }
    }

    /// Block (bounded) until a builder has parked at the gate. Returns
    /// `true` if a builder parked within the timeout.
    pub(crate) fn wait_for_builder_parked(&self, timeout: std::time::Duration) -> bool {
        let mut s = self.state.lock();
        let deadline = std::time::Instant::now() + timeout;
        while !s.builder_parked {
            if self.cv.wait_until(&mut s, deadline).timed_out() {
                return s.builder_parked;
            }
        }
        true
    }

    /// Release the held builder.
    pub(crate) fn release(&self) {
        let mut s = self.state.lock();
        s.released = true;
        self.cv.notify_all();
    }
}

#[cfg(test)]
pub(crate) static TEST_BUILD_GATE: TestBuildGate = TestBuildGate::new();

impl HostStoreView {
    /// Read the host's base store view as a typed [`StoreViewRead`].
    ///
    /// This is the warm-validation chokepoint: a [`StoreViewRead::Current`]
    /// carries a [`CurrentHostStoreView`] the manager proved current, and a
    /// [`StoreViewRead::ReturnOnly`] carries a KNOWN non-current view (with
    /// its [`StoreViewReturnOnlyReason`]). Warm-cache validators call this
    /// and accept ONLY the `Current` arm; a `ReturnOnly` is a cache miss
    /// that falls to the cold path.
    #[track_caller]
    pub(crate) fn from_host_read(host: &VerterHost) -> StoreViewRead {
        record_from_host_call(Location::caller());
        // Per-host measurement rail: hermetic counterpart of the
        // process-global per-call-site attribution table above. O(1)-read
        // batch regressions measure THIS counter so concurrent tests on
        // other hosts can never pollute their reset→measure window.
        host.provenance()
            .store_view_from_host_reads
            .fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        HOST_STORE_VIEW_FROM_HOST_BUILDS.with(|c| c.set(c.get().saturating_add(1)));
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.cache_counters
                .bypass_diagnostics
                .host_store_view_from_host_builds
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        host.store_view_manager().base_view(host)
    }

    /// Build a coherent base snapshot or report supersession (no-torn
    /// return). The final attempt never treats a view whose per-canonical
    /// snapshots could straddle a mutation as publishable. The
    /// [`StoreViewManager`] retries on `Superseded` (a concurrent winner
    /// may have published a coherent view in the meantime), and on
    /// retry-cap exhaustion hands the carried freshest view back
    /// return-only WITHOUT caching it.
    fn build_coherent(host: &VerterHost, session_id: Option<u64>) -> SnapshotBuildOutcome {
        // The freshest view from a superseded attempt, kept so an
        // all-superseded run can hand a built (if incoherent) view back to
        // the caller return-only instead of producing nothing.
        let mut freshest: Option<HostStoreView> = None;
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            // Count every ACTUAL full-workspace sweep (one per `build`
            // attempt). A batch-saturation gate reads this to verify the
            // `StoreViewManager` collapses a warm batch onto ~O(1) sweeps
            // — distinct from the `from_host` call count, which also bumps
            // on the cheap token-stable Arc-clone hits the manager serves
            // without sweeping.
            STORE_VIEW_COHERENT_BUILD_SWEEPS.fetch_add(1, Ordering::Relaxed);
            #[cfg(test)]
            COHERENT_BUILD_SWEEPS_THIS_THREAD.with(|c| c.set(c.get().saturating_add(1)));
            // Test-only: simulate a builder that panics mid-build. The
            // panic must unwind through `StoreViewManager::base_view`'s
            // claim region WITHOUT leaving `building == true` (the RAII
            // claim guard clears it on drop).
            #[cfg(test)]
            if FORCE_BUILD_PANIC.with(|c| {
                let armed = c.get();
                c.set(false);
                armed
            }) {
                panic!("FORCE_BUILD_PANIC: injected mid-build panic");
            }
            // Capture the COMPLETE set of token-relevant raw inputs ONCE,
            // before any per-canonical snapshotting begins, and build the
            // entire snapshot under that single captured token. `build`
            // stamps the view's token-relevant dimensions from this capture
            // (NOT from late live re-reads), so the view's own token equals
            // `pre.token()` exactly — there is no separately-reconstructed
            // "built token" that could straddle a mid-build mutation.
            let pre = PreBuildTokenInputs::capture(host);
            let pre_token = pre.token();
            // Test-only: if a test armed the build gate, park HERE (holding
            // the singleflight claim, having already captured `pre` at the
            // pre-bump token) until the test releases. The test uses this to
            // line a waiter up behind the claim and advance the live token
            // before releasing, deterministically driving the woken-waiter
            // regression. One-shot; only the first gated builder is held.
            #[cfg(test)]
            TEST_BUILD_GATE.wait_if_armed();
            // Test-only: arm the concurrent-sweep gauge across the ACTUAL
            // workspace sweep (`build`). Held only for the build itself (not
            // the gate park above), so the PEAK reflects threads genuinely
            // sweeping in parallel. An UNCLAIMED final-fallback sweep
            // (the defect) overlaps another build and drives the peak > 1;
            // the singleflight claim keeps it at 1.
            #[cfg(test)]
            let _sweep_gauge = ConcurrentSweepGauge::enter();
            let view = Self::build(host, &pre, session_id);
            debug_assert_eq!(
                view.validation_token(),
                pre_token,
                "the built view's token must equal the single pre-build capture \
                 (build must stamp from the capture, not late live reads)"
            );
            // Test-only: force a mid-build mutation so the post-build
            // token capture differs from the pre-build one, exercising
            // the supersession retry without a racing thread. The
            // decrementing-counter knob exhausts after N attempts; the
            // persistent knob churns on EVERY attempt (modelling a host
            // whose token never settles) to drive the bounded-retry
            // liveness gate.
            #[cfg(test)]
            if FORCE_SUPERSEDE_ATTEMPTS.with(|c| {
                let remaining = c.get();
                if remaining > 0 {
                    c.set(remaining - 1);
                    true
                } else {
                    false
                }
            }) || FORCE_SUPERSEDE_ALWAYS.with(std::cell::Cell::get)
            {
                host.bump_store_view_epoch();
            }
            // Compare the single PRE-build captured token against a fresh
            // live capture. Because the view was stamped entirely from
            // `pre`, this detects ANY mid-build advance of ANY token
            // dimension (epoch, the additive artifact / load generations,
            // the content generation, env fold,
            // project identity, project generation) — including a dimension
            // that moved WITHOUT bumping `store_view_epoch` — and forces a
            // retry / `Superseded` rather than publishing a torn view whose
            // snapshot maps and token straddle the mutation.
            let live_token = StoreViewValidationToken::capture(host);
            if pre_token == live_token {
                return SnapshotBuildOutcome::Coherent {
                    view,
                    token: pre_token,
                };
            }
            // Not coherent: the host moved across the build. Keep this
            // attempt's view as the freshest candidate to hand back if the
            // retry cap is exhausted; the next iteration tries for a
            // coherent capture.
            freshest = Some(view);
        }
        // Every attempt was superseded. Return the freshest built view. The
        // manager NEVER caches this view (its stamped token is stale); on
        // retry-cap exhaustion it hands it back return-only.
        let view = freshest
            .expect("STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS >= 1 guarantees at least one built view");
        SnapshotBuildOutcome::Superseded { view }
    }

    /// Build a session-scoped store view from a raw session id.
    ///
    /// The compat token includes the session identity so that two sessions
    /// with different overlays but the same epoch never coalesce into the
    /// same singleflight lane.
    ///
    /// This entry point replaces an earlier `from_session(view: &SessionView,
    /// host)` overload. The old overload took a session-scoped
    /// `SessionView` epoch carrier; under R17 the per-session
    /// overlay-mutation machinery is gone, so the singleflight
    /// lane identity is the raw `session_id` plumbed through the
    /// caller; the runtime-side epoch carrier no longer exists.
    pub(crate) fn from_session_id(session_id: u64, host: &VerterHost) -> Self {
        Self::from_session_id_read(session_id, host).0
    }

    /// Build a session-scoped store view together with the manager's
    /// currentness proof: `true` iff a coherent `build_coherent` produced
    /// the view, `false` iff the host churned and the fallback returned a
    /// known-stale base-derived snapshot.
    ///
    /// Session-scoped views carry per-session overlay identity, so they are
    /// not manager-cached (the manager caches the base workspace snapshot
    /// keyed by the base token); but they still get the no-torn-return
    /// guarantee. On supersession the host is genuinely contended — fall
    /// back to the manager's bounded base build re-scoped to this session
    /// id rather than risk a torn capture. `base_view` is itself bounded:
    /// under sustained churn it returns `ReturnOnly` in bounded time, so
    /// this fallback never spins; a `ReturnOnly` base is reported as
    /// non-current so the stable-request driver suppresses its warm peek.
    pub(crate) fn from_session_id_read(session_id: u64, host: &VerterHost) -> (Self, bool) {
        match Self::build_coherent(host, Some(session_id)) {
            SnapshotBuildOutcome::Coherent { view, .. } => (view, true),
            SnapshotBuildOutcome::Superseded { .. } => {
                let (mut base, is_current) = match host.store_view_manager().base_view(host) {
                    StoreViewRead::Current(current) => (current.view().clone(), true),
                    StoreViewRead::ReturnOnly { view, .. } => (view, false),
                };
                base.session_id = Some(session_id);
                base.compat_token = base.compute_compat_token();
                (base, is_current)
            }
        }
    }

    /// Drop every per-canonical / per-domain snapshot for a
    /// session-deleted (tombstoned) canonical — there is no current
    /// content for it. Removing its `whole_hashes`, `file_facts`, and
    /// `derived_hashes` entries makes strict validation reject any warm
    /// entry rooted on the now-deleted file (`validates_self_root_whole_hash`
    /// rejects an untracked self-root; `validates_parse_domain` rejects
    /// a real fact hash for an untracked file; the `derived_hashes`
    /// validators reject an absent entry), so the consumer recomputes.
    ///
    /// The canonical is also recorded in [`Self::tombstoned_canonicals`].
    /// Removal from `whole_hashes` alone makes the canonical look
    /// *untracked* to the lazy [`StoreView::validates`] `FileWholeHash`
    /// / `DirectSource` arms — whose untracked branch optimistically
    /// accepts a genuine cross-file dependency loaded after the view
    /// snapshot. A tombstoned canonical is a *deleted* file, not a
    /// genuinely-untracked dependency, so a cross-file `FileWholeHash`
    /// dependency on it MUST be rejected; the tombstone set lets
    /// `validates` distinguish the two.
    fn drop_tombstoned_canonical_snapshots(snapshot: &mut StoreViewSnapshot, canonical: &str) {
        snapshot.whole_hashes.remove(canonical);
        snapshot.file_facts.remove(canonical);
        for kind in [
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::ImportRoute,
            crate::resolver_core::DerivedFactKind::DirectSource,
        ] {
            snapshot
                .derived_hashes
                .remove(&(canonical.to_owned(), kind));
        }
        snapshot.tombstoned_canonicals.insert(canonical.to_owned());
    }

    /// Re-root this view against a [`SessionView`]'s overlay so
    /// warm-read validation observes the session's CURRENT content
    /// identity rather than the base host's — across **every**
    /// per-canonical / per-domain snapshot, not just `whole_hashes`.
    ///
    /// `HostStoreView::build` snapshots every per-canonical field from
    /// the scheduler / `FileArtifactStore` — i.e. the **base** content
    /// of every tracked canonical. A query executed under a
    /// [`crate::resolver_core::SessionResolverContext`] roots its
    /// cached values (semantic-graph `MemoEntry` self-roots, the
    /// path-precise fact rail, the legacy whole-hash rail) on the
    /// **overlay** content for every overlay-bearing canonical —
    /// `ensure_indexed_ready_serve` under a session resolves the overlay
    /// `IndexedReady`, and parse facts pin to the overlay content
    /// version. A warm read whose validation routed through the base
    /// view would compare overlay-rooted facts against base snapshots
    /// and miss on every call.
    ///
    /// Per-canonical / per-domain field treatment for the session's
    /// overlay canonicals:
    ///
    /// - **`whole_hashes`** — overlay-Upsert: set to
    ///   [`SessionView::overlay_content_hash_for`]; tombstone: removed.
    ///   The self-root `FileWholeHash` validator (`validates` /
    ///   `validates_self_root_whole_hash`) and the `DirectSource`
    ///   `DerivedFactHash` arm read this map; re-rooting it closes
    ///   them. It is also the `content_hash` dimension the
    ///   `resolve-imports` validator composes its
    ///   `ResolvedImportFactsKey` from, so re-rooting steers that
    ///   content-addressed `DashMap` lookup at the overlay slot.
    /// - **`file_facts`** — overlay-Upsert: refreshed from the overlay
    ///   `FileArtifacts` (via
    ///   [`OverlayArtifactIdentity::lookup_overlay_artifacts`](crate::host_manage::overlay_materialize::OverlayArtifactIdentity::lookup_overlay_artifacts),
    ///   which rebuilds the exact overlay-scoped key — raw-owner hash +
    ///   discriminator, normalised analysis canonical — and is
    ///   content-pinned); tombstone: removed.
    ///   `validates_parse_domain` reads this per-canonical
    ///   `Arc<FileFacts>` snapshot — a `Parse` fact pinned to the
    ///   overlay version validates against the overlay's `FileFacts`.
    /// - **`derived_hashes`** (`Route` / `ImportRoute`) — overlay-Upsert:
    ///   refreshed from the overlay `IndexedReady`
    ///   (`hash_route_surface` over the overlay `shallow_state`, and the
    ///   overlay `import_route_hash`); tombstone: removed alongside the
    ///   `DirectSource` entry. `validates` reads these per-`(canonical,
    ///   kind)` hashes; refreshing keeps an overlay-rooted
    ///   `DerivedFactHash` validating against overlay content.
    /// - **`resolved_import_facts`** / **`route_db`** — `Arc` clones of
    ///   the project store's content-addressed `DashMap`s. They are
    ///   shared and hold both the base and the overlay candidates; the
    ///   overlay candidate is reached because `whole_hashes` (the
    ///   `content_hash` key dimension) is re-rooted above. No
    ///   per-canonical re-root needed on the handle itself.
    /// - **`resolved_import_facts_known_miss_tags`** — the
    ///   `known_miss_generation` key dimension is generation-scoped, not
    ///   content-scoped; a pure overlay content edit does not advance
    ///   the project generation, so the base snapshot is correct.
    /// - **`route_surface_index_fingerprints`** — keyed by the
    ///   structural augmentation-target shape PLUS the augmentation
    ///   `population` dimension, not by canonical / content hash. The
    ///   snapshot mirrors the store's overlay-aware augmentation index:
    ///   it carries both the `(target, Base) → base_fp` and the
    ///   `(target, Session(fp)) → session_fp` entries, so a session
    ///   read's `EffectiveExportSet` consumer validates against the
    ///   session fingerprint and a base read against the base one. The
    ///   base snapshot is carried unchanged because population
    ///   discrimination lives in the key, not in a per-canonical
    ///   re-root: the validator composes [`Self::augmentation_population`]
    ///   (derived from the snapshot's `session_overlay_fingerprint`,
    ///   re-rooted below) so it selects the correct-population slot.
    /// - **`snapshot.session_overlay_fingerprint`** — re-rooted from
    ///   `view.fingerprint()` so [`Self::augmentation_population`]
    ///   reports `Session(fingerprint)` for this view; the route-surface
    ///   validator composes the matching population slot.
    /// - **`env_hashes`** / **`project_identity`** / **`project_generation`**
    ///   / **`compat_token`** / **`mutation_epoch`** / **`session_id`** —
    ///   view-level identity, not per-canonical content; untouched.
    ///
    /// The override is **not** a blanket accept: every refreshed
    /// snapshot validates against the session's CURRENT overlay
    /// content. An entry rooted on a *superseded* overlay version, or
    /// on the *base* content while an overlay now covers the canonical,
    /// still misses — exactly as the un-overlaid view validates against
    /// the base's current content.
    ///
    /// A canonical the session TOMBSTONED (overlay-Deleted) has its
    /// base per-canonical snapshots dropped — see
    /// [`Self::drop_tombstoned_canonical_snapshots`]. Tombstones are
    /// reported by [`SessionView::tombstoned_canonicals`], iterated
    /// independently of [`SessionView::overlay_canonicals`]: a session
    /// can delete a file without re-upserting it (so it has no overlay
    /// source), while a canonical re-upserted after a delete appears in
    /// `overlay_canonicals` and is treated as an overlay-Upsert.
    ///
    /// Non-overlay, non-tombstoned canonicals are untouched — they keep
    /// their base snapshots, so a session that overlays or deletes one
    /// file still validates every other canonical against base content.
    ///
    /// ## Copy-on-write
    ///
    /// The shared base `Arc<StoreViewSnapshot>` is **never mutated in
    /// place**. The first overlay/tombstone re-root clones the inner
    /// snapshot via `Arc::make_mut`, leaving the manager-cached base
    /// pristine for concurrent base readers. A view with no overlay
    /// canonicals and no tombstones returns the shared `Arc` untouched.
    /// The overlay identity (session id + a structural fingerprint of
    /// the masked canonical set) is folded into the validation token so
    /// two requests with different completion/session overlays carry
    /// distinct token identities.
    #[must_use]
    pub(crate) fn with_session_overlay(
        mut self,
        host: &VerterHost,
        view: &dyn crate::session_view::SessionView,
    ) -> Self {
        let tombstones: Vec<String> = view.tombstoned_canonicals();
        let overlay_canonicals: Vec<String> = view.overlay_canonicals();

        // Structural overlay fingerprint: fold the
        // masked canonical set + per-canonical overlay content hash. Two
        // sessions whose overlay shapes differ — or one whose overlaid
        // file content differs — produce distinct fingerprints, so their
        // validation tokens never collide.
        //
        // This is the VALIDATION-TOKEN overlay identity — one of TWO
        // overlay-set folds in this crate. The other,
        // `session_view.rs::overlay_set_fingerprint`, is the memoized
        // `u64` behind the `SessionView::fingerprint` surface (cache-key
        // derivation, augmentation-population identity). Different
        // surfaces, layouts, and output widths (`Hash16` with
        // tombstone/upsert domain markers here); deliberately NOT unified
        // — changing either fold changes that surface's identity values.
        let overlay_fingerprint = hash16_from_sorted(|hasher| {
            let mut tombstone_sorted = tombstones.clone();
            tombstone_sorted.sort_unstable();
            for canonical in &tombstone_sorted {
                0u8.hash(hasher);
                canonical.hash(hasher);
            }
            let mut overlay_sorted: Vec<(&String, Option<Hash16>)> = overlay_canonicals
                .iter()
                .map(|c| (c, view.overlay_content_hash_for(c)))
                .collect();
            overlay_sorted.sort_by(|a, b| a.0.cmp(b.0));
            for (canonical, content_hash) in overlay_sorted {
                1u8.hash(hasher);
                canonical.hash(hasher);
                content_hash.hash(hasher);
            }
        });
        self.overlay_identity = Some(OverlayIdentity {
            session_id: self.session_id,
            overlay_fingerprint,
        });

        // Copy-on-write: only clone the inner snapshot when there is at
        // least one overlay/tombstone canonical to apply. A
        // no-op-overlay session keeps the shared base `Arc`.
        //
        // A view with no overlays and no tombstones has
        // `view.fingerprint() == 0` (`overlay_set_fingerprint` returns 0
        // only when the overlay map AND the tombstone set are both
        // empty), so the snapshot's `session_overlay_fingerprint` stays
        // at its default 0 — `augmentation_population()` reports `Base`,
        // matching the producer's `augmentation_population_for_view`
        // derivation. Any view with a non-zero fingerprint necessarily
        // has at least one overlay or tombstone canonical, so it passes
        // the fast-path below and records its fingerprint on the
        // copy-on-written snapshot.
        if tombstones.is_empty() && overlay_canonicals.is_empty() {
            // The overlay identity changed (set above), so the complete
            // validation token — hence the coalescing-lane fingerprint —
            // must be recomputed before this view participates in any
            // singleflight/stability lane.
            self.compat_token = self.compute_compat_token();
            return self;
        }
        // The overlay RE-ROOT path: `Arc::make_mut` clones the shared
        // `StoreViewSnapshot` when the `Arc` is actually shared
        // (refcount > 1 — e.g. across batch jobs) and mutates a
        // uniquely-owned snapshot in place; either way the per-canonical
        // re-rooting below runs. The counter bumps on every entry into
        // this path — an upper bound on full clones, and exactly the
        // per-application work the O(1) batch contract bounds: the batch
        // path must reach here ONCE per batch (the per-batch capture), not
        // once per job — the PER-HOST counter gates that contract.
        //
        // The counter lives on `host.provenance()` (not a process-global
        // static): every worker in a host batch overlays through the SAME
        // host, so this bump is observed regardless of which rayon worker
        // performs the COW, while remaining isolated from other hosts'
        // (other tests') overlay activity — the measurement is hermetic by
        // construction.
        host.provenance()
            .session_overlay_cows
            .fetch_add(1, Ordering::Relaxed);
        let snapshot = Arc::make_mut(&mut self.snapshot);

        // Capture the session's overlay-set fingerprint on the COW
        // snapshot — the augmentation-index population identity for this
        // view. `overlay_set_fingerprint` hashes tombstones too and
        // returns `0` only when overlays AND tombstones are both empty,
        // so every view reaching this point — tombstone-only included —
        // carries a non-zero fingerprint and reports
        // `AugmentationPopulation::Session(fingerprint)`; the
        // route-surface validator composes that population, matching the
        // producer's `augmentation_population_for_view` derivation.
        snapshot.session_overlay_fingerprint = view.fingerprint();

        // Tombstone-only canonicals: deleted by the session and never
        // re-upserted, so absent from `overlay_canonicals()`. This is
        // the delete-case analogue of the overlay-Upsert re-rooting
        // below — without it a warm entry rooted on a session-deleted
        // file's BASE content would still validate.
        for canonical in &tombstones {
            Self::drop_tombstoned_canonical_snapshots(snapshot, canonical);
        }

        for canonical in &overlay_canonicals {
            if view.is_tombstoned(canonical) {
                // Both an overlay-source key AND tombstoned — the
                // tombstone wins over a stale overlay-source entry.
                Self::drop_tombstoned_canonical_snapshots(snapshot, canonical);
                continue;
            }
            let Some(overlay_hash) = view.overlay_content_hash_for(canonical) else {
                continue;
            };
            // Re-root the self-root whole-hash rail.
            snapshot
                .whole_hashes
                .insert(canonical.clone(), overlay_hash);

            // Refresh the per-domain parse-fact + derived-fact
            // snapshots from the overlay artifact. `canonical` is the
            // RAW overlay owner (from `overlay_canonicals()`);
            // `lookup_overlay_artifacts` builds the exact overlay
            // artifact key — the raw-owner overlay hash + discriminator
            // with the NORMALISED `analysis_canonical` as
            // `FileArtifactKey.canonical` — so it returns the overlay
            // `FileArtifacts` candidate (not the base one) even when
            // `normalize(raw) != raw`.
            let overlay_artifact_identity = host.overlay_artifact_identity(canonical);
            match overlay_artifact_identity.lookup_overlay_artifacts(host, view) {
                Some(overlay_artifacts) => {
                    snapshot.file_facts.insert(
                        canonical.clone(),
                        std::sync::Arc::clone(&overlay_artifacts.facts),
                    );
                    let overlay_indexed = &overlay_artifacts.indexed;
                    // Edge-currency gate. A wildcard-bearing overlay surface
                    // bakes its `export *` edge `canonical_id`s from the
                    // dependency file set; once `content_generation` advances
                    // past its edge generation BOTH the route-surface hash and
                    // the import-route hash are stale. Suppress both derived
                    // hashes (the same outcome as an unmaterialised overlay
                    // artifact below) so a warm entry rooted on them fails
                    // validation and recomputes through the edge-gated readers,
                    // which re-materialise the overlay surface — rather than
                    // copying a stale hash into the view.
                    let edge_current = host.indexed_surface_is_current(canonical, overlay_indexed);
                    if overlay_indexed.shallow_state.has_resolvable_surface() && edge_current {
                        snapshot.derived_hashes.insert(
                            (
                                canonical.clone(),
                                crate::resolver_core::DerivedFactKind::Route,
                            ),
                            hash_route_surface(&overlay_indexed.shallow_state),
                        );
                    } else {
                        snapshot.derived_hashes.remove(&(
                            canonical.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ));
                    }
                    match overlay_indexed.import_route_hash {
                        Some(hash) if edge_current => {
                            snapshot.derived_hashes.insert(
                                (
                                    canonical.clone(),
                                    crate::resolver_core::DerivedFactKind::ImportRoute,
                                ),
                                hash,
                            );
                        }
                        _ => {
                            snapshot.derived_hashes.remove(&(
                                canonical.clone(),
                                crate::resolver_core::DerivedFactKind::ImportRoute,
                            ));
                        }
                    }
                }
                None => {
                    // The overlay artifact has not been materialised
                    // yet. The base per-domain snapshots are stale
                    // relative to the overlay content; drop them so
                    // `validates_parse_domain` / the `DerivedFactHash`
                    // validator reject any entry rooted on the overlay
                    // and the consumer cold-recomputes (the correct R3
                    // outcome under stale producer state — same shape
                    // as an absent base snapshot).
                    snapshot.file_facts.remove(canonical);
                    snapshot.derived_hashes.remove(&(
                        canonical.clone(),
                        crate::resolver_core::DerivedFactKind::Route,
                    ));
                    snapshot.derived_hashes.remove(&(
                        canonical.clone(),
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    ));
                }
            }
        }
        // The overlay re-rooted the snapshot AND set a new overlay
        // identity, both of which feed the complete validation token.
        // Recompute the coalescing-lane fingerprint so an overlaid view
        // never shares a singleflight/stability lane with the base (or a
        // differently-overlaid) view.
        self.compat_token = self.compute_compat_token();
        self
    }

    /// Augmentation-index population identity for this view (overlay
    /// isolation). Mirrors the SINGLE derivation in
    /// [`crate::session_view::augmentation_population_for_view`]: a
    /// non-zero overlay-set fingerprint means `Session(fingerprint)`,
    /// otherwise `Base`. The route-surface validator composes the
    /// matching population so base and session augmenter-set
    /// fingerprints never cross-validate.
    fn augmentation_population(&self) -> crate::file_artifact_store::AugmentationPopulation {
        use crate::file_artifact_store::AugmentationPopulation;
        if self.snapshot.session_overlay_fingerprint != 0 {
            AugmentationPopulation::Session(self.snapshot.session_overlay_fingerprint)
        } else {
            AugmentationPopulation::Base
        }
    }

    /// The validation token under which this view was built. The base
    /// token is captured by [`StoreViewManager`]; a session-overlaid
    /// view re-derives it from the shared snapshot + its frozen overlay
    /// identity so the token reflects the overlay.
    pub(crate) fn validation_token(&self) -> StoreViewValidationToken {
        StoreViewValidationToken {
            store_view_epoch: self.mutation_epoch,
            project_generation: self.snapshot.project_generation,
            artifact_generation: self.artifact_generation,
            load_generation: self.load_generation,
            content_generation: self.content_generation,
            env_hash_fold: fold_env_hashes(&self.snapshot.env_hashes),
            project_identity: self.snapshot.project_identity,
            overlay_identity: self.overlay_identity,
        }
    }

    fn build(host: &VerterHost, pre: &PreBuildTokenInputs, session_id: Option<u64>) -> Self {
        // EVERY token-relevant by-value dimension comes from the single
        // `pre` capture taken BEFORE the per-canonical snapshot population
        // window opened. They are NEVER re-read live here — re-reading any
        // of them late (after the snapshot maps were populated) would let a
        // mid-build mutation that advanced a dimension WITHOUT bumping
        // `store_view_epoch` produce a view whose token reflects the NEW
        // value while its snapshot maps were captured under the OLD value;
        // `build_coherent`'s post-build coherence check (which compares the
        // PRE-build captured token against the live token) would then accept
        // that TORN view as coherent. Stamping every dimension from `pre`
        // keeps the snapshot maps and the token coherent under one read
        // window; any mid-build advance is caught by the post-build
        // comparison and forces a retry / `Superseded`.
        let snapshot_epoch = pre.store_view_epoch;
        let artifact_generation = pre.artifact_generation;
        let load_generation = pre.load_generation;
        let content_generation = pre.content_generation;
        let mut snapshot = StoreViewSnapshot::default();

        {
            let mut canonical_ids = host.scheduler.node_ids();
            canonical_ids.extend(host.compile_cache().iter().map(|entry| entry.key().clone()));
            canonical_ids.sort();
            canonical_ids.dedup();

            for canonical_id in canonical_ids {
                if let Some(source) = host.scheduler.try_get_source(&canonical_id) {
                    snapshot
                        .whole_hashes
                        .insert(canonical_id.clone(), source.whole_hash);
                }

                if !snapshot.whole_hashes.contains_key(&canonical_id) {
                    if let Some(state) = host.effective_file_state(&canonical_id, None) {
                        snapshot
                            .whole_hashes
                            .insert(canonical_id.clone(), state.whole_hash);
                    }
                }

                // The known-miss generation sidecar
                // lives on DerivedRawState (D48 split);
                // capture it so the validator can compose
                // `ResolvedImportFactsKey.known_miss_generation`
                // identically to the producer. The per-specifier
                // `import_routes` themselves are NOT snapshotted: no
                // `HostStoreView` validator reads them — the
                // import-route domain validates through `derived_hashes`
                // (`ImportRoute` kind) and the content-addressed
                // `resolved_import_facts` handle instead.
                if let Some(entry) = host.derived_raw_cache().get(&canonical_id) {
                    let tag = crate::resolved_import_facts::compute_known_miss_generation_tag(
                        &entry.import_routes_known_miss_recorded_at_generation,
                    );
                    snapshot
                        .resolved_import_facts_known_miss_tags
                        .insert(canonical_id.clone(), tag);
                }
            }
        }

        // WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

        // Snapshot FileArtifactStore entries into the store view. The
        // `IndexedReady` artifact is the SOLE route-surface source —
        // identical to the producer (`current_route_surface_hash`), so
        // producer and validator stay on one source order.
        for (canonical_id, indexed) in host.project_type_store.indexed().snapshot_all() {
            let canonical_str = canonical_id.as_ref().to_owned();
            // The tracked current whole hash for this canonical: the
            // value seeded earlier from `effective_file_state`, or — for
            // an artifact-only canonical the single authority gate
            // accepts — `indexed.whole_hash`. A canonical with NO
            // scheduler state that fails the gate (absent file,
            // scheduler-superseded leftover) contributes NOTHING: the
            // accessors reject it, so manufacturing a tracked hash from
            // the artifact itself would let stale
            // FileWholeHash/Route/file facts validate against state no
            // read path will serve.
            let tracked_whole_hash = match snapshot.whole_hashes.get(&canonical_str) {
                Some(tracked) => *tracked,
                None => {
                    if !host
                        .artifact_only_candidate_is_fresh(&canonical_str, indexed.edge_generation)
                    {
                        continue;
                    }
                    snapshot
                        .whole_hashes
                        .insert(canonical_str.clone(), indexed.whole_hash);
                    indexed.whole_hash
                }
            };
            // A current-content `IndexedReady` (`indexed.whole_hash ==
            // tracked`) is the route-surface authority for this
            // canonical. The `Route` derived fact is contributed only
            // when the current indexed surface is route-resolvable AND
            // edge-current: a wildcard-bearing artifact whose baked
            // `export *` edges are stale (a dependency appeared /
            // retargeted while this file's content stayed put) must not
            // contribute its stale `Route` hash, so a warm entry rooted
            // on the stale hash recomputes.
            if indexed.whole_hash == tracked_whole_hash
                && host.indexed_surface_is_current(&canonical_str, &indexed)
                && indexed.shallow_state.has_resolvable_surface()
            {
                snapshot.derived_hashes.insert(
                    (
                        canonical_str.clone(),
                        crate::resolver_core::DerivedFactKind::Route,
                    ),
                    hash_route_surface(&indexed.shallow_state),
                );
            }
            // The `ImportRoute` derived fact must reflect the
            // generation-current import-target surface. A file with
            // an unresolvable specifier carries a known-miss in its
            // content-pinned `IndexedReady.import_route_hash`; that
            // snapshot would otherwise be served unchanged after a
            // new file satisfies the specifier (the importer's
            // content, hence its `IndexedReady`, does not change), so
            // a dependent cache entry would validate against a stale
            // miss. `generation_current_import_route_hash`
            // re-resolves the miss specifiers against the current
            // workspace so the validator observes the appearance.
            if let Some(hash) = host.generation_current_import_route_hash(&canonical_str) {
                snapshot.derived_hashes.insert(
                    (
                        canonical_str,
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    ),
                    hash,
                );
            }
        }

        Self::snapshot_tracked_import_route_hashes(&mut snapshot, host);
        Self::snapshot_augmentation_index_into(&mut snapshot, host.project_type_store.indexed());
        Self::snapshot_file_facts_into(&mut snapshot, host.project_type_store.indexed());
        // R26 per-domain producer handles captured at view-build
        // time. Cheap `Arc::clone` per snapshot; reads through the
        // handles are wait-free against concurrent writers because
        // both `ResolvedImportFactsDb` and `RouteDb` shard by key
        // (DashMap-backed).
        snapshot.resolved_import_facts = Some(std::sync::Arc::clone(
            host.project_type_store.resolved_import_facts_handle(),
        ));
        snapshot.route_db = Some(host.project_type_store.routes_handle());

        // Test-only: inject an env-hash mutation HERE — after every
        // per-canonical snapshot map was populated under `pre`'s env, but
        // before the token-relevant env / identity dimensions are stamped.
        // The mutation advances `resolve_env_hash` WITHOUT bumping
        // `store_view_epoch`, deterministically reproducing the mid-build
        // non-epoch dimension change. Because the stamps below read from
        // `pre` (NOT live), the view's token reflects the OLD env while the
        // post-build live token reflects the NEW env → the coherence check
        // mismatches and the attempt is treated as superseded. (Were the
        // stamps to re-read live env here, the view's token would also
        // reflect the NEW env and the torn view would be accepted.)
        #[cfg(test)]
        if FORCE_MID_BUILD_ENV_BUMP.with(|c| {
            let armed = c.get();
            c.set(false);
            armed
        }) {
            host.ws()
                .set_default_resolve_extensions(vec![".zzzmidbuildext".to_string()]);
        }

        // R21 env-hash + project-identity + project-generation capture,
        // taken from the single `pre` read window (NOT re-read live here)
        // so the snapshot maps and the validation token stay coherent under
        // one token. Required for `ResolvedImportFactsKey` +
        // `EffectiveExportSetKey` composition inside the per-domain
        // validators (env / identity) and the
        // `FactVersionRef::ProjectGeneration` validator (project
        // generation) — a warm read rejects a value rooted on a superseded
        // generation.
        snapshot.env_hashes = pre.env_hashes;
        snapshot.project_identity = pre.project_identity;
        snapshot.project_generation = pre.project_generation;

        let mut view = Self {
            // Interim placeholder — `compute_compat_token()` below recomputes
            // the lane identity (including `validity_fingerprint`) once the
            // view's snapshot + generations are in place.
            compat_token: crate::resolver_core::StoreViewCompatToken {
                epoch: snapshot_epoch,
                session: session_id,
                validity_fingerprint: 0,
            },
            mutation_epoch: snapshot_epoch,
            session_id,
            snapshot: Arc::new(snapshot),
            overlay_identity: None,
            artifact_generation,
            load_generation,
            content_generation,
        };
        view.compat_token = view.compute_compat_token();
        view
    }

    /// Snapshot `Arc<FileFacts>` per canonical from the indexed
    /// store. One refcount bump per tracked file at view-build time;
    /// parse-domain validation reads through these handles
    /// wait-free against concurrent writers because each entry is
    /// immutable.
    ///
    /// If multiple `(content_hash, parse_env_hash)` variants coexist
    /// for one canonical (the multi-candidate cache shape under R20),
    /// the first one encountered wins — subsequent variants do not
    /// overwrite. The view's `whole_hashes` map records the canonical
    /// content hash; a path-precise consumer that observed against
    /// a parse-env-hash variant outside this snapshot will miss
    /// validation and recompute against the current variant.
    fn snapshot_file_facts_into(
        snapshot: &mut StoreViewSnapshot,
        store: &crate::file_artifact_store::FileArtifactStore,
    ) {
        // Snapshot ONLY the `FileFacts` variant whose `content_hash`
        // matches the view's tracked `whole_hashes[canonical]` —
        // that is the source-of-truth content hash for the
        // canonical under this view. Other variants (stale
        // candidates from prior content generations) coexist in
        // the multi-candidate store per R20 but must NOT back the
        // parse-domain validator: a path-precise consumer observed
        // against the live content, so its validation MUST consult
        // the live content's facts.
        //
        // When the artifact store has not yet been refreshed for
        // the new content (lazy `ensure_indexed_ready_serve` has not run
        // yet), the `file_facts` entry for that canonical stays
        // ABSENT. The parse-domain validator interprets absence as
        // a miss (`validates_parse_domain` returns `false` for any
        // observed real-hash fact under an absent entry) — the
        // consumer falls through to cold recompute, which is the
        // correct R3 outcome under stale producer state.
        for (key, artifacts) in store.snapshot_artifacts() {
            let canonical_str = key.canonical.as_ref().to_owned();
            let matches_live = match snapshot.whole_hashes.get(&canonical_str) {
                Some(h) => key.content_hash == *h,
                None => false,
            };
            if matches_live {
                snapshot
                    .file_facts
                    .insert(canonical_str, std::sync::Arc::clone(&artifacts.facts));
            }
        }
    }

    fn snapshot_tracked_import_route_hashes(snapshot: &mut StoreViewSnapshot, host: &VerterHost) {
        let canonical_ids: Vec<String> = snapshot.whole_hashes.keys().cloned().collect();
        let empty_import_routes = FxHashMap::default();
        let empty_import_route_hash = hash_import_route_targets(&empty_import_routes);

        for canonical_id in canonical_ids {
            if snapshot.derived_hashes.contains_key(&(
                canonical_id.clone(),
                crate::resolver_core::DerivedFactKind::ImportRoute,
            )) {
                continue;
            }

            // Generation-current `ImportRoute` fact for files not
            // covered by the `IndexedReady` snapshot loop above —
            // re-resolves known-miss specifiers against the current
            // workspace so a previously-unresolvable dependency's
            // appearance is observable by the validator.
            let import_route_hash = host.generation_current_import_route_hash(&canonical_id);

            snapshot.derived_hashes.insert(
                (
                    canonical_id.clone(),
                    crate::resolver_core::DerivedFactKind::ImportRoute,
                ),
                import_route_hash.unwrap_or(empty_import_route_hash),
            );
        }
    }

    /// `build`-time variant operating on the under-construction
    /// [`StoreViewSnapshot`] (R29 + G1).
    fn snapshot_augmentation_index_into(
        snapshot: &mut StoreViewSnapshot,
        artifact_store: &crate::file_artifact_store::FileArtifactStore,
    ) {
        for (key, fingerprint) in artifact_store.snapshot_augmentation_index_fingerprints() {
            let snap_key = RouteSurfaceIndexShapeKey {
                target_kind_tag: augmentation_target_kind_tag_for(&key.target),
                external_specifier: augmentation_target_external_specifier(&key.target),
                resolved_relative_canonical: augmentation_target_resolved_relative_canonical(
                    &key.target,
                ),
                wildcard_pattern: augmentation_target_wildcard_pattern(&key.target),
                // Carry the population from the store's
                // `AugmentationTargetKey` so base and session
                // fingerprints stay in distinct snapshot slots.
                population: key.population,
            };
            snapshot
                .route_surface_index_fingerprints
                .insert(snap_key, fingerprint);
        }
    }

    /// Epoch dimension of this view's snapshot. Test-only accessor: the
    /// production stable-promotion / view-liveness decisions now gate on
    /// the COMPLETE external-supersession token
    /// (`current_external_supersession_fingerprint`), not the epoch alone.
    #[allow(dead_code)]
    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    #[allow(dead_code)]
    pub(crate) fn whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.snapshot.whole_hashes.get(canonical_id).copied()
    }

    #[allow(dead_code)]
    pub(crate) fn derived_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        self.snapshot
            .derived_hashes
            .get(&(canonical_id.to_string(), kind))
            .copied()
    }

    pub(crate) fn invalid_fact_details(
        &self,
        facts: &[crate::resolver_core::FactVersionRef],
        limit: usize,
    ) -> Vec<String> {
        facts
            .iter()
            .filter_map(|fact| self.describe_invalid_fact(fact))
            .take(limit)
            .collect()
    }

    fn describe_invalid_fact(&self, fact: &crate::resolver_core::FactVersionRef) -> Option<String> {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                match self.snapshot.whole_hashes.get(canonical_id) {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "FileWholeHash mismatch canonical={} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "FileWholeHash missing canonical={} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => {
                let current = match kind {
                    crate::resolver_core::DerivedFactKind::DirectSource => {
                        self.snapshot.whole_hashes.get(canonical_id)
                    }
                    _ => self
                        .snapshot
                        .derived_hashes
                        .get(&(canonical_id.clone(), *kind)),
                };
                match current {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "DerivedFactHash mismatch canonical={} kind={kind:?} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "DerivedFactHash missing canonical={} kind={kind:?} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            // R26 per-domain variants — per-domain producers populate
            // the matching stores and produce structured diagnostics
            // there. `HostStoreView` does not observe them directly,
            // so the diagnostic shape is a generic "domain fact not
            // validated yet" string.
            crate::resolver_core::FactVersionRef::Parse(p) => Some(format!(
                "ParseFactRef canonical={} key={:?} lane={:?} expected={:?}",
                p.canonical_id, p.key, p.lane, p.expected_hash
            )),
            crate::resolver_core::FactVersionRef::ResolveImports(r) => Some(format!(
                "ResolveImportsFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
            crate::resolver_core::FactVersionRef::RouteSurface(r) => Some(format!(
                "RouteSurfaceFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
            crate::resolver_core::FactVersionRef::ProjectGeneration { generation } => {
                if self.snapshot.project_generation == *generation {
                    None
                } else {
                    Some(format!(
                        "ProjectGeneration mismatch expected={generation} actual={}",
                        self.snapshot.project_generation
                    ))
                }
            }
        }
    }

    fn compute_compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        crate::resolver_core::StoreViewCompatToken {
            epoch: self.mutation_epoch,
            session: self.session_id,
            // Fold the EXTERNAL-supersession dimensions into the lane
            // identity (the SAME oracle the promotion fence `is_stable`
            // compares). The singleflight / stability coalescing lane hands
            // a leader's stable result to followers WITHOUT per-follower
            // revalidation, so two views may share a lane ONLY if neither
            // externally supersedes the other. `epoch` alone is insufficient
            // — a view's validity can change (env, identity, project /
            // overlay) at an unchanged `store_view_epoch`. The additive
            // artifact / load generations are EXCLUDED: a cold
            // compute advances them as its own work, so folding them would
            // split identical concurrent cold requests across lanes (see
            // `StoreViewValidationToken::lane_fingerprint`).
            validity_fingerprint: self.validation_token().lane_fingerprint(),
        }
    }

    /// Overlay-aware variant of
    /// [`crate::resolver_core::StoreView::validates_resolve_imports_domain`]:
    /// composes the `ResolvedImportFactsKey` against the supplied
    /// `content_hash` rather than `self.snapshot.whole_hashes[canonical]`. Used
    /// by [`crate::resolver_core::RequestStoreView`] when a canonical
    /// was promoted into the per-request completion overlay after the
    /// base view was built. All other key
    /// dimensions (`parse_env_hash`, `resolve_env_hash`,
    /// `resolver_version`, `known_miss_generation`) compose against
    /// the base view's snapshot unchanged.
    pub(crate) fn validates_resolve_imports_domain_for_content_hash(
        &self,
        fact: &crate::resolver_core::ResolveImportsFactRef,
        content_hash: Hash16,
    ) -> bool {
        use verter_semantic::facts::registry::FactLane;
        use verter_semantic::facts::FactKey;
        const ZERO_HASH: Hash16 = [0u8; 16];

        let facts_db = match self.snapshot.resolved_import_facts.as_ref() {
            Some(db) => db,
            None => return false,
        };

        // `known_miss_generation`:
        // captured at view-build time from
        // `DerivedRawState::import_routes_known_miss_recorded_at_generation`.
        // Absent entries → `[0u8; 16]` so an owner that never had
        // `set_import_dependencies` called still composes the same
        // key value the producer admitted under (the producer also
        // reads `[0u8; 16]` when there is no `DerivedRawState`
        // entry yet).
        let known_miss_generation = self
            .snapshot
            .resolved_import_facts_known_miss_tags
            .get(fact.canonical_id.as_str())
            .copied()
            .unwrap_or(ZERO_HASH);

        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: std::sync::Arc::from(fact.canonical_id.as_str()),
            content_hash,
            parse_env_hash: self.snapshot.env_hashes.parse_env_hash,
            resolve_env_hash: self.snapshot.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };

        let facts = match facts_db.get(&key) {
            Some(f) => f,
            // Cache slot absent — the consumer observed a real fact
            // hash but the resolve-imports producer has not yet
            // populated the entry under this view. Reject so the
            // caller recomputes through the producer (which will
            // populate the cache + re-emit).
            None => return fact.expected_hash == ZERO_HASH,
        };

        // Pick the lane that the consumer observed under.
        let pick_lane = |f: &std::sync::Arc<verter_semantic::facts::registry::Fact>| match fact.lane
        {
            FactLane::Semantic => f.semantic_hash,
            FactLane::Display => f.display_hash,
        };

        match &fact.key {
            FactKey::ResolvedImportClause {
                specifier,
                binding,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.import_clauses.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.binding == *binding
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            FactKey::ResolvedReexportBinding {
                specifier,
                source_name,
                target_name,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.reexport_bindings.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.source_name == *source_name
                    && entry.target_name == *target_name
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            // Non-resolve-imports FactKey shapes do not belong to
            // the resolve-imports domain. The dispatch layer routes
            // by `FactDomain` so this arm is defensive.
            _ => false,
        }
    }
}

pub(crate) fn hash_import_route_targets(
    resolutions: &FxHashMap<String, crate::types::DependencyResolution>,
) -> Hash16 {
    let mut entries: Vec<_> = resolutions.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    hash16_from_sorted(|hasher| {
        for (specifier, resolution) in &entries {
            0u8.hash(hasher);
            specifier.hash(hasher);
            resolution
                .resolved_canonical_id
                .clone()
                .or_else(|| resolution.effective_target().map(str::to_string))
                .hash(hasher);
        }
    })
}

pub(crate) fn hash_route_surface(state: &crate::resolver_core::ShallowFileState) -> Hash16 {
    hash16_from_sorted(|hasher| {
        // Hash sorted exports WITH their routing targets. A named
        // reexport bakes a resolved dependency canonical exactly like a
        // wildcard edge does; hashing only the export NAME would leave
        // the `Route` fact blind to a dependency-set retarget (a
        // `.d.ts` companion or a more-specific sibling appearing moves
        // `Reexport.canonical_id` while the owner's content — and the
        // export name set — stays put), so a stale cached route would
        // keep validating against the refreshed surface.
        let mut exports: Vec<(
            &str,
            &crate::resolver_core::shallow_file_state::ExportTarget,
        )> = state
            .exports
            .iter()
            .map(|(name, target)| (name.as_str(), target))
            .collect();
        exports.sort_unstable_by_key(|(name, _)| *name);
        for (name, target) in &exports {
            name.hash(hasher);
            match target {
                crate::resolver_core::shallow_file_state::ExportTarget::Local { symbol_name } => {
                    0u8.hash(hasher);
                    symbol_name.hash(hasher);
                }
                crate::resolver_core::shallow_file_state::ExportTarget::Reexport {
                    source_specifier,
                    original_name,
                    canonical_id,
                    is_type,
                } => {
                    1u8.hash(hasher);
                    source_specifier.hash(hasher);
                    original_name.hash(hasher);
                    canonical_id.hash(hasher);
                    is_type.hash(hasher);
                }
            }
        }

        // Hash wildcard reexport source specifiers in declaration order.
        for wildcard in &state.wildcard_reexports {
            wildcard.source_specifier.hash(hasher);
            wildcard.canonical_id.hash(hasher);
        }

        // Hash sorted import targets — baked dependency canonicals the
        // prepared-decl / bare-name chains traverse; a retarget moves
        // the route surface the same way a reexport retarget does.
        let mut import_targets: Vec<(
            &str,
            &crate::resolver_core::shallow_file_state::ImportTarget,
        )> = state
            .import_targets
            .iter()
            .map(|(name, target)| (name.as_str(), target))
            .collect();
        import_targets.sort_unstable_by_key(|(name, _)| *name);
        for (name, target) in &import_targets {
            name.hash(hasher);
            target.source_specifier.hash(hasher);
            target.imported_name.hash(hasher);
            target.canonical_id.hash(hasher);
        }

        // Hash the file content hash.
        state.whole_hash.hash(hasher);
    })
}

fn hash16_from_sorted(f: impl Fn(&mut rustc_hash::FxHasher)) -> Hash16 {
    let mut left = rustc_hash::FxHasher::default();
    0u8.hash(&mut left);
    f(&mut left);

    let mut right = rustc_hash::FxHasher::default();
    1u8.hash(&mut right);
    f(&mut right);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&left.finish().to_le_bytes());
    out[8..].copy_from_slice(&right.finish().to_le_bytes());
    out
}

/// Map an [`AugmentationTargetKind`] into the parallel-fields shape
/// the parse-domain [`FactKey::ModuleAugmentationIndexShape`] +
/// audit-event variants use.
pub(crate) fn augmentation_target_kind_tag_for(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> verter_semantic::facts::registry::AugmentationTargetKindTag {
    use crate::file_artifact_store::AugmentationTargetKind;
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(_) => {
            AugmentationTargetKindTag::ExternalSpecifier
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(_) => {
            AugmentationTargetKindTag::ResolvedRelativeCanonical
        }
        AugmentationTargetKind::WildcardAmbient(_) => AugmentationTargetKindTag::WildcardAmbient,
        AugmentationTargetKind::GlobalAugmentation => AugmentationTargetKindTag::GlobalAugmentation,
    }
}

pub(crate) fn augmentation_target_external_specifier(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => Some(spec.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_resolved_relative_canonical(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => Some(canon.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_wildcard_pattern(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::WildcardAmbient(pat) => Some(pat.as_ref().to_owned()),
        _ => None,
    }
}

impl crate::resolver_core::StoreView for HostStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.compat_token
    }

    fn validates(&self, fact: &crate::resolver_core::FactVersionRef) -> bool {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                // Session-tombstoned canonical: the file is DELETED in
                // this session. A cross-file `FileWholeHash` dependency
                // on a deleted file is invalid — reject before the lazy
                // untracked-accept rule below. `with_session_overlay`
                // removed the canonical from `whole_hashes`, so without
                // this guard it would fall into the `None => true`
                // untracked branch and a parent entry depending on the
                // deleted file would still validate.
                if self.snapshot.tombstoned_canonicals.contains(canonical_id) {
                    return false;
                }
                match self.snapshot.whole_hashes.get(canonical_id) {
                    Some(current) => current == hash,
                    // File not tracked by this store view — it was loaded as a
                    // dependency AFTER the view snapshot was taken. Accept it:
                    // the facts were just materialized from current disk/workspace
                    // state and are valid. This avoids forcing every dependency
                    // access through the expensive permissive fallback path in
                    // `ensure_indexed_ready_serve`.
                    None => true,
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                crate::resolver_core::DerivedFactKind::DirectSource => {
                    // `DirectSource` is a content-hash alias for
                    // `FileWholeHash` (it reads `whole_hashes`) — apply
                    // the same tombstone rejection so the
                    // removal-makes-it-look-untracked window cannot be
                    // re-exploited on the `DirectSource` rail.
                    if self.snapshot.tombstoned_canonicals.contains(canonical_id) {
                        return false;
                    }
                    match self.snapshot.whole_hashes.get(canonical_id) {
                        Some(current) => current == hash,
                        // Untracked dependency file — accept (same reasoning
                        // as FileWholeHash above).
                        None => true,
                    }
                }
                _ => self
                    .snapshot
                    .derived_hashes
                    .get(&(canonical_id.clone(), *kind))
                    .is_some_and(|current| current == hash),
            },
            // R26 per-domain variants — route to the per-domain
            // validators. `HostStoreView` participates in the
            // legacy whole-hash regime today; the per-domain
            // validators are populated by their respective
            // producers. Default impls (returning `false`) are
            // inherited from the trait until per-domain producers
            // wire actual validation through this view.
            // R26 per-domain variants — route to the per-domain
            // validators (which return `false` by trait default;
            // per-domain producers override).
            crate::resolver_core::FactVersionRef::Parse(p) => {
                crate::resolver_core::StoreView::validates_parse_domain(self, p)
            }
            crate::resolver_core::FactVersionRef::ResolveImports(r) => {
                crate::resolver_core::StoreView::validates_resolve_imports_domain(self, r)
            }
            crate::resolver_core::FactVersionRef::RouteSurface(r) => {
                crate::resolver_core::StoreView::validates_route_surface_domain(self, r)
            }
            // Project-generation fact: the cached value observed the
            // project-wide generation `generation`. It validates iff
            // the generation snapshotted at this view's build time
            // still matches — a project-shape change (`tsconfig`,
            // path-alias, SDK, workspace-folder, project-graph) bumps
            // the counter and rejects the entry.
            crate::resolver_core::FactVersionRef::ProjectGeneration { generation } => {
                self.snapshot.project_generation == *generation
            }
        }
    }

    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.snapshot.whole_hashes.contains_key(canonical_id)
    }

    /// Direct read of the snapshotted `DerivedFactHash` for a
    /// `(canonical, kind)` pair. Used by per-rejection attribution
    /// helpers to discriminate "entry absent" from "entry present,
    /// hash differs" without re-probing the validator.
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<crate::resolver_core::ResolverHash16> {
        self.snapshot
            .derived_hashes
            .get(&(canonical_id.to_owned(), kind))
            .copied()
    }

    /// Strict self-root `FileWholeHash` validation.
    ///
    /// Unlike the [`Self::validates`] `FileWholeHash` arm — whose
    /// untracked-file branch optimistically returns `true` so a
    /// dependency loaded after the view snapshot is not forced through
    /// a permissive recheck — this strict variant returns `false` for
    /// an untracked keyed canonical. A self-root names a query-identity
    /// cache entry's OWN keyed canonical; if that file is untracked by
    /// the live view its content is unknown here, which must invalidate
    /// the entry (a same-canonical content edit must not survive). A
    /// tracked canonical is validated by exact hash equality, identical
    /// to the [`Self::validates`] tracked arm.
    fn validates_self_root_whole_hash(
        &self,
        canonical_id: &str,
        hash: &crate::resolver_core::ResolverHash16,
    ) -> bool {
        match self.snapshot.whole_hashes.get(canonical_id) {
            Some(current) => current == hash,
            // Untracked self-root canonical — the entry's own file is
            // not in this view. Reject: the warm read misses and
            // recomputes against current content.
            None => false,
        }
    }

    /// Parse-domain validator (R26).
    ///
    /// Look up `fact.key` against the file's `FileFacts` registry and
    /// compare the stored fact's `semantic_hash` / `display_hash`
    /// (per `fact.lane`) to the observed `expected_hash`. The lookup
    /// resolves the current `FileArtifacts` for `canonical_id` from
    /// the project type store; the view snapshot's `whole_hashes`
    /// already pins the parse-env-hash slice the artifacts derive
    /// from, so this read is wait-free against concurrent writers.
    ///
    /// `None` outcomes — file untracked, artifacts absent, key not
    /// in registry — all signal "no longer there", which under R3
    /// must invalidate the consumer's warm hit. The validator
    /// therefore returns `false` rather than the optimistic-accept
    /// shape used for `FileWholeHash` untracked files: a path-precise
    /// `Member`/`MemberPresence` consumer expects the fact to BE in
    /// the registry it recorded, so absence is a discriminating miss.
    fn validates_parse_domain(&self, fact: &crate::resolver_core::ParseFactRef) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];
        let facts = match self.snapshot.file_facts.get(fact.canonical_id.as_str()) {
            Some(f) => f,
            // Untracked file — accept if the observed hash was the
            // zero sentinel (producer saw the file as unavailable
            // and recorded the sentinel; absence is consistent).
            // Otherwise reject — the consumer observed a real fact
            // hash but the file has dropped out of the index.
            None => return fact.expected_hash == ZERO_HASH,
        };
        // `lookup_or_compute`: a recorded body-sensitive `Export` /
        // `LocalDecl` observation revalidates through the SAME lazy
        // body fact path that produced it (the side-store is Arc-shared
        // with the snapshot's `FileFacts` clone).
        match facts.lookup_or_compute(&fact.key) {
            Some(stored) => {
                let stored_hash = match fact.lane {
                    verter_semantic::facts::registry::FactLane::Semantic => stored.semantic_hash,
                    verter_semantic::facts::registry::FactLane::Display => stored.display_hash,
                };
                stored_hash == fact.expected_hash
            }
            // Fact absent in registry — accept iff observed was the
            // zero sentinel (consistent absence — see
            // `fact_signature_helpers::parse_fact_ref`).
            None => fact.expected_hash == ZERO_HASH,
        }
    }

    /// Resolve-imports-domain validator (R26).
    ///
    /// Compose `ResolvedImportFactsKey { canonical, content_hash,
    /// parse_env_hash, resolve_env_hash, resolver_version,
    /// known_miss_generation }` from the fact's `canonical_id`, the
    /// view's tracked `whole_hashes[canonical]`,
    /// `resolved_import_facts_known_miss_tags[canonical]`, and the
    /// view's `env_hashes`. Look up the matching
    /// `Arc<ResolvedImportFacts>` from the captured
    /// `ResolvedImportFactsDb` handle and compare the per-binding
    /// `semantic_hash` / `display_hash` (per `fact.lane`) of the
    /// matching `ResolvedImportClauseEntry` or
    /// `ResolvedReexportBindingEntry` against `expected_hash`.
    ///
    /// Outcomes:
    /// - Handle missing (view built without a resolved-import-facts
    ///   snapshot) → reject. A consumer that observed a real fact
    ///   under no producer is a bug; the caller falls back to cold
    ///   compute, which will re-emit through the producer.
    /// - File untracked under the view (no `whole_hashes[canonical]`
    ///   entry) → accept the optimistic content-hash sentinel
    ///   (`expected_hash == ZERO_HASH`); reject any real fact hash
    ///   for an untracked file (same shape as
    ///   `validates_parse_domain`).
    /// - Cache slot absent for the composed key → reject. The cache
    ///   was the recording site; absence means the consumer
    ///   observed a stale slice.
    /// - Binding present and hash matches → accept; hash differs →
    ///   reject (cosmetic-only edit invalidates display-lane
    ///   consumers but not semantic-lane consumers, per the lane
    ///   discriminator).
    fn validates_resolve_imports_domain(
        &self,
        fact: &crate::resolver_core::ResolveImportsFactRef,
    ) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];

        // R26 producer: untracked-file optimistic-accept window. A
        // path-precise resolve-imports consumer that observed against
        // a sentinel hash (`ZERO_HASH`) means "this file produced no
        // value at observation time"; accept that observation against
        // an untracked file (still produces no value).
        let content_hash = match self.snapshot.whole_hashes.get(fact.canonical_id.as_str()) {
            Some(h) => *h,
            None => return fact.expected_hash == ZERO_HASH,
        };

        self.validates_resolve_imports_domain_for_content_hash(fact, content_hash)
    }

    /// Route-surface-domain validator (R26 + R29 + G1).
    ///
    /// `ModuleAugmentationIndexShape` → consult the snapshot of
    /// augmentation-index fingerprints captured at view-build time
    /// (R29 / G1 producer state).
    ///
    /// `EffectiveExportSet` → compose
    /// `EffectiveExportSetKey { provider_canonical,
    /// project_identity, resolve_env_hash, lib_env_hash, session_scope }`
    /// from the fact's `canonical_id` plus the view's `project_identity`,
    /// `env_hashes`, and CONTENT-FREE session scope (R6), look up the
    /// cached entry in the captured `RouteDb` handle, and compare the
    /// entry's `augmenter_set_fingerprint` to `fact.expected_hash` (the
    /// overlay content identity is matched here on the VALUE, never in
    /// the key).
    fn validates_route_surface_domain(
        &self,
        fact: &crate::resolver_core::RouteSurfaceFactRef,
    ) -> bool {
        use verter_semantic::facts::FactKey;
        match &fact.key {
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
            } => {
                let key = RouteSurfaceIndexShapeKey {
                    target_kind_tag: *target_kind_tag,
                    external_specifier: external_specifier.as_ref().map(|s| s.as_ref().to_owned()),
                    resolved_relative_canonical: resolved_relative_canonical
                        .as_ref()
                        .map(|s| s.as_ref().to_owned()),
                    wildcard_pattern: wildcard_pattern.as_ref().map(|s| s.as_ref().to_owned()),
                    // CONTENT-ADDRESSED population: a session view validates
                    // against the `Session(overlay-set fingerprint)` augmenter
                    // set, a base view against `Base`. This is the
                    // augmentation-INDEX population (the fingerprint IS its
                    // content view identity), deliberately DISTINCT from the
                    // `EffectiveExportSet` arm below, which composes the
                    // CONTENT-FREE `EffectiveExportSetScope` (R6). The index
                    // snapshot is fresh per fingerprint, so a session
                    // membership/content change moves the fingerprint and the
                    // validated lookup misses. The fact carries no population
                    // (a content-free target shape); the population is the
                    // VIEW's, via the SAME derivation as the producer.
                    population: self.augmentation_population(),
                };
                match self.snapshot.route_surface_index_fingerprints.get(&key) {
                    Some(current) => current == &fact.expected_hash,
                    // Absent from the snapshot — the augmentation
                    // index has not been populated under this view.
                    // Refuse the candidate so the consumer recomputes
                    // through the cold path (which will populate the
                    // index).
                    None => false,
                }
            }
            FactKey::EffectiveExportSet => {
                let route_db = match self.snapshot.route_db.as_ref() {
                    Some(db) => db,
                    None => return false,
                };
                // Compose the `EffectiveExportSetKey` from the fact's
                // `canonical_id` (provider) + view env. Then walk the
                // cache slot for `provider_canonical`; we cannot call
                // `get_effective_export_set(_, view)` here because we
                // ARE the view — that would recurse on validation.
                // Permissive cache-state snapshot via `snapshot_all`
                // is acceptable: the validator only needs to find a
                // candidate whose `augmenter_set_fingerprint` matches
                // the consumer's `expected_hash` under the matching
                // `(provider, project, resolve_env, lib_env)`
                // quadruple.
                let target_key = crate::resolver_core::route_db::EffectiveExportSetKey {
                    provider_canonical: fact.canonical_id.clone(),
                    project_identity: self.snapshot.project_identity,
                    resolve_env_hash: self.snapshot.env_hashes.resolve_env_hash,
                    lib_env_hash: self.snapshot.env_hashes.lib_env_hash,
                    // Compose the view's CONTENT-FREE session scope (R6) so a
                    // session consumer validates against the session slot, a
                    // base consumer against the base slot. The overlay content
                    // fingerprint is NOT in this key — it is matched separately
                    // on the value via the `augmenter_set_fingerprint` compared
                    // below.
                    session_scope:
                        crate::resolver_core::route_db::EffectiveExportSetScope::from_session(
                            self.session_id,
                        ),
                };
                route_db.lookup_effective_export_set_fingerprint(&target_key)
                    == Some(fact.expected_hash)
            }
            // Other parse-domain / resolve-domain keys do not belong
            // to the route-surface domain; the dispatch layer guards
            // against this so the match is exhaustive defensively.
            _ => false,
        }
    }
}

/// Caches one immutable `Arc<StoreViewSnapshot>`-backed base
/// [`HostStoreView`] keyed by its [`StoreViewValidationToken`].
///
/// Batch component-meta rebuilds the full-workspace base view once per
/// job today (the dominant repeated CPU cost). The manager turns that
/// into a build-once / share-by-`Arc`-clone discipline: while the token
/// is unchanged, [`Self::base_view`] hands back a refcount-bumped clone
/// of the cached view instead of re-sweeping the workspace. On a token
/// change the next caller rebuilds and republishes; concurrent callers
/// that observe the same stale token cooperatively converge on whichever
/// build lands first under the lock.
///
/// The cached view is the BASE workspace snapshot only (no session
/// overlay). Session-overlaid views start from this cached base and
/// re-root via copy-on-write in [`HostStoreView::with_session_overlay`],
/// so the shared base stays pristine.
/// Shared state behind the [`StoreViewManager`] mutex.
///
/// `cached` is the published token-keyed base view. `building` is the
/// singleflight claim: while it is set, a builder owns the in-flight
/// cold sweep and concurrent token-miss callers WAIT on `built` rather
/// than launching their own parallel sweeps.
#[derive(Debug, Default)]
struct StoreViewManagerState {
    cached: Option<(StoreViewValidationToken, HostStoreView)>,
    /// `true` while a builder is running `build_coherent` outside the
    /// lock. A single in-flight build at a time; joiners block on the
    /// condvar.
    building: bool,
    /// Monotonic reset generation. Advanced by every [`StoreViewManager::clear`]
    /// (a host-lifecycle reset: `close` / `set_workspace` /
    /// `configure_projects`). A builder claims the build under the
    /// reset generation observed at claim time and refuses to publish its
    /// snapshot into `cached` if the generation advanced while it built —
    /// otherwise an in-flight builder that captured its `pre` token BEFORE
    /// a reset could republish a pre-reset snapshot AFTER `clear()` ran,
    /// defeating the reset's `Arc`-release intent and re-warming the cache
    /// with a stale base view. This is the reset half of the snapshot
    /// publish/return invariant (the token half is the live-token re-read
    /// in `base_view`).
    reset_generation: u64,
    /// Test-only: number of callers currently parked on `built.wait`.
    /// The woken-waiter regression polls this so it can deterministically
    /// line a waiter up behind the gated builder before advancing the
    /// token.
    #[cfg(test)]
    parked_waiters: usize,
}

/// Caches one immutable `Arc<StoreViewSnapshot>`-backed base view keyed
/// by its [`StoreViewValidationToken`], with a singleflight cold-build
/// claim so concurrent token-miss callers do not run N parallel
/// full-workspace sweeps.
#[derive(Debug, Default)]
pub(crate) struct StoreViewManager {
    state: parking_lot::Mutex<StoreViewManagerState>,
    /// Signalled when a builder publishes (or abandons) the in-flight
    /// build, waking joiners that parked on `building`.
    built: parking_lot::Condvar,
}

/// RAII guard for the [`StoreViewManager`] singleflight build claim.
///
/// Armed the moment a caller sets `building = true`. Its `Drop` re-acquires
/// the manager mutex, clears the claim, and wakes every parked joiner on the
/// `built` condvar — on EVERY exit path, including a panic unwinding out of
/// `build_coherent`. parking_lot mutexes do NOT poison, so without this guard
/// a panicking builder would leave `building == true` forever and every
/// current joiner AND every future caller would block permanently on
/// `self.built.wait` (a total hang of the store-view path). The guard makes
/// the claim-release unconditional.
///
/// The publish path drops the manager lock BEFORE this guard drops (parking_lot
/// is not reentrant), so the guard's `Drop` re-locks cleanly.
struct BuildClaimGuard<'m> {
    manager: &'m StoreViewManager,
}

impl Drop for BuildClaimGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.manager.state.lock();
        state.building = false;
        // Wake every parked joiner: on a published `Coherent` view they
        // re-probe and (when tokens match) warm-hit the freshly-published
        // view; on `Superseded` (or a panic that published nothing) one of
        // them re-claims the build.
        self.manager.built.notify_all();
    }
}

impl StoreViewManager {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            state: parking_lot::Mutex::new(StoreViewManagerState::default()),
            built: parking_lot::Condvar::new(),
        }
    }

    /// Return the cached base view when its token still matches the
    /// host's current token; otherwise build a fresh coherent snapshot
    /// once (singleflight), publish it, and return it.
    ///
    /// The returned [`HostStoreView`] clone is cheap: its
    /// `Arc<StoreViewSnapshot>` is shared with the cached entry, so a
    /// stable-token hit costs one refcount bump rather than a
    /// full-workspace sweep.
    ///
    /// ## Singleflight (canonical-dependency-cache rule)
    ///
    /// On a token miss, exactly ONE caller claims the build (`building =
    /// true`) and runs `build_coherent` OUTSIDE the lock; concurrent
    /// token-miss callers WAIT on the `built` condvar and then clone the
    /// winner's published `Arc<StoreViewSnapshot>` instead of each
    /// running a parallel full-workspace sweep. This collapses the first
    /// wave after any token change onto one materialization (without it,
    /// N batch workers all pass the warm probe and run N sweeps — the
    /// exact CPU waste this manager exists to remove).
    ///
    /// The build runs strictly outside the lock, so unrelated readers are
    /// never serialised behind it and there is no self-await / deadlock:
    /// a builder never re-enters `base_view` while holding the claim.
    ///
    /// ## Bounded liveness (no spin under token churn)
    ///
    /// The cooperative loop is BOUNDED by
    /// [`STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS`]: each round either warm-hits
    /// (returns), claims a build (which on `Superseded` keeps the freshest
    /// built view and re-loops), or parks behind an in-flight builder
    /// (which on wake re-loops). Under sustained validation-token churn —
    /// a host whose token advances on every snapshot attempt — every
    /// claimed build is superseded and every wake re-misses, so a naive
    /// unbounded loop would re-claim a never-coherent build FOREVER. The
    /// bound caps the cooperative rounds; on exhaustion the manager hands
    /// the caller the freshest built view as [`StoreViewRead::ReturnOnly`]
    /// WITHOUT publishing it, so an incoherent view is never cached (the
    /// no-torn-PUBLISH contract holds) and the caller's `is_stable` /
    /// publish fence (cold builder) — or its warm-validation miss-to-cold
    /// fallthrough — degrades the stale view to return-only. The loop
    /// therefore terminates in bounded time and never spins.
    ///
    /// ## Typed currentness contract
    ///
    /// A view is handed back as [`StoreViewRead::Current`] ONLY when it
    /// was published / coalesced through [`PublishOutcome::Published`] —
    /// i.e. its token matched the live host token under the manager lock
    /// at handoff. Every other terminating path
    /// ([`PublishOutcome::Declined`] on the final build,
    /// [`SnapshotBuildOutcome::Superseded`] on the final build, or
    /// retry-budget exhaustion) yields [`StoreViewRead::ReturnOnly`]. The
    /// manager NEVER hands a warm validator a view it knows is stale.
    fn base_view(&self, host: &VerterHost) -> StoreViewRead {
        // The freshest view a superseded build on THIS call produced, kept
        // to hand back return-only if the cooperative rounds are exhausted,
        // together with the reason classifying why the freshest round was
        // non-current.
        let mut fallback: Option<(HostStoreView, StoreViewReturnOnlyReason)> = None;
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            // Acquire the manager lock and decide a role: warm-hit,
            // join-the-flight, or claim-the-flight. The live token is
            // re-read INSIDE the lock (below) on every iteration, so a woken
            // waiter that re-enters this loop top always decides its role
            // against a freshly-captured token — never one captured before
            // it slept. The block evaluates to the reset generation observed
            // when this round CLAIMED the build (the warm-hit / park arms
            // return / continue before reaching it).
            let claim_reset_generation = {
                let mut state = self.state.lock();
                // Warm probe: token-stable cache hit hands back an Arc clone.
                //
                // RE-READ the live token here, AFTER acquiring the manager
                // lock, rather than trusting the `current` captured at the
                // loop top. A host mutation can land between that pre-lock
                // capture and this point (e.g. while this thread waited to
                // acquire `state`), bumping the live token; the cached entry
                // would still match the STALE `current` and we would return a
                // view the host has already superseded. Direct callers such
                // as `try_component_meta_cache_hit` use this view for
                // immediate fact validation, so a stale warm hit can validate
                // an old cache entry against already-invalidated state. Re-
                // reading the live token while holding the lock closes that
                // window: the returned view's token equals the live token at
                // return time, so a warm hit is only served when it is
                // genuinely current. (A mutation that lands AFTER this re-read
                // and return is a normal post-read race every warm cache
                // accepts — the consumer revalidates facts.)
                //
                // Test-only: fire a one-shot host mutation HERE, inside the
                // lock and immediately before the live-token re-read, to
                // reproduce deterministically a mutation that landed while
                // this thread waited to acquire `state`. Because `live` is
                // read AFTER the bump, the cached (pre-bump) entry false-
                // misses and we rebuild — never returning a stale warm hit.
                #[cfg(test)]
                if FORCE_WARM_PROBE_TOKEN_BUMP.with(|c| {
                    let armed = c.get();
                    c.set(false);
                    armed
                }) {
                    host.bump_store_view_epoch();
                }
                let live = StoreViewValidationToken::capture(host);
                if let Some((token, view)) = state.cached.as_ref() {
                    if *token == live {
                        // The cached entry's token equals the live host
                        // token under the lock: proven current, safe for
                        // warm-cache validation.
                        return StoreViewRead::Current(CurrentHostStoreView(view.clone()));
                    }
                }
                if state.building {
                    // A builder owns the in-flight cold sweep. Park until it
                    // publishes. On wake the live host token may have advanced
                    // (a host mutation while we slept, or the winner was
                    // superseded to a still newer token), so we restart the
                    // loop to RE-READ the live token inside the lock before
                    // deciding our role afresh: a published winner keyed on a
                    // now-stale token must false-miss (forcing a re-claim),
                    // and a winner keyed on the current token warm-hits.
                    #[cfg(test)]
                    {
                        state.parked_waiters += 1;
                    }
                    self.built.wait(&mut state);
                    #[cfg(test)]
                    {
                        state.parked_waiters = state.parked_waiters.saturating_sub(1);
                    }
                    continue;
                }
                // No warm hit and no in-flight build — claim it. Record the
                // reset generation observed at claim time so the publish
                // fence below can detect a `clear()` that races this build.
                state.building = true;
                state.reset_generation
            };

            // We hold the build claim. Arm the RAII guard FIRST so the
            // claim is cleared (and joiners woken) on EVERY exit path —
            // including a panic unwinding out of `build_coherent`.
            // parking_lot mutexes do not poison, so without the guard a
            // panicking builder would leave `building == true` forever and
            // every current joiner AND future caller would block on the
            // `built` condvar permanently (a total store-view hang).
            let claim = BuildClaimGuard { manager: self };

            // Run the coherent build OUTSIDE the lock. A panic here unwinds
            // through `claim`'s `Drop`, releasing the claim and waking
            // joiners; it never strands `building == true`.
            let outcome = HostStoreView::build_coherent(host, None);

            match outcome {
                SnapshotBuildOutcome::Coherent { view, token } => {
                    let mut state = self.state.lock();
                    let outcome = self.publish_coherent(
                        &mut state,
                        host,
                        claim_reset_generation,
                        token,
                        view,
                    );
                    // Drop the manager lock BEFORE `claim` so the guard's
                    // `Drop` re-locks cleanly (parking_lot is not
                    // reentrant). The guard then clears `building` + wakes
                    // joiners, which re-probe and warm-hit the published
                    // view.
                    drop(state);
                    drop(claim);
                    match outcome {
                        // Published / coalesced onto a live same-token entry:
                        // the view's token equals the live host token, so it
                        // is PROVEN-CURRENT and safe to hand to a warm-cache
                        // validator.
                        PublishOutcome::Published { view } => {
                            return StoreViewRead::Current(CurrentHostStoreView(view));
                        }
                        // KNOWN-STALE: a reset raced this build, or the host
                        // token moved past the build's token between build
                        // completion and publish. The view must NOT be
                        // returned to a warm validator (it would validate a
                        // cached entry against pre-mutation state). Keep it as
                        // the return-only fallback and re-loop within the
                        // bound: the warm probe at the top re-reads the live
                        // token and either warm-hits a concurrently published
                        // coherent view or re-claims a fresh build against the
                        // now-current token. Same cooperative, bounded
                        // treatment as `Superseded`.
                        PublishOutcome::Declined { view } => {
                            fallback = Some((view, StoreViewReturnOnlyReason::PublishDeclined));
                            continue;
                        }
                    }
                }
                SnapshotBuildOutcome::Superseded { view } => {
                    // The host mutated on every build attempt. Keep this
                    // build's freshest (incoherent) view as the return-only
                    // fallback, release the claim (the guard clears
                    // `building` + wakes joiners), and re-loop within the
                    // bound: the warm probe at the top picks up a
                    // concurrently published coherent view, or we re-claim a
                    // fresh build against the now-current token. The view is
                    // NOT published — its token is stale, so caching it
                    // would violate the no-torn-PUBLISH contract.
                    fallback = Some((view, StoreViewReturnOnlyReason::Superseded));
                    drop(claim);
                    continue;
                }
            }
        }

        // Cooperative rounds exhausted under sustained churn. Hand back the
        // freshest built view as `ReturnOnly` WITHOUT caching it. It is
        // KNOWN non-current, so a warm validator that receives it must miss
        // to cold; a cold builder that seeds with it relies on its own
        // `is_stable` / publish fence to reject promotion. The manager never
        // hands a warm validator a view it knows is stale.
        if let Some((view, reason)) = fallback {
            return StoreViewRead::ReturnOnly { view, reason };
        }
        // We never claimed a build on any round (every round was a warm miss
        // followed by a park behind a builder that kept getting superseded),
        // so this waiter has no view of its own. The final build MUST stay
        // singleflighted: an UNCLAIMED `build_coherent` here would let every
        // exhausted waiter (N of them under churn) sweep the full workspace
        // in parallel — the exact CPU waste this manager exists to remove.
        // Route the final build through a claim-or-rejoin lane instead.
        self.claim_or_rejoin_final_build(host)
    }

    /// Final-fallback read for a `base_view` waiter that exhausted its
    /// retry budget WITHOUT ever claiming a build (it only ever parked
    /// behind other in-flight builders, so it carries no view of its own).
    ///
    /// The defect this closes: returning the final view via an UNCLAIMED
    /// `build_coherent` lets every exhausted waiter sweep the full workspace
    /// in parallel under sustained churn, defeating the [`StoreViewManager`]
    /// singleflight guarantee. A build here MUST run only under the
    /// singleflight claim (`building`), so at most one waiter sweeps at a
    /// time and the rest rejoin the lane.
    ///
    /// Bounded claim-or-rejoin loop, each round under the manager lock:
    ///
    /// * **warm-hit** — a builder published a live-token view → `Current`.
    /// * **stale cache** — a builder left a non-current cached view → clone
    ///   it as a cold-seed `ReturnOnly` (the caller's own `is_stable` /
    ///   warm-validation-miss fence degrades it). No sweep.
    /// * **lane free** — claim it (`building = true`) and run exactly ONE
    ///   singleflighted `build_coherent`, then publish / return.
    /// * **lane busy** — PARK on the `built` condvar; on wake, RE-CHECK the
    ///   lane in the SAME lock acquisition and claim it inline if it has
    ///   freed (so a park that wakes to a free lane claims WITHIN the round
    ///   rather than spending another round). Never claim over a `true`
    ///   `building` — that would double-claim and sweep in parallel.
    ///
    /// Liveness: `build_coherent` is internally bounded and ALWAYS releases
    /// its claim (RAII guard) on completion, so the lane frees within bounded
    /// time and a parked waiter is always woken. The loop is capped by
    /// [`STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS`]; on the rare exhaustion (a
    /// builder monopolised the lane for the whole budget without caching) the
    /// freshest view this waiter managed to claim-build is returned
    /// `ReturnOnly`. The path terminates in bounded time, never spins, never
    /// sweeps unclaimed, and never advertises a known-stale view as
    /// `Current`.
    fn claim_or_rejoin_final_build(&self, host: &VerterHost) -> StoreViewRead {
        let mut fallback: Option<(HostStoreView, StoreViewReturnOnlyReason)> = None;
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let claim_reset_generation = {
                let mut state = self.state.lock();
                // Re-probe the cache under the lock (re-reading the live token
                // so a hit is served only when genuinely current). A cached
                // view — current OR stale — is returned WITHOUT sweeping.
                if let Some(read) = Self::current_or_stale_cached_read(host, &state) {
                    return read;
                }
                if state.building {
                    // A builder owns the lane and nothing is cached yet. PARK
                    // and ride its result rather than sweeping in parallel.
                    #[cfg(test)]
                    {
                        state.parked_waiters += 1;
                    }
                    self.built.wait(&mut state);
                    #[cfg(test)]
                    {
                        state.parked_waiters = state.parked_waiters.saturating_sub(1);
                    }
                    // Re-check IN THE SAME lock acquisition: a cached view now
                    // (current/stale) is returned; if the lane freed, fall
                    // through to claim it within THIS round; if a different
                    // builder re-claimed, park again next round.
                    if let Some(read) = Self::current_or_stale_cached_read(host, &state) {
                        return read;
                    }
                    if state.building {
                        // Another builder re-claimed before we could; re-loop
                        // and park again (bounded).
                        continue;
                    }
                    // Lane freed on wake with an empty cache — claim it inline.
                }
                // Lane free, cache empty — claim so exactly THIS waiter sweeps.
                state.building = true;
                state.reset_generation
            };

            let claim = BuildClaimGuard { manager: self };
            let outcome = HostStoreView::build_coherent(host, None);
            match outcome {
                SnapshotBuildOutcome::Coherent { view, token } => {
                    let mut state = self.state.lock();
                    let outcome = self.publish_coherent(
                        &mut state,
                        host,
                        claim_reset_generation,
                        token,
                        view,
                    );
                    drop(state);
                    drop(claim);
                    match outcome {
                        PublishOutcome::Published { view } => {
                            return StoreViewRead::Current(CurrentHostStoreView(view));
                        }
                        PublishOutcome::Declined { view } => {
                            fallback = Some((view, StoreViewReturnOnlyReason::PublishDeclined));
                            continue;
                        }
                    }
                }
                SnapshotBuildOutcome::Superseded { view } => {
                    fallback = Some((view, StoreViewReturnOnlyReason::Superseded));
                    drop(claim);
                    continue;
                }
            }
        }
        // Budget exhausted. This waiter now either claimed and built at least
        // one (known-stale) view of its own, or a builder left a cached view
        // the cache-clone arm above would have returned. Hand back the
        // freshest claimed-build view as `ReturnOnly` WITHOUT caching it.
        if let Some((view, reason)) = fallback {
            return StoreViewRead::ReturnOnly { view, reason };
        }
        // Adversarial corner: a concurrent builder monopolised the lane for
        // the ENTIRE bounded budget without ever caching, so this waiter
        // never claimed and has no view. One last lock-guarded read: return a
        // cached view if one finally exists; otherwise claim the now-or-soon-
        // free lane for a single guarded build. This still never sweeps
        // unclaimed.
        let claim_reset_generation = {
            let mut state = self.state.lock();
            if let Some(read) = Self::current_or_stale_cached_read(host, &state) {
                return read;
            }
            // Wait (bounded) for the lane to free, then claim it. Each
            // `build_coherent` releases its claim on completion, so this wakes
            // within bounded time.
            while state.building {
                #[cfg(test)]
                {
                    state.parked_waiters += 1;
                }
                self.built.wait(&mut state);
                #[cfg(test)]
                {
                    state.parked_waiters = state.parked_waiters.saturating_sub(1);
                }
                if let Some(read) = Self::current_or_stale_cached_read(host, &state) {
                    return read;
                }
            }
            state.building = true;
            state.reset_generation
        };
        let claim = BuildClaimGuard { manager: self };
        let outcome = HostStoreView::build_coherent(host, None);
        match outcome {
            SnapshotBuildOutcome::Coherent { view, token } => {
                let mut state = self.state.lock();
                let outcome =
                    self.publish_coherent(&mut state, host, claim_reset_generation, token, view);
                drop(state);
                drop(claim);
                match outcome {
                    PublishOutcome::Published { view } => {
                        StoreViewRead::Current(CurrentHostStoreView(view))
                    }
                    PublishOutcome::Declined { view } => StoreViewRead::ReturnOnly {
                        view,
                        reason: StoreViewReturnOnlyReason::PublishDeclined,
                    },
                }
            }
            SnapshotBuildOutcome::Superseded { view } => {
                drop(claim);
                StoreViewRead::ReturnOnly {
                    view,
                    reason: StoreViewReturnOnlyReason::Superseded,
                }
            }
        }
    }

    /// If the manager has a cached base view, classify it against the live
    /// host token (re-read here under the caller's held lock) and return the
    /// matching [`StoreViewRead`]:
    ///
    /// * token matches live → [`StoreViewRead::Current`] (warm hit).
    /// * token is stale → [`StoreViewRead::ReturnOnly`] cold-seed (a
    ///   superseded builder left it; the caller's own fence degrades it).
    ///
    /// Returns `None` when the cache is empty. The caller holds the manager
    /// lock; this performs no build and never claims — it only reads the
    /// already-published entry, so an exhausted waiter prefers cloning a
    /// cached view to launching a fresh (unclaimed) sweep.
    fn current_or_stale_cached_read(
        host: &VerterHost,
        state: &StoreViewManagerState,
    ) -> Option<StoreViewRead> {
        let (token, view) = state.cached.as_ref()?;
        let live = StoreViewValidationToken::capture(host);
        if *token == live {
            Some(StoreViewRead::Current(CurrentHostStoreView(view.clone())))
        } else {
            Some(StoreViewRead::ReturnOnly {
                view: view.clone(),
                reason: StoreViewReturnOnlyReason::Superseded,
            })
        }
    }

    /// Publish a freshly-built coherent base view into `cached` under the
    /// snapshot publish/return invariant.
    ///
    /// Three gates decide whether the view is current and admissible:
    ///
    /// 1. **Reset fence.** If `clear()` advanced `reset_generation` since
    ///    this build claimed (`claim_reset_generation`), the build straddled
    ///    a host-lifecycle reset. Republishing its pre-reset snapshot would
    ///    defeat the reset's `Arc`-release intent and re-warm the cache with
    ///    a stale base view, so the view is [`PublishOutcome::Declined`].
    /// 2. **Live-token fence.** A snapshot is only published when its token
    ///    still equals the live host token, re-read here while holding the
    ///    manager lock. A token that moved between build completion and
    ///    publish is stale; caching it would violate the no-torn-PUBLISH
    ///    contract, and RETURNING it to a warm validator would let that
    ///    validator validate a cached entry against pre-mutation state. The
    ///    view is [`PublishOutcome::Declined`]. (`build_coherent` already
    ///    guarantees the built view is internally coherent; this re-read
    ///    additionally rejects a view the host superseded after the build's
    ///    own post-build check.)
    /// 3. **Newer-cache fence.** A racing build may have already published
    ///    the SAME token (reuse its `Arc` so all callers share one snapshot)
    ///    or a DIFFERENT one this older snapshot must not clobber.
    ///
    /// A declined view is KNOWN-STALE: `base_view` must re-loop against the
    /// freshly-read live token rather than return it to a warm validator.
    /// Only the live, published/coalesced view is [`PublishOutcome::Published`].
    fn publish_coherent(
        &self,
        state: &mut StoreViewManagerState,
        host: &VerterHost,
        claim_reset_generation: u64,
        token: StoreViewValidationToken,
        view: HostStoreView,
    ) -> PublishOutcome {
        // Test-only PERSISTENT knob: force the RESET fence to decline on
        // every publish WITHOUT advancing any token dimension, so
        // `base_view` exhausts its bounded retry and returns a
        // `ReturnOnly` seed whose token still matches the live host. This
        // is the additive/reset-only `ReturnOnly` the `FORCE_SUPERSEDE_*`
        // (epoch-bumping) knobs cannot produce — it isolates the publish
        // fence's seed-currentness gate from its token gate.
        #[cfg(test)]
        if FORCE_RESET_FENCE_DECLINE_ALWAYS.with(std::cell::Cell::get) {
            return PublishOutcome::Declined { view };
        }
        // Gate 1: reset raced this build → known-stale, decline.
        if state.reset_generation != claim_reset_generation {
            // Test-only: record the SPECIFIC pre-reset snapshot that the
            // reset fence declined, so the reset-fence regression can assert
            // by `Arc` identity that this exact snapshot is never re-warmed
            // into `cached` (the bounded re-loop must rebuild a fresh
            // post-reset snapshot instead).
            #[cfg(test)]
            record_reset_declined_snapshot_for_tests(&view);
            return PublishOutcome::Declined { view };
        }
        // Test-only ONE-SHOT knob: advance `store_view_epoch` HERE, inside
        // the publish lock and immediately before the live-token fence,
        // modelling a host mutation that landed between build completion and
        // publish. Because the fence re-reads the live token AFTER the bump,
        // the build's `token` no longer matches → Gate 2 declines, driving
        // the bounded re-loop. Consumed on fire.
        #[cfg(test)]
        if FORCE_PUBLISH_DECLINE_ONCE.with(|c| {
            let armed = c.get();
            c.set(false);
            armed
        }) {
            host.bump_store_view_epoch();
        }
        // Gate 2: the host moved past this token after the build →
        // known-stale, decline. Returning this view to a warm validator
        // would validate a cached entry against already-superseded state.
        if token != StoreViewValidationToken::capture(host) {
            return PublishOutcome::Declined { view };
        }
        // Gate 3: reuse an already-published same-token entry; never clobber
        // a newer one. Either branch yields a live view at `token`.
        match state.cached.as_ref() {
            Some((cached_token, cached_view)) if *cached_token == token => {
                PublishOutcome::Published {
                    view: cached_view.clone(),
                }
            }
            _ => {
                state.cached = Some((token, view.clone()));
                PublishOutcome::Published { view }
            }
        }
    }

    /// Drop the cached base-view `Arc<StoreViewSnapshot>` so the snapshot
    /// (and its per-file maps / fact `Arc`s) is released immediately.
    ///
    /// A host-lifecycle reset (`close`, full-cache-clear) bumps the
    /// validation token so the cached view is no longer a valid warm-hit
    /// candidate — but a token bump ALONE keeps the `Arc` strongly held
    /// until the NEXT store-view request rebuilds and replaces it. For a
    /// host that is closed and never reused (the NAPI finalisation case),
    /// that next request never comes, so the snapshot would stay resident.
    /// Clearing here releases the memory at reset time. A normal upsert
    /// does NOT call this: it bumps the token and the next build replaces
    /// the `Arc` — only closed / fully-cleared hosts need the explicit
    /// drop.
    pub(crate) fn clear(&self) {
        let mut state = self.state.lock();
        state.cached = None;
        // Invalidate any in-flight build claim: a builder that captured its
        // `pre` token BEFORE this reset must NOT republish its (now
        // pre-reset) snapshot into `cached` afterward. The builder compares
        // the reset generation it observed at claim time against this value
        // before publishing and abandons the publish if it advanced —
        // honouring the reset's intent to RELEASE the snapshot `Arc` and not
        // re-warm the cache with a stale base view.
        state.reset_generation = state.reset_generation.wrapping_add(1);
    }

    /// Test-only: how many entries are cached (0 or 1). Lets the
    /// discriminating tests assert the manager publishes exactly one
    /// base view.
    #[cfg(test)]
    pub(crate) fn is_populated(&self) -> bool {
        self.state.lock().cached.is_some()
    }

    /// Test-only: the token of the currently cached base view, if any.
    #[cfg(test)]
    pub(crate) fn cached_token(&self) -> Option<StoreViewValidationToken> {
        self.state.lock().cached.as_ref().map(|(token, _)| *token)
    }

    /// Test-only: the currently cached `(token, snapshot Arc)`, if any. Lets
    /// the reset-fence regression assert by `Arc` identity that the manager
    /// never re-warms `cached` with a builder's specific pre-reset snapshot.
    #[cfg(test)]
    pub(crate) fn cached_entry_for_tests(
        &self,
    ) -> Option<(StoreViewValidationToken, Arc<StoreViewSnapshot>)> {
        self.state
            .lock()
            .cached
            .as_ref()
            .map(|(token, view)| (*token, Arc::clone(&view.snapshot)))
    }

    /// Test-only: number of callers currently parked on `built.wait`.
    /// The woken-waiter regression polls this to line waiters up behind a
    /// gated builder before advancing the token.
    #[cfg(test)]
    pub(crate) fn parked_waiters(&self) -> usize {
        self.state.lock().parked_waiters
    }
}

impl crate::resolver_core::ResolverStore for VerterHost {
    type View = HostStoreView;

    fn snapshot_view(&self) -> Self::View {
        // `ResolverStore::snapshot_view` is a quiescent-host accessor (its
        // sole consumer validates facts against a settled view). Hand back
        // the proven-current view when the manager could prove one; under
        // sustained churn it falls to the cold-seed's inner view, whose own
        // builder fence guards any result computed from it. The capability
        // split lives on `resolver_store_view` / `resolver_store_view_read`
        // for the warm-validation chokepoint.
        match self.resolver_store_view_read() {
            StoreViewRead::Current(current) => current.view().clone(),
            StoreViewRead::ReturnOnly { view, .. } => view,
        }
    }
}

impl VerterHost {
    /// Read the host's base store view as a typed [`StoreViewRead`].
    ///
    /// This is the SOLE general-purpose store-view accessor and the
    /// warm-validation chokepoint. It returns the capability-split
    /// [`StoreViewRead`] rather than a raw [`HostStoreView`]: every caller
    /// must compile-choose one of two capabilities —
    ///
    /// * [`StoreViewRead::current`] / [`CurrentHostStoreView`] — proven
    ///   current by the [`StoreViewManager`]; safe for warm-cache fact
    ///   validation AND for returning a normal query result.
    /// * [`StoreViewRead::into_cold_seed_view`] / [`ColdSeedHostStoreView`]
    ///   — usable ONLY for a fenced cold builder, whose own `is_stable` /
    ///   publish fence (or bounded-retry-then-supersede contract) guards
    ///   any result computed from the seed.
    ///
    /// The raw unwrap that erased the non-current proof is gone: there is
    /// no general accessor that hands back a plain `HostStoreView`, so a
    /// future caller cannot validate / return against a stale view by
    /// accident. The static guard
    /// `resolver_store_view_returns_store_view_read` pins this signature.
    #[track_caller]
    pub(crate) fn resolver_store_view(&self) -> StoreViewRead {
        HostStoreView::from_host_read(self)
    }

    /// Alias for [`Self::resolver_store_view`] retained for the call sites
    /// that name the typed read explicitly. Identical behaviour.
    #[track_caller]
    pub(crate) fn resolver_store_view_read(&self) -> StoreViewRead {
        HostStoreView::from_host_read(self)
    }

    /// Read the host's base store view together with the manager's
    /// currentness proof, for the stable-request executors whose warm
    /// preflight peek must be suppressed on a known-stale snapshot.
    /// Returns `(view, is_current)`; `is_current` is `true` only for a
    /// [`StoreViewRead::Current`] read.
    #[track_caller]
    pub(crate) fn resolver_store_view_with_currentness(&self) -> (HostStoreView, bool) {
        match HostStoreView::from_host_read(self) {
            StoreViewRead::Current(current) => (current.view().clone(), true),
            StoreViewRead::ReturnOnly { view, .. } => (view, false),
        }
    }

    /// Capture ONE coherent [`BatchFixedView`] for a whole component-meta
    /// batch from a SINGLE store-view read, with the session overlay applied
    /// exactly ONCE.
    ///
    /// Every per-capability view (warm-probe current view, cold-seed,
    /// executor fixed-view) is derived from the same [`StoreViewRead`] AND
    /// the same single [`HostStoreView::with_session_overlay`] copy-on-write,
    /// so currentness stays intrinsic to the read and every job reads the
    /// overlay-aware snapshot without re-cloning it. The batch coordinator
    /// calls this ONCE (after the overlay pre-warm) and threads the bundle
    /// into every per-job closure, replacing both the per-job
    /// `resolver_store_view_read()` calls AND the per-job
    /// `with_session_overlay` re-applications — the O(N)→O(1) read AND
    /// overlay-COW collapse.
    ///
    /// **One overlay COW per batch.** The overlay is applied ONCE to the
    /// captured raw view; the three typed wrappers are then derived by cheap
    /// `Arc`-refcount-bump clones of that ALREADY-overlaid view, NOT by
    /// re-invoking `with_session_overlay` (which would re-COW). For a base
    /// (empty-overlay) session `with_session_overlay` is a no-op that keeps
    /// the shared base snapshot — identical to a base capture, with no COW.
    ///
    /// **Fence consistency (CRITICAL).** The jobs now compute against the
    /// OVERLAID view, whose token carries a non-`None` `overlay_identity`.
    /// The captured-vs-live fences ([`BatchFixedView::payload_promotion_admissible`],
    /// the executor's `is_stable`) compare the captured token/fingerprint
    /// against the host's LIVE token, which is always the BASE token
    /// (`overlay_identity: None`, via [`Self::current_validation_token`]).
    /// The request's frozen overlay is NOT an external mutation, so the
    /// captured fence inputs are taken with `overlay_identity` normalised OUT
    /// — exactly the precedent in
    /// [`crate::resolver_core::request_store_view`]'s completion-overlay
    /// publish fence. This keeps the fence like-for-like (base-external
    /// captured vs base-external live): a mid-batch EXTERNAL mutation (epoch
    /// / project-generation / env / identity) still moves the live token and
    /// declines promotion, while the constant overlay identity never
    /// false-supersedes. Because `with_session_overlay` mutates ONLY
    /// `overlay_identity` among the token's dimensions, the base view's token
    /// and the overlaid view's token-with-`overlay_identity`-cleared are
    /// byte-identical, so capturing from the base view yields exactly that
    /// base-external token.
    ///
    /// All the typed wrappers are re-bound from the one `(view,
    /// is_current)` pair via [`StoreViewRead::from_executor_snapshot`] —
    /// the SOLE single-read re-bind point — so no second read is mixed in.
    #[track_caller]
    pub(crate) fn capture_batch_fixed_view(
        &self,
        view: &dyn crate::session_view::SessionView,
    ) -> BatchFixedView {
        let (base_view, is_current) = self.resolver_store_view_with_currentness();
        // Base-external fence token: the captured view's token with
        // `overlay_identity` cleared. The base read's token already carries
        // `overlay_identity: None`, so this IS the base view's token; it is
        // the exact like-for-like reference the captured-vs-live fences
        // compare against the host's live BASE token.
        let captured_token = base_view.validation_token();
        let captured_fingerprint = captured_token.external_supersession_fingerprint();

        // Apply the session overlay ONCE. For a non-empty overlay this is
        // the single per-batch `StoreViewSnapshot` COW + per-canonical
        // re-rooting; for a base (empty) overlay it is a no-op keeping the
        // shared base snapshot. Every typed wrapper below derives from THIS
        // overlaid view via a cheap `Arc`-refcount-bump clone — never a
        // second `with_session_overlay`.
        let overlaid_view = base_view.with_session_overlay(self, view);

        // Re-bind currentness onto the OVERLAID view for each typed
        // capability without re-COWing the overlay. `current_view` is `Some`
        // only on a proven-current read; a `ReturnOnly` capture yields `None`
        // so per-job warm probes miss to cold. `with_session_overlay`
        // preserves currentness (it re-roots a snapshot the manager already
        // proved current), so deriving the overlaid current view here does
        // not launder a non-current capture.
        let current_view = is_current.then(|| CurrentHostStoreView(overlaid_view.clone()));
        let cold_seed = ColdSeedHostStoreView {
            view: overlaid_view.clone(),
            current: is_current,
        };
        BatchFixedView {
            current_view,
            cold_seed,
            executor_view: overlaid_view,
            captured_fingerprint,
            captured_token,
            is_current,
        }
    }

    /// The host's [`StoreViewManager`] — caches one Arc-shareable base
    /// store view keyed by the validation token.
    pub(crate) fn store_view_manager(&self) -> &StoreViewManager {
        &self.store_view_manager
    }

    /// Capture the host's current [`StoreViewValidationToken`] (base,
    /// no session overlay). The publish fence rechecks against this
    /// before promoting a cold result.
    pub(crate) fn current_validation_token(&self) -> StoreViewValidationToken {
        StoreViewValidationToken::capture(self)
    }

    /// Live `u64` fold of the host's EXTERNAL-supersession dimensions
    /// ([`StoreViewValidationToken::external_supersession_fingerprint`]
    /// of the current base token).
    ///
    /// The resolver-tier request executors capture this at snapshot time
    /// and compare it at stable-promotion time: a mismatch means an
    /// external mutation (epoch / project-generation / env-hash /
    /// identity) advanced mid-compute, so the result MUST NOT be promoted
    /// to the shared cache. Threaded through the request-host traits as a
    /// `u64` so the resolver-core seal never sees the concrete token type.
    pub(crate) fn current_external_supersession_fingerprint(&self) -> u64 {
        self.current_validation_token()
            .external_supersession_fingerprint()
    }

    pub(crate) fn component_meta_audit_store_snapshot(
        &self,
        store_view: Option<&HostStoreView>,
    ) -> (
        crate::component_meta_audit::RequestStoreAudit,
        ComponentMetaStoreCounters,
    ) {
        // Entry count and byte sum MUST be drawn from the SAME
        // population. `FileArtifactStore::len` counts every keyed
        // artifact (base + overlay-scoped); the byte sum therefore
        // routes through `snapshot_artifacts()`, which enumerates that
        // same full keyed set. `snapshot_all()` is base-only (it
        // filters to `FileArtifactKey::is_base` keys), so summing
        // bytes over it while counting entries via `len()` would report
        // two different populations in a session that materialised
        // overlay artifacts.
        let artifacts = self.project_type_store.indexed().snapshot_artifacts();
        let indexed_entries = artifacts.len() as u32;
        let indexed_bytes = artifacts
            .iter()
            .map(|(key, file_artifacts)| {
                key.canonical.len() as u64
                    + file_artifacts.indexed.raw_source.len() as u64
                    + file_artifacts.indexed.eval_source.len() as u64
            })
            .sum::<u64>();

        let prepared_bundles = self
            .resolver_runtime()
            .prepared_decl_bundles
            .cached_values();
        let prepared_type_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_type_decls.len() as u32)
        });
        let prepared_value_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_value_decls.len() as u32)
        });

        // Pull per-request materialiser/storage counters off the
        // active `RequestContext` (zero ops when no context is
        // installed; the audit pipeline always installs one before
        // taking this snapshot). These counters move into the
        // component-meta payload — they are kind-specific and do
        // not belong on the generic `RequestStoreAudit`.
        let component_meta_counters = match crate::request_context::current_request_context() {
            Some(ctx) => ComponentMetaStoreCounters {
                materialize_structure_calls: ctx
                    .materialize_structure_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                materialize_structure_cache_hits: ctx
                    .materialize_structure_cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
                node_arena_lock_acquisitions: ctx
                    .node_arena_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                family_map_lock_acquisitions: ctx
                    .family_map_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_merges: ctx
                    .dep_signature_merges
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_intern_hits: ctx
                    .dep_signature_intern_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            },
            None => ComponentMetaStoreCounters::default(),
        };

        let store_audit = crate::component_meta_audit::RequestStoreAudit {
            store_view_hits: u32::from(store_view.is_some()),
            store_view_misses: u32::from(store_view.is_none()),
            structural_merges: 0,
            imported_dependency_entries: indexed_entries,
            imported_dependency_bytes: indexed_bytes,
            prepared_type_decls,
            prepared_value_decls,
            cache_layers: Default::default(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
        };
        (store_audit, component_meta_counters)
    }

    pub(crate) fn component_meta_audit_memory_bytes(&self) -> (u64, u64) {
        let host_cache_bytes: u64 = self
            .project_type_store
            .indexed()
            .snapshot_all()
            .iter()
            .map(|(id, indexed)| {
                id.len() as u64 + indexed.raw_source.len() as u64 + indexed.eval_source.len() as u64
            })
            .sum();

        let workspace = self.workspace();
        let workspace_snapshot = workspace.resource_snapshot();
        let workspace_bytes = workspace_snapshot.overlay_bytes + workspace_snapshot.snapshot_bytes;

        (host_cache_bytes, workspace_bytes)
    }
}

#[cfg(test)]
impl HostStoreView {
    /// Test-only constructor: a view that tracks exactly the supplied
    /// `whole_hashes` map and is otherwise [`HostStoreView::default`].
    ///
    /// `whole_hashes` is a private field, so the unit tests in the
    /// sibling `resolver_store_tests` module cannot build the view via
    /// a struct literal — they construct it through this helper.
    pub(crate) fn with_whole_hashes_for_tests(whole_hashes: FxHashMap<String, Hash16>) -> Self {
        Self {
            snapshot: Arc::new(StoreViewSnapshot {
                whole_hashes,
                ..StoreViewSnapshot::default()
            }),
            ..Self::default()
        }
    }

    /// Test-only: arm the mid-build panic knob on the CURRENT thread. The
    /// next `build_coherent` call on this thread panics partway through the
    /// build (after the [`StoreViewManager`] singleflight claim is taken).
    /// Drives the builder-panic regression: the claim must still be
    /// released (RAII guard) so subsequent callers do not hang on the
    /// `built` condvar. One-shot — the knob disarms itself after firing.
    pub(crate) fn arm_build_panic_for_tests() {
        FORCE_BUILD_PANIC.with(|c| c.set(true));
    }

    /// Test-only: arm the PERSISTENT supersede knob on the CURRENT thread.
    /// Every `build_coherent` attempt on this thread then forces a mid-build
    /// mutation, so no build ever produces a coherent view — modelling a
    /// host whose validation token churns on every snapshot attempt under
    /// sustained load. Drives the bounded-retry liveness gate:
    /// [`StoreViewManager::base_view`] must TERMINATE within its retry cap
    /// and hand back the freshest built view return-only, rather than
    /// re-claiming a never-coherent build forever (an unbounded infinite
    /// loop). Stays armed until [`Self::disarm_supersede_always_for_tests`].
    pub(crate) fn arm_supersede_always_for_tests() {
        FORCE_SUPERSEDE_ALWAYS.with(|c| c.set(true));
    }

    /// Test-only: disarm the persistent supersede knob on the CURRENT
    /// thread so a subsequent build can complete coherently.
    pub(crate) fn disarm_supersede_always_for_tests() {
        FORCE_SUPERSEDE_ALWAYS.with(|c| c.set(false));
    }

    /// Test-only: arm the PERSISTENT reset-fence-decline knob on the
    /// CURRENT thread. Every `publish_coherent` then declines its build
    /// through the reset-fence gate WITHOUT advancing any token dimension,
    /// so [`StoreViewManager::base_view`] exhausts its bounded retry and
    /// returns a [`StoreViewRead::ReturnOnly`] seed whose validation token
    /// still equals the live host. This is the additive/reset-only
    /// `ReturnOnly` that isolates the publish fence's seed-currentness
    /// gate from its token gate (the `FORCE_SUPERSEDE_*` knobs all bump the
    /// epoch, so a token-only fence already rejects their seeds). Stays
    /// armed until [`Self::disarm_reset_fence_decline_always_for_tests`].
    pub(crate) fn arm_reset_fence_decline_always_for_tests() {
        FORCE_RESET_FENCE_DECLINE_ALWAYS.with(|c| c.set(true));
    }

    /// Test-only: disarm the persistent reset-fence-decline knob on the
    /// CURRENT thread so a subsequent build can publish coherently.
    pub(crate) fn disarm_reset_fence_decline_always_for_tests() {
        FORCE_RESET_FENCE_DECLINE_ALWAYS.with(|c| c.set(false));
    }

    /// Test-only: arm the ONE-SHOT warm-probe token-bump knob on the CURRENT
    /// thread. The next [`StoreViewManager::base_view`] iteration advances
    /// `store_view_epoch` inside the manager lock, immediately before the
    /// warm-probe re-reads the live token — reproducing a mutation that lands
    /// while a caller waits to acquire `state`. Drives the
    /// warm-hit-revalidation regression: a manager that compared the cached
    /// entry against a token captured BEFORE the lock would return the stale
    /// cached view; re-reading the live token forces a rebuild instead. The
    /// knob disarms itself after firing.
    pub(crate) fn arm_warm_probe_token_bump_for_tests() {
        FORCE_WARM_PROBE_TOKEN_BUMP.with(|c| c.set(true));
    }

    /// Test-only: arm the ONE-SHOT publish-decline knob on the CURRENT
    /// thread. The next `publish_coherent` call on this thread advances
    /// `store_view_epoch` inside the publish lock, immediately before the
    /// live-token fence (Gate 2) — modelling a host mutation that lands
    /// between build completion and publish, so the freshly-built view's
    /// token no longer matches the live token and the publish is declined.
    /// Drives the publish-decline-must-not-return-stale regression: a
    /// manager that returned the declined (stale) view would hand a
    /// warm-cache validator a superseded view; the bounded re-loop rebuilds
    /// against the now-current token instead. The knob disarms itself after
    /// firing.
    pub(crate) fn arm_publish_decline_once_for_tests() {
        FORCE_PUBLISH_DECLINE_ONCE.with(|c| c.set(true));
    }

    /// Test-only: drive a SINGLE `build` attempt with a mid-build env-hash
    /// mutation injected AFTER the per-canonical snapshot maps are populated
    /// but BEFORE the token dimensions are stamped (the
    /// [`FORCE_MID_BUILD_ENV_BUMP`] one-shot knob, which advances
    /// `resolve_env_hash` WITHOUT bumping `store_view_epoch`).
    ///
    /// Returns `(view, pre_token, live_token)` where `pre_token` is the
    /// single complete token captured BEFORE snapshotting and `live_token`
    /// is captured AFTER the build (reflecting the mid-build env mutation).
    ///
    /// Discriminates the build-coherence contract:
    ///
    /// * `view.validation_token()` MUST equal `pre_token` (the view was
    ///   stamped entirely from the pre-build capture, so its env fold
    ///   matches the snapshot maps that were also captured under the OLD
    ///   env) and MUST differ from `live_token` (which reflects the NEW
    ///   env) — so `build_coherent` rejects the attempt and retries.
    /// * Were `build` to re-read env LATE, `view.validation_token()` would
    ///   equal `live_token` (both the NEW env), and the torn view (NEW-env
    ///   token over OLD-env snapshot maps) would be accepted as coherent.
    pub(crate) fn build_one_attempt_with_mid_build_env_bump_for_tests(
        host: &VerterHost,
    ) -> (
        HostStoreView,
        StoreViewValidationToken,
        StoreViewValidationToken,
    ) {
        let pre = PreBuildTokenInputs::capture(host);
        let pre_token = pre.token();
        FORCE_MID_BUILD_ENV_BUMP.with(|c| c.set(true));
        let view = Self::build(host, &pre, None);
        // Defensive: ensure the knob is disarmed even if `build` did not
        // reach the firing point (it always does, but keep it leak-proof).
        FORCE_MID_BUILD_ENV_BUMP.with(|c| c.set(false));
        let live_token = StoreViewValidationToken::capture(host);
        (view, pre_token, live_token)
    }

    /// Test-only: forget that `canonical` was tracked. The view loses
    /// its `whole_hashes` entry (so the `FileWholeHash` / resolve-
    /// imports validators see it as untracked) but retains all other
    /// snapshot state (`resolved_import_facts`, `env_hashes`, etc.).
    /// Used by the discriminating
    /// test to simulate a base view that pre-dates a mid-request
    /// `ensure_loaded` promotion of the canonical.
    pub(crate) fn forget_whole_hash_for_tests(&mut self, canonical: &str) {
        Arc::make_mut(&mut self.snapshot)
            .whole_hashes
            .remove(canonical);
    }

    /// Test-only: peek the view's `whole_hashes` entry for a canonical
    /// id. The discriminating test
    /// reads the owner's authoritative content hash here so it can
    /// stage the overlay's `whole_hashes` entry with the same hash
    /// the producer admitted under.
    pub(crate) fn whole_hashes_get_for_tests(&self, canonical: &str) -> Option<Hash16> {
        self.snapshot.whole_hashes.get(canonical).copied()
    }

    /// Test-only: raw pointer identity of the shared
    /// `Arc<StoreViewSnapshot>`. Two views that share one snapshot
    /// (`from_host` token-stable reuse, or a no-op-overlay
    /// `with_session_overlay`) report the SAME pointer; a rebuilt or
    /// copy-on-write-cloned snapshot reports a different one. Lets the
    /// `StoreViewManager` Arc-reuse tests assert sharing via pointer
    /// identity.
    pub(crate) fn snapshot_ptr_for_tests(&self) -> *const StoreViewSnapshot {
        Arc::as_ptr(&self.snapshot)
    }

    /// Test-only: a `Weak` to this view's shared
    /// `Arc<StoreViewSnapshot>`. The close-cleanup regression downgrades
    /// the cached view's snapshot here, drops its own strong refs, then
    /// asserts the `Weak` fails to upgrade after `close()` — proving the
    /// manager dropped its cached `Arc` (token-bump alone would keep it
    /// alive).
    pub(crate) fn snapshot_weak_for_tests(&self) -> std::sync::Weak<StoreViewSnapshot> {
        Arc::downgrade(&self.snapshot)
    }

    /// Test-only: the complete validation token for this view.
    pub(crate) fn validation_token_for_tests(&self) -> StoreViewValidationToken {
        self.validation_token()
    }

    /// Test-only: run [`Self::build_coherent`] with EVERY retry attempt
    /// forced to observe a mid-build mutation, so the builder exhausts
    /// its retries and reports supersession. Returns `true` iff the
    /// outcome was [`SnapshotBuildOutcome::Superseded`] — i.e. the
    /// builder refused to publish a torn view. Discriminates against the
    /// retired "retry 3× then return-anyway" behaviour, which would have
    /// published a (potentially torn) view rather than reporting
    /// supersession.
    pub(crate) fn build_coherent_is_superseded_for_tests(host: &VerterHost) -> bool {
        FORCE_SUPERSEDE_ATTEMPTS.with(|c| c.set(STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS));
        let outcome = Self::build_coherent(host, None);
        // Reset the knob so it cannot leak into a later build on this
        // thread.
        FORCE_SUPERSEDE_ATTEMPTS.with(|c| c.set(0));
        matches!(outcome, SnapshotBuildOutcome::Superseded { .. })
    }
}
