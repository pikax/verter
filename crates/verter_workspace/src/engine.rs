use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use arc_swap::{ArcSwap, ArcSwapOption};
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use verter_scheduler::invalidation::Hash16;

use crate::ambient_lib::AmbientLibsByProject;
use crate::changes::{ChangeResult, WorkspaceChange};
use crate::dir_index::DirIndex;
use crate::env_hash::{EnvHashInputs, IdeProjectConfigEnvHash};
use crate::exact_resolution::{DependencySnapshotView, EdgeStore};
use crate::memory::MemorySnapshot;
use crate::module_resolution::{ConditionSet, ModuleResolutionMode};
use crate::overlay::OverlayStore;
use crate::package_index::PackageIndex;
use crate::project_graph::ProjectGraph;
use crate::published_state::{ProjectEnvHashArray, PublishedRoot};
use crate::resolution_currency::{
    explicit_context, manifest_resolution_fingerprint, selected_context_for_path,
    CanonicalResolutionId, CapturedResolutionWorld, ObservedResolutionValues, ResolutionEpoch,
    ResolutionFactKey, ResolutionFactVersion, ResolutionOutcome, ResolutionQueryKey,
    ResolutionSessionRoot, ResolutionTransaction, ResolutionWorldRoot, ResolveContextId,
    TransactionReader,
};
use crate::traits::WorkspaceResourceSnapshot;
use crate::types::{ExactResolution, ExactResolutionResult, VfsProvenance};
use crate::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use crate::{SignatureAdmission, CANDIDATE_CAP};
use verter_semantic::resolver_core::{
    IdeProjectConfig, ResolutionPopulation, ResolutionWorldId, ResolvePhase, ResolveRequestKind,
    ResolveResult, SessionFingerprint,
};

static NEXT_STRICT_SELF_ROOT_AUTHORITY_ID: AtomicU64 = AtomicU64::new(1);

fn next_strict_self_root_authority_id() -> u64 {
    NEXT_STRICT_SELF_ROOT_AUTHORITY_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .expect("strict self-root authority id space exhausted")
}

/// Path-segment marker used by the package classification helpers to
/// detect node_modules-rooted paths.
const NODE_MODULES_SEGMENT: &str = "/node_modules/";

/// The single package-backed classification primitive: given `suffix`, the
/// remainder of a path after stripping some project root, whether the path
/// is package-backed content **under** that root rather than the project's
/// own source.
///
/// Every workspace-ownership decision in this crate is this predicate or its
/// complement — [`Engine::is_workspace_owned`] is the complement over the
/// project list, and resolve-context selection is the predicate itself. No
/// caller re-derives it from a path substring.
///
/// A root that itself lives inside `node_modules/` (legal under pnpm) owns
/// its files: only a `node_modules/` hop **between** the root and the path
/// counts.
pub(crate) fn suffix_crosses_node_modules(suffix: &str) -> bool {
    suffix.trim_start_matches('/').starts_with("node_modules/")
        || suffix.contains(NODE_MODULES_SEGMENT)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LazyResolutionCacheKey {
    importer_id: String,
    specifier: String,
    phase: ResolvePhase,
    kind: ResolveRequestKind,
    population: ResolutionPopulation,
}

#[derive(Debug, Clone)]
struct LazyResolutionCacheEntry {
    result: Option<ResolveResult>,
    query: ResolutionQueryKey,
    signature: crate::ReadSetSignature,
}

/// One bounded multi-candidate resolution slot.
///
/// Concurrent base/session/world resolutions of the same
/// `(importer, specifier, phase, kind, population)` coexist as distinct
/// candidates whose value-side `ReadSetSignature`s distinguish the
/// resolution inputs they observed. Retention is the shared per-slot
/// [`crate::CANDIDATE_CAP`] with FIFO eviction — the same policy the
/// session `ValidatedFactCache` slot applies — so a retarget never
/// silently discards the witness of the candidate it superseded.
type LazyResolutionCandidates = SmallVec<[LazyResolutionCacheEntry; CANDIDATE_CAP]>;

/// Push `candidate` onto a slot, evicting oldest-first at
/// [`crate::CANDIDATE_CAP`]. Mirrors the session `ValidatedFactCache`
/// admission policy exactly: append, then drain the front until the
/// slot is at the cap.
///
/// Returns the queries whose candidates aged out, so the caller can
/// remove their decision nodes in the same fence. An aged-out candidate
/// no longer has an answer behind it, so leaving its decision published
/// would let a consumer keep validating against a decision nothing can
/// serve — and would grow the graph without bound.
fn admit_resolution_candidate(
    slot: &mut LazyResolutionCandidates,
    candidate: LazyResolutionCacheEntry,
) -> Vec<ResolutionQueryKey> {
    let mut evicted = Vec::new();
    if slot.len() >= CANDIDATE_CAP {
        let drop_count = slot.len() - CANDIDATE_CAP + 1;
        evicted.extend(slot.drain(..drop_count).map(|entry| entry.query));
    }
    // The incoming candidate republishes this query's decision, so an
    // aged-out entry for the SAME query is a replacement rather than a
    // removal.
    evicted.retain(|query| *query != candidate.query);
    slot.push(candidate);
    evicted
}

/// Post-mutation realpath knowledge a content mutator can assert.
#[derive(Debug, Clone)]
pub(crate) enum BaseRealpathTransition {
    /// The backend cannot cheaply prove the post-mutation realpath. The
    /// recorded value is dropped so later comparisons stay conservative.
    Unknown,
    /// The post-mutation realpath is known (`None` = no realpath, e.g. a
    /// deleted path). Value-sensitive: an unchanged value advances nothing.
    Known(Option<String>),
}

struct SessionResolutionDomain {
    root: ArcSwap<ResolutionSessionRoot>,
    epoch: AtomicU64,
    write: Mutex<()>,
}

impl SessionResolutionDomain {
    fn new(root: ResolutionSessionRoot) -> Self {
        Self {
            root: ArcSwap::from_pointee(root),
            epoch: AtomicU64::new(0),
            write: Mutex::new(()),
        }
    }
}

struct CapturedResolutionFence {
    base_epoch: ResolutionEpoch,
    session_epoch: Option<ResolutionEpoch>,
    session_domain: Option<Arc<SessionResolutionDomain>>,
    world: Arc<CapturedResolutionWorld>,
}

struct ParsedEdgeInputs {
    parsed_resolved: BTreeSet<String>,
    unresolved_pairs: Vec<((String, ResolveRequestKind), String)>,
    bare_specifiers: Vec<(String, ResolveRequestKind)>,
}

/// Unforgeable crate-internal proof that a resolver call is owned by Engine's
/// sealed resolution transaction.
///
/// `ModuleResolverCore` accepts this only alongside a `TransactionReader`.
/// Keeping construction private to this module prevents sibling production
/// code from bypassing fact capture while preserving direct resolver unit
/// tests inside the resolver module.
pub(crate) struct TrackedResolutionCapability {
    _private: (),
}

impl TrackedResolutionCapability {
    fn new() -> Self {
        Self { _private: () }
    }

    #[cfg(test)]
    pub(crate) fn for_conversion_test() -> Self {
        Self::new()
    }
}

/// One canonical's re-observed resolution-visible values, read live before
/// the resolution-world write gate is entered.
struct ReobservedEvidence {
    canonical: String,
    live: crate::resolution_currency::LiveResolutionObservation,
}

/// What a resolution-world mutation did to the replacement root.
///
/// Three outcomes, not two, because "the value must persist" and "captured
/// roots are superseded" are different questions. A first-observation
/// baseline FILL moves no fact — no witness's meaning changed — so
/// republishing the world identity for it supersedes every in-flight
/// attempt's capture and forces a retry for nothing. But the filled value
/// still has to survive, or the next generation refills it and the family
/// never acquires the baseline that lets a real change be detected as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorldWrite {
    /// Nothing moved; discard the replacement.
    Discard,
    /// Retain the replacement under the SAME world identity and epoch: no
    /// fact advanced, so no capture is superseded and nothing retries.
    Retain,
    /// A fact advanced: mint a new identity and advance the epoch.
    Publish,
}

impl From<bool> for WorldWrite {
    fn from(published: bool) -> Self {
        if published {
            Self::Publish
        } else {
            Self::Discard
        }
    }
}

/// One observed value of one reobservable family, addressed by canonical.
struct ObservedBaselineValue<'a> {
    canonical: &'a str,
    value: ObservedFamilyValue<'a>,
}

/// The reobservable families, exhaustively. Adding a family here is
/// compile-forced into the one fold rule, the totality of the admission
/// fold, and the live observation triple at once.
#[derive(Debug, Clone, Copy)]
enum ObservedFamilyValue<'a> {
    Probe(verter_semantic::resolver_core::PathProbe),
    Realpath(Option<&'a str>),
    Manifest(Option<[u8; 16]>),
}

/// What one observed value did to the world's recorded baseline for its
/// family. The vocabulary of the ONE first-observation rule below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaselineFold {
    /// The recorded baseline already held this value.
    Unchanged,
    /// There was no recorded baseline; the value fills it.
    Filled,
    /// A recorded baseline held a DIFFERENT value: the observation reveals
    /// state newer than the recorded world.
    Conflicted,
}

impl BaselineFold {
    fn write(self) -> WorldWrite {
        match self {
            Self::Unchanged => WorldWrite::Discard,
            Self::Filled => WorldWrite::Retain,
            Self::Conflicted => WorldWrite::Publish,
        }
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Conflicted, _) | (_, Self::Conflicted) => Self::Conflicted,
            (Self::Filled, _) | (_, Self::Filled) => Self::Filled,
            (Self::Unchanged, Self::Unchanged) => Self::Unchanged,
        }
    }
}

/// Shared internal engine used by both `FilesystemWorkspace` and `MemoryWorkspace`.
///
/// All fields are wrapped in `RwLock` for interior mutability so that
/// `WorkspaceAccess` (which takes `&self`) can read and write state.
///
/// # Published state
///
/// The `published_state` field is the primary source of truth for ownership
/// and resolution. It starts as `None` before first publish. After the first
/// call to `publish_snapshot()`, it is always `Some`.
///
/// `set_project_graph()` and `configure_resolver()` both write to
/// `project_graph` and then call `rebuild_and_publish()` which atomically
/// publishes to `published_state`.
///
/// # Lock ordering
///
/// To prevent deadlocks, locks must be acquired in this order. `parking_lot`
/// grants neither reentrancy nor a global order, so a site that takes two of
/// these the other way round wedges against a site that takes them this way —
/// with no CPU burn, no timeout and no panic, which is the hardest failure in
/// this file to diagnose.
///
///  1. `resolution_world_write` — the resolution-world publication gate
///  2. `resolution_sessions` (read or write)
///  3. `overlay` (read or write)
///  4. `snapshot` (read or write)
///  5. `edges` (read or write)
///  6. `project_graph` (read only — write is rare)
///  7. `package_index` (read or write)
///  8. `dir_index` (read or write)
///  9. `lazy_resolution_cache` (read or write)
/// 10. `content_transitions` / `subtree_transitions` (read or write)
///
/// **`resolution_world_write` outranks BOTH evidence ledgers.** Gate → ledger
/// is the REQUIRED order and the one the code takes:
/// `bump_content_generation_for` acquires `pending_resolution_refresh` inside
/// the `mutate_resolution_world` closure, and the admission path holds the
/// gate while `fold_observed_base_evidence` acquires
/// `evidence_verified_generation`. Ledger → gate is FORBIDDEN: acquiring
/// either ledger and then entering `mutate_resolution_world` (or
/// `mutate_resolution_world_locked`) while still holding it ABBAs against both
/// of those sites. Hoisting a ledger acquisition above a world mutation to
/// "settle it first" is exactly that mistake, and nothing asserts against it —
/// it presents as a wedged request with no CPU burn, no timeout and no panic.
///
/// The two resolution-evidence ledgers — `pending_resolution_refresh` and
/// `evidence_verified_generation` — have no ordering relationship with EACH
/// OTHER and are NEVER held together, in either order. Read or write one,
/// release it, then touch the other. `refresh_resolution_evidence` settles
/// both before it enters the gate, one at a time, for that reason; that is a
/// property of that function, not a general rule that ledger work must
/// precede the gate.
///
/// `published_state` and `resolution_world` use lock-free `ArcSwap` — no
/// ordering constraints. `resolution_epoch` is an atomic, sequenced by
/// `resolution_world_write` rather than by a lock order of its own.
pub(crate) struct Engine {
    pub(crate) input_resolution_budgets: verter_semantic::resolver_core::InputResolutionBudgets,
    pub(crate) overlay: RwLock<OverlayStore>,
    pub(crate) snapshot: RwLock<MemorySnapshot>,
    pub(crate) edges: RwLock<EdgeStore>,
    lazy_resolution_cache: RwLock<FxHashMap<LazyResolutionCacheKey, LazyResolutionCandidates>>,
    /// Per-canonical pending-evidence ledger: canonicals whose content
    /// transitioned through [`Self::bump_content_generation_for`] and whose
    /// resolution-visible evidence has not been re-observed yet. The bump
    /// itself advances zero resolution facts; the resolve path — where a
    /// reader is in scope — re-observes exactly the intersection of this
    /// ledger with the canonicals a candidate's witness recorded, and
    /// advances only facts whose observed value actually changed.
    pending_resolution_refresh: RwLock<rustc_hash::FxHashSet<String>>,
    /// Per-canonical stamp: the content generation at which this canonical's
    /// base resolution evidence was last re-observed LIVE.
    ///
    /// Read only for a backend that answers
    /// [`crate::traits::WorkspaceRead::resolution_reuse_requires_evidence_reobservation`]
    /// `true` — one whose resolver-visible changes can arrive with no event.
    /// For such a backend a warm candidate may be reused inside the
    /// generation its evidence was confirmed at, and its FIRST reuse in a
    /// later generation re-confirms that evidence against live state.
    ///
    /// This is bookkeeping about WHEN evidence was last read, never a
    /// validity oracle: a reuse still requires the candidate's fact
    /// signature to validate against the captured world, and a re-observed
    /// value that disagrees advances its exact fact through the ordinary
    /// mutation protocol. The key space is the canonicals resolution
    /// witnesses name — the same set the world's own probe baseline holds.
    evidence_verified_generation: RwLock<FxHashMap<String, u64>>,
    pub(crate) content_generation: AtomicU64,
    /// Process-unique identity of this strict-self-root authority. Unlike the
    /// per-workspace generation, it cannot alias after a workspace swap.
    strict_self_root_authority_id: u64,
    /// Dedicated authority for strict structural self-root validation.
    /// Advances on every workspace transition that may change a strict
    /// whole-hash or trackedness answer, including publication-only changes.
    strict_self_root_generation: AtomicU64,
    /// Number of overlapping strict-self-root authority writers. A witness
    /// cannot be minted while this is non-zero; unlike an odd/even bit this
    /// remains sound when independent host-side membership writers overlap.
    strict_self_root_writers: AtomicU64,
    /// Counter behind the SOURCE-ENV compaction domain. See
    /// [`Engine::current_source_env_generation`].
    source_env_generation: AtomicU64,
    /// Immutable resolution-visible composition and its four-step publisher.
    resolution_world: ArcSwap<ResolutionWorldRoot>,
    resolution_epoch: AtomicU64,
    resolution_world_write: Mutex<()>,
    resolution_sessions: RwLock<FxHashMap<SessionFingerprint, Arc<SessionResolutionDomain>>>,
    default_resolution_session: SessionFingerprint,
    next_resolution_world_id: AtomicU64,
    next_resolution_fact_version: AtomicU64,
    /// Count of resolution fact advances a WITNESS COULD OBSERVE.
    ///
    /// Deliberately not the mint counter. Publishing a brand-new derived
    /// node mints a version — freshness comes from one global source, so
    /// a removal/reintroduction can never reproduce an old version — but
    /// no witness could have recorded that node, so nothing a witness
    /// observes moved. The session folds this counter into
    /// `StoreViewValidationToken`'s EXTERNAL-supersession set, where a
    /// cold compute's own work must never appear: a resolve that
    /// published its own decision node would otherwise fence its own
    /// promotion.
    resolution_fact_generation: AtomicU64,
    /// Project graph — the write-side store. Callers update this via
    /// `set_project_graph()` / `configure_resolver()`, then
    /// `rebuild_and_publish()` atomically derives and publishes a
    /// `WorkspaceSnapshot` + `ModuleResolverCore` to `published_state`.
    pub(crate) project_graph: RwLock<ProjectGraph>,
    configured_resolver_projects: RwLock<Option<Vec<IdeProjectConfig>>>,
    #[allow(dead_code)]
    pub(crate) package_index: RwLock<PackageIndex>,
    pub(crate) dir_index: RwLock<DirIndex>,
    pub(crate) vfs_provenance: VfsProvenance,

    /// Atomic published workspace state — primary source of truth for
    /// ownership and resolution.
    ///
    /// Always `Some` after `Engine::new()` — the constructor eagerly publishes
    /// an empty bootstrap snapshot (`ownership_ready: false`). After
    /// `background_init` builds the full project graph, a real snapshot with
    /// `ownership_ready: true` is published.
    pub(crate) published_state: ArcSwapOption<PublishedRoot>,
    /// TEST-ONLY: each publish sends the new snapshot generation so a
    /// waiter observes an exact publication receipt instead of polling
    /// `load_published`. UNBOUNDED on purpose — this send happens inside
    /// `mutate_resolution_world`, so a bounded channel with an armed but
    /// undrained receiver would stall a publish while the resolution world
    /// is locked. Gated to `test` / the opt-in `test-support` feature, so
    /// no production build carries the slot or the send.
    #[cfg(any(test, feature = "test-support"))]
    published_tx: Mutex<Vec<std::sync::mpsc::Sender<u64>>>,

    /// Per-project ambient TypeScript lib registry.
    ///
    /// Lock-free `ArcSwap` so reads (file shadowing checks, symbol lookup,
    /// dep-fact validation) never block on concurrent registrations. Concrete
    /// workspaces mutate via CAS in `register_ambient_lib`.
    pub(crate) ambient_libs: ArcSwap<AmbientLibsByProject>,

    /// Extension list used for reverse-dep stem stripping. Initialised to
    /// the merged static `probe_extensions()` + initial host config, sorted
    /// longest-first. `ArcSwap` so `set_default_resolve_extensions` does
    /// not stall reverse queries on the hot path.
    pub(crate) default_resolve_extensions: ArcSwap<Vec<String>>,

    /// Cached workspace-default env-hash array — see
    /// [`workspace_default_env_hash_array_for_engine`]. `None` until the
    /// first read; validated by pointer identity against
    /// [`Self::default_resolve_extensions`], so an extension republish
    /// invalidates it implicitly.
    workspace_default_env_hashes: ArcSwapOption<WorkspaceDefaultEnvHashes>,

    /// Per-canonical content-transition ledger: canonical id → the
    /// `content_generation` recorded at its most recent content
    /// transition (overlay write/clear, snapshot inject/remove, disk
    /// write/copy/delete). The workspace is the sole content authority,
    /// so this is the AUTHORITATIVE per-canonical freshness rail for
    /// consumers retaining content-derived artifacts: an artifact built
    /// at generation `G` for canonical `C` is provably content-fresh
    /// only while `G >= last_content_transition_generation(C)`. Unlike a
    /// global generation-equality clause, the ledger is per-canonical —
    /// unrelated transitions never invalidate an untouched canonical's
    /// retained artifacts (package reuse). Recording lives at the
    /// workspace mutation chokepoints, so mutators that bypass any
    /// host-level wrapper (direct embedder `notify_upsert`, `write_file`,
    /// `copy_file`) are covered by construction. Keys are normalized
    /// through [`verter_semantic::resolver_core::normalize_canonical_id`] at the
    /// recording chokepoints AND the query, so a direct embedder passing
    /// a non-canonical key form (backslashes, Windows drive casing)
    /// records under the same key the gate reads.
    ///
    /// Growth: insert-only, bounded by the number of DISTINCT canonicals
    /// ever mutated in this workspace instance — the same order as the
    /// snapshot/edge stores, which key per-canonical and are also never
    /// compacted; the ledger adds one `(String, u64)` per such canonical.
    content_transitions: RwLock<FxHashMap<String, u64>>,

    /// Per-SUBTREE content-transition ledger: directory prefix → the
    /// `content_generation` recorded at its most recent subtree-scoped
    /// mutation (`delete_dir_all`, a watcher `DirectoryTreeDirty`
    /// recovery). Those mutations change an UNKNOWN member set — the
    /// engine cannot enumerate every canonical a recursive disk delete
    /// or an out-of-band disk change touched — so they record the
    /// PREFIX instead; [`Self::last_content_transition_generation`]
    /// folds in every recorded prefix that contains the queried
    /// canonical. Same normalization and growth story as
    /// `content_transitions` (bounded by distinct mutated directory
    /// prefixes).
    subtree_transitions: RwLock<FxHashMap<String, u64>>,
}

struct StrictSelfRootTransition<'a>(&'a Engine);

impl Drop for StrictSelfRootTransition<'_> {
    fn drop(&mut self) {
        self.0.end_strict_self_root_transition();
    }
}

impl Engine {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_input_resolution_budgets(
            verter_semantic::resolver_core::InputResolutionBudgets::default(),
        )
    }

    pub(crate) fn new_with_input_resolution_budgets(
        input_resolution_budgets: verter_semantic::resolver_core::InputResolutionBudgets,
    ) -> Self {
        static NEXT_SESSION_FINGERPRINT: AtomicU64 = AtomicU64::new(1);
        let initial_extensions: Vec<String> = Self::merge_extensions(&[]);
        let initial_resolution_world = Arc::new(ResolutionWorldRoot::bootstrap(
            ResolutionWorldId::from_raw(1),
        ));
        let default_resolution_session =
            SessionFingerprint::from_raw(NEXT_SESSION_FINGERPRINT.fetch_add(1, Ordering::Relaxed));
        let default_session_domain = Arc::new(SessionResolutionDomain::new(
            ResolutionSessionRoot::bootstrap(ResolutionWorldId::from_raw(2)),
        ));
        let mut resolution_sessions = FxHashMap::default();
        resolution_sessions.insert(default_resolution_session, default_session_domain);
        let engine = Self {
            input_resolution_budgets,
            overlay: RwLock::new(OverlayStore::new()),
            snapshot: RwLock::new(MemorySnapshot::new()),
            edges: RwLock::new(EdgeStore::new()),
            lazy_resolution_cache: RwLock::new(FxHashMap::default()),
            pending_resolution_refresh: RwLock::new(rustc_hash::FxHashSet::default()),
            evidence_verified_generation: RwLock::new(FxHashMap::default()),
            content_generation: AtomicU64::new(1),
            strict_self_root_authority_id: next_strict_self_root_authority_id(),
            strict_self_root_generation: AtomicU64::new(1),
            strict_self_root_writers: AtomicU64::new(0),
            source_env_generation: AtomicU64::new(1),
            resolution_world: ArcSwap::from(initial_resolution_world),
            resolution_epoch: AtomicU64::new(0),
            resolution_world_write: Mutex::new(()),
            resolution_sessions: RwLock::new(resolution_sessions),
            default_resolution_session,
            next_resolution_world_id: AtomicU64::new(3),
            next_resolution_fact_version: AtomicU64::new(1),
            resolution_fact_generation: AtomicU64::new(1),
            project_graph: RwLock::new(ProjectGraph::new()),
            configured_resolver_projects: RwLock::new(None),
            package_index: RwLock::new(PackageIndex::new()),
            dir_index: RwLock::new(DirIndex::new()),
            vfs_provenance: VfsProvenance::default(),
            published_state: ArcSwapOption::new(None),
            #[cfg(any(test, feature = "test-support"))]
            published_tx: Mutex::new(Vec::new()),
            ambient_libs: ArcSwap::from_pointee(AmbientLibsByProject::default()),
            default_resolve_extensions: ArcSwap::from_pointee(initial_extensions),
            workspace_default_env_hashes: ArcSwapOption::new(None),
            content_transitions: RwLock::new(FxHashMap::default()),
            subtree_transitions: RwLock::new(FxHashMap::default()),
        };
        // Publish an initial snapshot from the empty project graph so that
        // `published_state` is always `Some`. This ensures basic relative
        // path resolution works immediately, before any `set_project_graph()`
        // or `configure_resolver()` call populates real project configs.
        engine.rebuild_and_publish();
        engine
    }

    /// Merge `host_resolve_extensions` with the workspace's static
    /// `probe_extensions()` list, dedupe, and sort by descending length
    /// then ascending lex. Used by [`Engine::new`] and
    /// [`Engine::set_default_resolve_extensions`] (single source of truth).
    fn merge_extensions(host_resolve_extensions: &[String]) -> Vec<String> {
        let mut merged: BTreeSet<String> = verter_semantic::resolver_core::probe_extensions()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        for ext in host_resolve_extensions {
            merged.insert(ext.clone());
        }
        let mut sorted: Vec<String> = merged.into_iter().collect();
        sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        sorted
    }

    /// Replace the workspace's reverse-dep extension list (additive: merges
    /// with `probe_extensions()` and sorts longest-first at set-time).
    /// Lock-free swap; does not stall reverse queries.
    ///
    /// An extension-priority change is a resolve-config mutation: the merged
    /// extension set feeds every project's `resolve_env_hash` (R21), and
    /// resolution outcomes (effective targets, resolved wildcard canonicals)
    /// depend on it. When the merged list actually changes, recompose +
    /// republish the env-hash tables (so RouteDb effective-export-set entries
    /// keyed on the OLD `resolve_env_hash` become unreachable) and advance
    /// `content_generation` (so the session-side epoch consumers — route-
    /// surface edge currency (`indexed_surface_is_current`) and known-miss
    /// staleness checks — invalidate). This mirrors the
    /// [`Self::set_default_resolve_extensions`] sibling resolver-config
    /// mutation [`crate::traits::WorkspaceAccess::configure_resolver`], which
    /// also republishes through `rebuild_and_publish`.
    ///
    /// TODO(follow-up): the BROADER runtime resolver-config-mutation API —
    /// a host-level `VerterHost` setter that drives the full
    /// `configure_projects`-style cascade on the session side (clear derived
    /// route state, reset resolver caches, bump project/store-view
    /// generations) for resolve-config changes that are NOT extension-list
    /// changes — is not yet wired; until then only the extension-list
    /// dimension invalidates the host route memo through this path.
    pub(crate) fn set_default_resolve_extensions(&self, host_resolve_extensions: Vec<String>) {
        let sorted = Self::merge_extensions(&host_resolve_extensions);
        let changed = **self.default_resolve_extensions.load() != sorted;
        self.default_resolve_extensions.store(Arc::new(sorted));
        if changed {
            self.rebuild_and_publish();
            self.bump_content_generation();
            // NO separate source-env bump here: `rebuild_and_publish`
            // above already advanced it, and a second bump would be an
            // unfalsifiable producer claim — removing it changes no
            // observable.
        }
    }

    pub(crate) fn set_configured_resolver_projects(&self, projects: Option<Vec<IdeProjectConfig>>) {
        *self.configured_resolver_projects.write() = projects;
    }

    /// Publish a workspace snapshot atomically.
    ///
    /// After this call, all readers loading from `published_state` see the
    /// new snapshot. One store, one generation.
    pub(crate) fn publish_snapshot(&self, mut root: PublishedRoot) {
        let _strict_transition = self.strict_self_root_transition();
        let tables_complete = root.snapshot.projects.iter().all(|project| {
            root.env_hashes_by_project.contains_key(&project.id)
                && root.project_identity_hashes.contains_key(&project.id)
        });
        if !tables_complete {
            let extensions = self.default_resolve_extensions.load_full();
            let (env_hashes, identities) =
                compose_env_hash_tables(&root.snapshot.projects, &extensions);
            root.env_hashes_by_project = env_hashes;
            root.project_identity_hashes = identities;
        }
        // Publishing a snapshot republishes the per-project env-hash
        // tables (`parse_env_hash`, project identity) with no content
        // bump, so the source-env domain advances here.
        self.bump_source_env_generation();
        self.mutate_resolution_world(|world| {
            let root = Arc::new(root);
            self.published_state.store(Some(Arc::clone(&root)));
            #[cfg(any(test, feature = "test-support"))]
            self.notify_published(root.snapshot.generation.0);
            world.replace_published(root, &self.registered_session_context_keys(), || {
                self.next_resolution_fact_version()
            });
            ((), true)
        });
    }

    #[cfg(any(test, feature = "test-support"))]
    fn notify_published(&self, generation: u64) {
        // Every live subscriber gets the receipt; a dropped receiver makes its
        // sender fail and is pruned here.
        self.published_tx
            .lock()
            .retain(|tx| tx.send(generation).is_ok());
    }

    /// TEST-ONLY: subscribe to snapshot publications. Arm before the
    /// publish that should wake the waiter; a publication that already
    /// landed is observed by `load_published` first.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn subscribe_published(&self) -> std::sync::mpsc::Receiver<u64> {
        // Subscribers ACCUMULATE. A single-slot design silently orphans an
        // earlier waiter the moment a test subscribes twice, which surfaces as
        // an unexplained hang rather than an error.
        let (tx, rx) = std::sync::mpsc::channel();
        self.published_tx.lock().push(tx);
        rx
    }

    pub(crate) fn current_content_generation(&self) -> u64 {
        self.content_generation.load(Ordering::Relaxed)
    }

    pub(crate) fn current_strict_self_root_generation(&self) -> u64 {
        self.strict_self_root_generation.load(Ordering::Acquire)
    }

    pub(crate) fn strict_self_root_authority_id(&self) -> u64 {
        self.strict_self_root_authority_id
    }

    pub(crate) fn bump_strict_self_root_generation(&self) -> u64 {
        self.strict_self_root_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1
    }

    pub(crate) fn strict_self_root_transition_active(&self) -> bool {
        self.strict_self_root_writers.load(Ordering::Acquire) != 0
    }

    pub(crate) fn begin_strict_self_root_transition(&self) {
        self.strict_self_root_writers.fetch_add(1, Ordering::AcqRel);
        self.bump_strict_self_root_generation();
    }

    pub(crate) fn end_strict_self_root_transition(&self) {
        self.bump_strict_self_root_generation();
        let previous = self.strict_self_root_writers.fetch_sub(1, Ordering::AcqRel);
        assert!(
            previous > 0,
            "strict self-root transition ended without a writer"
        );
    }

    fn strict_self_root_transition(&self) -> StrictSelfRootTransition<'_> {
        self.begin_strict_self_root_transition();
        StrictSelfRootTransition(self)
    }

    /// Live generation of the SOURCE-ENV compaction domain: the counter
    /// behind every `FileSourceEnv` observation.
    ///
    /// Its own domain, deliberately NOT folded into the content counter.
    /// `FileSourceEnv` carries `parse_env_hash` / `parse_key` /
    /// `file_language_id`, and the production paths that move them —
    /// `publish_snapshot`, `rebuild_and_publish` (both reached through
    /// `configure_projects` / `configure_resolver`) and
    /// `WorkspaceChange::ConfigChanged` — do NOT bump
    /// `content_generation`. A source-env fact folded into the content
    /// domain would therefore survive a parse-env or file-language
    /// change, which is a new poisoning class rather than a bounded
    /// coarsening.
    /// Read in production through [`crate::WorkspaceAccess::source_env_generation`],
    /// which is the seam the session's store view captures it from.
    pub(crate) fn current_source_env_generation(&self) -> u64 {
        self.source_env_generation.load(Ordering::Relaxed)
    }

    fn bump_source_env_generation(&self) -> u64 {
        self.source_env_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn bump_content_generation_in_world(&self) -> u64 {
        self.content_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn record_content_transition_at(&self, canonical_id: &str, generation: u64) {
        self.content_transitions.write().insert(
            verter_semantic::resolver_core::normalize_canonical_id(canonical_id),
            generation,
        );
    }

    fn record_subtree_content_transition_at(&self, prefix: &str, generation: u64) {
        let mut normalized = verter_semantic::resolver_core::normalize_canonical_id(prefix);
        while normalized.len() > 1 && normalized.ends_with('/') {
            normalized.pop();
        }
        self.subtree_transitions
            .write()
            .insert(normalized, generation);
    }

    pub(crate) fn bump_content_generation(&self) -> u64 {
        self.mutate_resolution_world(|_| {
            let generation = self.bump_content_generation_in_world();
            (generation, true)
        })
    }

    /// Per-canonical content transition: bump `content_generation` AND
    /// record the post-bump generation against `canonical_id` in the
    /// transition ledger. EVERY per-canonical content mutator routes its
    /// generation bump through this helper (the recording chokepoint);
    /// `bump_content_generation` alone remains for canonical-less
    /// mutations (config/extension changes).
    ///
    /// This advances ZERO resolution facts speculatively: the canonical
    /// enters the pending-evidence ledger and the resolve path — where a
    /// reader is in scope — re-observes it value-sensitively
    /// ([`Self::refresh_resolution_evidence`]). The world identity still
    /// advances so an in-flight transaction that straddles the transition
    /// retries instead of admitting mixed-world observations.
    ///
    /// Native-only: its production callers are the ambient-library
    /// registration pair, which does not exist on `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn bump_content_generation_for(&self, canonical_id: &str) -> u64 {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        self.mutate_resolution_world(|_world| {
            self.pending_resolution_refresh
                .write()
                .insert(canonical.clone());
            let generation = self.bump_content_generation_in_world();
            self.record_content_transition_at(&canonical, generation);
            (generation, true)
        })
    }

    /// Record a content transition for `canonical_id` at the CURRENT
    /// generation without bumping — for multi-canonical mutations that
    /// bump once after recording every affected id.
    pub(crate) fn record_content_transition(&self, canonical_id: &str) {
        let generation = self.current_content_generation();
        self.record_content_transition_at(canonical_id, generation + 1);
    }

    /// Record a SUBTREE content transition for every canonical under
    /// `prefix` (inclusive) at the current generation, without bumping —
    /// for directory-scoped mutations whose member set the engine cannot
    /// enumerate (`delete_dir_all`, watcher `DirectoryTreeDirty`
    /// recovery). Callers bump once after recording, exactly like
    /// [`Self::record_content_transition`].
    #[allow(dead_code)]
    pub(crate) fn record_subtree_content_transition(&self, prefix: &str) {
        let generation = self.current_content_generation();
        self.record_subtree_content_transition_at(prefix, generation + 1);
    }

    /// The generation recorded at `canonical_id`'s most recent content
    /// transition; `0` when the canonical has never transitioned. Folds
    /// the exact per-canonical record with every recorded SUBTREE prefix
    /// containing the canonical (a `delete_dir_all` / watcher recovery
    /// transitions every member of the subtree).
    pub(crate) fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        let exact = self
            .content_transitions
            .read()
            .get(&canonical)
            .copied()
            .unwrap_or(0);
        let subtree = self
            .subtree_transitions
            .read()
            .iter()
            // Boundary-correct subtree containment through the shared
            // `path_matches_prefix` chokepoint — the recorded root
            // prefix `"/"` folds into every canonical (a raw
            // next-byte-is-`'/'` check can never match it), while a
            // byte-prefix sibling (`/srcx.ts` under `/src`) never
            // matches. Recorded prefixes are normalized at
            // [`Self::record_subtree_content_transition`]; the helper
            // re-normalizes on read so both sides agree.
            .filter(|(prefix, _)| crate::path_matches_prefix(canonical.as_str(), prefix))
            .map(|(_, generation)| *generation)
            .max()
            .unwrap_or(0);
        exact.max(subtree)
    }

    /// Four-step resolution-world publication protocol. The callback performs
    /// the state mutation while the epoch is odd, then updates the replacement
    /// immutable root. A semantic no-op restores the original even epoch
    /// without publishing a new identity.
    fn mutate_resolution_world<R, W: Into<WorldWrite>>(
        &self,
        mutation: impl FnOnce(&mut ResolutionWorldRoot) -> (R, W),
    ) -> R {
        let _write = self.resolution_world_write.lock();
        self.mutate_resolution_world_locked(mutation)
    }

    /// The session fingerprint whose write gate a caller already holds,
    /// so the base publication protocol's session fan-out reuses it
    /// instead of re-locking a non-reentrant mutex against itself.
    fn held_session_of(captured: &CapturedResolutionFence) -> Option<SessionFingerprint> {
        match (captured.session_domain.as_ref(), captured.world.population) {
            (Some(_), ResolutionPopulation::Session(fingerprint)) => Some(fingerprint),
            _ => None,
        }
    }

    /// Apply a mutation only when the caller's captured world is still the
    /// current stable world. Validation and the odd/even publication protocol
    /// share the same write-gate critical section, so no writer can land
    /// between the final fence and the mutation.
    fn mutate_resolution_world_if_current<R, W: Into<WorldWrite>>(
        &self,
        captured: &CapturedResolutionFence,
        mutation: impl FnOnce(&mut ResolutionWorldRoot) -> (R, W),
    ) -> Result<R, ()> {
        let _write = self.resolution_world_write.lock();
        let _session_write = captured
            .session_domain
            .as_ref()
            .map(|domain| domain.write.lock());
        if !self.resolution_world_still_current(captured) {
            return Err(());
        }
        Ok(self.mutate_resolution_world_locked_with_held_session(
            Self::held_session_of(captured),
            mutation,
        ))
    }

    /// Publication implementation for callers already holding
    /// `resolution_world_write`.
    fn mutate_resolution_world_locked<R, W: Into<WorldWrite>>(
        &self,
        mutation: impl FnOnce(&mut ResolutionWorldRoot) -> (R, W),
    ) -> R {
        self.mutate_resolution_world_locked_with_held_session(None, mutation)
    }

    /// [`Self::mutate_resolution_world_locked`] for a caller that already
    /// holds one session's write gate — the parsed-edge recorder and the
    /// resolve fence both do, and `parking_lot::Mutex` is not reentrant,
    /// so the session fan-out must be told which domain not to re-lock.
    fn mutate_resolution_world_locked_with_held_session<R, W: Into<WorldWrite>>(
        &self,
        held_session: Option<SessionFingerprint>,
        mutation: impl FnOnce(&mut ResolutionWorldRoot) -> (R, W),
    ) -> R {
        let stable = self.resolution_epoch.load(Ordering::Acquire);
        assert_eq!(
            stable % 2,
            0,
            "resolution-world writer entered while publication was already active"
        );
        self.resolution_epoch
            .store(stable.wrapping_add(1), Ordering::Release);

        struct RestoreEpoch<'a> {
            epoch: &'a AtomicU64,
            value: u64,
            armed: bool,
        }
        impl Drop for RestoreEpoch<'_> {
            fn drop(&mut self) {
                if self.armed {
                    self.epoch.store(self.value, Ordering::Release);
                }
            }
        }

        let mut restore = RestoreEpoch {
            epoch: &self.resolution_epoch,
            value: stable,
            armed: true,
        };
        let current = self.resolution_world.load_full();
        let mut replacement = (*current).clone();
        let (result, write) = mutation(&mut replacement);
        let write = write.into();
        // Step 3: every direct base fact this batch advanced propagates
        // ONCE over the base graph's reverse edges, before anything is
        // published. Nothing is evicted — a dependent cache entry becomes
        // cold only when its own recorded derived version fails ordinary
        // read-side validation.
        let base_seeds = replacement.facts.take_pending_seeds();
        if !base_seeds.is_empty() && !matches!(write, WorldWrite::Discard) {
            replacement.facts.propagate(base_seeds.iter().cloned(), || {
                self.next_resolution_fact_version()
            });
            // Step 4/5: the same changed BASE keys propagate through every
            // live session graph, and each changed session root is
            // published — all of it while the base epoch is still ODD, so
            // no capture can pair a new base root with an unpropagated
            // session decision root.
            self.propagate_base_changes_into_sessions(&replacement, &base_seeds, held_session);
        }
        match write {
            WorldWrite::Discard => {}
            WorldWrite::Retain => {
                // Same identity, same epoch: a capture taken before this
                // store stays current, because nothing a witness can observe
                // moved. Only the recorded baseline grew.
                self.resolution_world.store(Arc::new(replacement));
            }
            WorldWrite::Publish => {
                replacement.id = ResolutionWorldId::from_raw(
                    self.next_resolution_world_id
                        .fetch_add(1, Ordering::Relaxed),
                );
                self.resolution_world.store(Arc::new(replacement));
                restore.value = stable.wrapping_add(2);
            }
        }
        restore.armed = false;
        self.resolution_epoch
            .store(restore.value, Ordering::Release);
        result
    }

    /// The `ContextSelection` leaves registered in every LIVE session
    /// graph, normalised to the base population.
    ///
    /// A session decision records its context edge in its own root, so
    /// the base world's own reverse map names none of them. The caller
    /// holds the base publication gate, and this only reads published
    /// session roots through their `ArcSwap`.
    fn registered_session_context_keys(&self) -> Vec<ResolutionFactKey> {
        self.resolution_sessions
            .read()
            .values()
            .flat_map(|domain| {
                domain
                    .root
                    .load()
                    .facts
                    .registered_context_selection_keys()
                    .into_iter()
                    .map(|key| key.in_population(ResolutionPopulation::Base))
            })
            .collect()
    }

    /// **Step 4 and 5 of the base publication protocol.** Propagate the
    /// base keys this batch advanced through every live session graph and
    /// publish each session root that changed.
    ///
    /// A session decision records SESSION-population edges whose versions
    /// fall back to the base root, so a base advance is invisible to the
    /// session graph until its keys are translated into that population.
    /// Skipping this step is a stale serve, not an optimisation: the
    /// session decision keeps validating across a base mutation it
    /// genuinely depends on.
    ///
    /// The caller holds the base publication gate and the base epoch is
    /// ODD for the whole traversal — `capture_resolution_world` cannot
    /// complete at all while it is, so the intermediate state where a new
    /// base root is paired with an unpropagated session root is
    /// unreachable rather than merely unpaired. Lock order is
    /// base gate → `resolution_sessions` → session write, exactly the
    /// documented order.
    ///
    /// Cost: `O(changed base keys × live sessions)` seeds plus the
    /// reachable-subgraph propagation in each. `resolution_sessions` is
    /// never pruned — production interns exactly one session domain per
    /// engine, but if multi-session ever lands this traversal grows with
    /// the accumulated, never-reaped domain count while the global base
    /// gate is held.
    fn propagate_base_changes_into_sessions(
        &self,
        base: &ResolutionWorldRoot,
        base_seeds: &[ResolutionFactKey],
        held_session: Option<SessionFingerprint>,
    ) {
        verter_debug_assert!(
            !crate::resolution_currency::ResolutionEpoch::from_raw(
                self.resolution_epoch.load(Ordering::Acquire)
            )
            .is_stable(),
            "session propagation must run inside the base publication window: a session \
             root published under a STABLE base epoch can be captured paired with the \
             pre-mutation base root"
        );
        let domains: Vec<(SessionFingerprint, Arc<SessionResolutionDomain>)> = self
            .resolution_sessions
            .read()
            .iter()
            .map(|(fingerprint, domain)| (*fingerprint, Arc::clone(domain)))
            .collect();
        for (fingerprint, domain) in domains {
            let population = ResolutionPopulation::Session(fingerprint);
            let seeds: Vec<ResolutionFactKey> = base_seeds
                .iter()
                .map(|key| key.in_population(population))
                .collect();
            let _session_write = (held_session != Some(fingerprint)).then(|| domain.write.lock());
            self.mutate_resolution_session_write_held(&domain, base, |_base, session| {
                let advanced = session
                    .facts
                    .propagate(seeds, || self.next_resolution_fact_version());
                (
                    (),
                    if advanced.is_empty() {
                        WorldWrite::Discard
                    } else {
                        WorldWrite::Publish
                    },
                )
            });
        }
    }

    /// The number of resolution fact advances a witness could observe.
    ///
    /// One count per fact whose observed value actually moved (a
    /// first-observation baseline fill counts nothing, and neither does
    /// publishing a derived node no witness can have recorded), so a
    /// consumer retaining a captured world can tell whether ANY fact it
    /// could have observed has advanced since its capture without
    /// enumerating facts.
    pub(crate) fn current_resolution_fact_generation(&self) -> u64 {
        self.resolution_fact_generation.load(Ordering::Relaxed)
    }

    /// Mint a fresh version for a fact whose observed value MOVED.
    fn next_resolution_fact_version(&self) -> ResolutionFactVersion {
        self.resolution_fact_generation
            .fetch_add(1, Ordering::Relaxed);
        self.mint_fact_version()
    }

    /// Mint a fresh version WITHOUT claiming an observable advance.
    ///
    /// The single global mint source, so a version is unique across
    /// every family and a removed-and-reintroduced node can never
    /// reproduce one a witness holds. Only the observable-advance count
    /// is withheld.
    fn mint_fact_version(&self) -> ResolutionFactVersion {
        ResolutionFactVersion::fresh(
            self.next_resolution_fact_version
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    pub(crate) fn default_resolution_population(&self) -> ResolutionPopulation {
        ResolutionPopulation::Session(self.default_resolution_session)
    }

    fn session_resolution_domain(
        &self,
        fingerprint: SessionFingerprint,
    ) -> Arc<SessionResolutionDomain> {
        if let Some(domain) = self.resolution_sessions.read().get(&fingerprint) {
            return Arc::clone(domain);
        }
        let mut domains = self.resolution_sessions.write();
        Arc::clone(domains.entry(fingerprint).or_insert_with(|| {
            Arc::new(SessionResolutionDomain::new(
                ResolutionSessionRoot::bootstrap(ResolutionWorldId::from_raw(
                    self.next_resolution_world_id
                        .fetch_add(1, Ordering::Relaxed),
                )),
            ))
        }))
    }

    /// Publish one session-overlay mutation under that session's independent
    /// odd/even gate. The base gate is held only to pin the fallback root while
    /// the replacement overlay root is constructed; the base epoch/root are
    /// not advanced by a session-only edit.
    fn mutate_resolution_session<R>(
        &self,
        fingerprint: SessionFingerprint,
        mutation: impl FnOnce(&ResolutionWorldRoot, &mut ResolutionSessionRoot) -> (R, bool),
    ) -> R {
        crate::probe_scope!(MUTATE_RESOLUTION_SESS);
        let domain = self.session_resolution_domain(fingerprint);
        let _base_read_fence = self.resolution_world_write.lock();
        let base = self.resolution_world.load_full();
        self.mutate_resolution_session_locked(&domain, base.as_ref(), mutation)
    }

    fn mutate_resolution_session_locked<R, W: Into<WorldWrite>>(
        &self,
        domain: &Arc<SessionResolutionDomain>,
        base: &ResolutionWorldRoot,
        mutation: impl FnOnce(&ResolutionWorldRoot, &mut ResolutionSessionRoot) -> (R, W),
    ) -> R {
        let _session_write = domain.write.lock();
        self.mutate_resolution_session_write_held(domain, base, mutation)
    }

    /// Session publication for a caller ALREADY holding
    /// `domain.write` — the resolve fence, which takes that gate before
    /// its final currency check so no writer can land between the check
    /// and the publication.
    ///
    /// `parking_lot::Mutex` is not reentrant, so this split is load
    /// bearing: routing the fence through
    /// [`Self::mutate_resolution_session_locked`] would wedge with no CPU
    /// burn, no timeout and no panic.
    fn mutate_resolution_session_write_held<R, W: Into<WorldWrite>>(
        &self,
        domain: &Arc<SessionResolutionDomain>,
        base: &ResolutionWorldRoot,
        mutation: impl FnOnce(&ResolutionWorldRoot, &mut ResolutionSessionRoot) -> (R, W),
    ) -> R {
        let stable = domain.epoch.load(Ordering::Acquire);
        assert_eq!(
            stable % 2,
            0,
            "session resolution publisher entered from an unstable epoch"
        );
        domain
            .epoch
            .store(stable.wrapping_add(1), Ordering::Release);

        struct RestoreEpoch<'a> {
            epoch: &'a AtomicU64,
            value: u64,
            armed: bool,
        }
        impl Drop for RestoreEpoch<'_> {
            fn drop(&mut self) {
                if self.armed {
                    self.epoch.store(self.value, Ordering::Release);
                }
            }
        }

        let mut restore = RestoreEpoch {
            epoch: &domain.epoch,
            value: stable,
            armed: true,
        };
        let current = domain.root.load_full();
        let mut replacement = (*current).clone();
        let (result, write) = mutation(base, &mut replacement);
        let write = write.into();
        // Step 3 of the publication protocol, session side: every direct
        // fact this batch advanced propagates ONCE over the session
        // graph's reverse edges before the root is published.
        let seeds = replacement.facts.take_pending_seeds();
        if !seeds.is_empty() && !matches!(write, WorldWrite::Discard) {
            replacement
                .facts
                .propagate(seeds, || self.next_resolution_fact_version());
        }
        match write {
            WorldWrite::Discard => {}
            WorldWrite::Retain => {
                domain.root.store(Arc::new(replacement));
            }
            WorldWrite::Publish => {
                replacement.id = ResolutionWorldId::from_raw(
                    self.next_resolution_world_id
                        .fetch_add(1, Ordering::Relaxed),
                );
                domain.root.store(Arc::new(replacement));
                restore.value = stable.wrapping_add(2);
            }
        }
        restore.armed = false;
        domain.epoch.store(restore.value, Ordering::Release);
        result
    }

    /// Publish one query's DECISION node: a fresh, non-initial version
    /// plus the atomic replacement of its COMPLETE direct edge set.
    ///
    /// Runs inside the resolve fence, so the caller already holds the
    /// base publication gate and — for a session population — that
    /// session's write gate.
    ///
    /// The write is a RETAIN, not a Publish, and that is a correctness
    /// choice rather than an optimisation. A decision node's edges are
    /// bookkeeping no captured witness can observe, and its fresh version
    /// is a value only a LATER capture can read: the attempt that mints
    /// it never records it (a cold attempt's witness is its own direct
    /// observations), so nothing in flight is superseded. Minting a new
    /// world identity per admitted resolution would instead invalidate
    /// every concurrent capture and turn a cold sweep into a retry storm.
    fn publish_resolution_decision(
        &self,
        captured: &CapturedResolutionFence,
        query: ResolutionQueryKey,
        direct_edges: Vec<ResolutionFactKey>,
    ) {
        let node = ResolutionFactKey::decision(query);
        match (node.population(), captured.session_domain.as_ref()) {
            (ResolutionPopulation::Base, _) => {
                self.mutate_resolution_world_locked_with_held_session(
                    Self::held_session_of(captured),
                    |world| {
                        world.facts.publish_derived(node, direct_edges);
                        ((), WorldWrite::Retain)
                    },
                );
            }
            (ResolutionPopulation::Session(_), Some(domain)) => {
                self.mutate_resolution_session_write_held(
                    domain,
                    captured.world.base.as_ref(),
                    |_base, session| {
                        session.facts.publish_derived(node, direct_edges);
                        ((), WorldWrite::Retain)
                    },
                );
            }
            // A session-population query with no captured session domain
            // has no root to publish into. Fail closed: no node, so no
            // consumer can ever record one.
            (ResolutionPopulation::Session(_), None) => {}
        }
    }

    /// Drop the decision node of a query whose candidate aged out of its
    /// slot, under the same fence that admitted the replacement.
    ///
    /// Publishes a new world identity: the removal ADVANCES the node's
    /// version, which every parent that recorded it must observe.
    fn remove_resolution_decision(
        &self,
        captured: &CapturedResolutionFence,
        query: ResolutionQueryKey,
    ) {
        let node = ResolutionFactKey::decision(query);
        let remove = |facts: &mut crate::resolution_currency::ResolutionFactRoot| {
            if facts.remove_derived(&node, self.next_resolution_fact_version()) {
                WorldWrite::Publish
            } else {
                WorldWrite::Discard
            }
        };
        match (node.population(), captured.session_domain.as_ref()) {
            (ResolutionPopulation::Base, _) => {
                self.mutate_resolution_world_locked_with_held_session(
                    Self::held_session_of(captured),
                    |world| ((), remove(&mut world.facts)),
                );
            }
            (ResolutionPopulation::Session(_), Some(domain)) => {
                self.mutate_resolution_session_write_held(
                    domain,
                    captured.world.base.as_ref(),
                    |_base, session| ((), remove(&mut session.facts)),
                );
            }
            (ResolutionPopulation::Session(_), None) => {}
        }
    }

    /// Publish one owner's `OwnerResolutionSet` node over that owner's
    /// currently published child decisions.
    ///
    /// The node records CHILD DECISIONS, never their leaves, so an owner
    /// witness is bounded by the owner's own decision count instead of by
    /// the transitive closure everything it imports reaches through.
    ///
    /// IDEMPOTENT on an unchanged child set: the edges are replaced only
    /// when the set actually differs, so re-asking for a warm owner
    /// surface neither churns the graph nor supersedes the very view that
    /// asked. Like a decision publication it mints no version — the node
    /// reads what the caller's own captured world says, and advances only
    /// through propagation from a child decision or through removal.
    ///
    /// Returns the node's fact ref so the caller can observe it as its
    /// single owner-scoped root, or `None` when the owner has no
    /// published decision to stand for.
    pub(crate) fn publish_owner_resolution_set(
        &self,
        owner_canonical: &str,
        population: ResolutionPopulation,
    ) -> Option<crate::FactVersionRef> {
        let node = ResolutionFactKey::owner_resolution_set(
            CanonicalResolutionId::new(verter_semantic::resolver_core::normalize_canonical_id(
                owner_canonical,
            )),
            population,
        );
        let publish = |facts: &mut crate::resolution_currency::ResolutionFactRoot| -> bool {
            let mut children = facts.owner_child_decisions(owner_canonical, population);
            if children.is_empty() {
                return false;
            }
            children.sort();
            let unchanged = facts
                .direct_dependencies(&node)
                .map(|mut existing| {
                    existing.sort();
                    existing == children
                })
                .unwrap_or(false);
            if unchanged {
                return false;
            }
            facts.publish_derived(node.clone(), children);
            true
        };
        match population {
            ResolutionPopulation::Base => {
                self.mutate_resolution_world(|world| {
                    let changed = publish(&mut world.facts);
                    (
                        (),
                        if changed {
                            WorldWrite::Retain
                        } else {
                            WorldWrite::Discard
                        },
                    )
                });
            }
            ResolutionPopulation::Session(fingerprint) => {
                let domain = self.session_resolution_domain(fingerprint);
                let _base_read_fence = self.resolution_world_write.lock();
                let base = self.resolution_world.load_full();
                self.mutate_resolution_session_locked(&domain, base.as_ref(), |_base, session| {
                    let changed = publish(&mut session.facts);
                    (
                        (),
                        if changed {
                            WorldWrite::Retain
                        } else {
                            WorldWrite::Discard
                        },
                    )
                });
            }
        }
        let world = self.capture_published_resolution_world(population)?;
        let owns_children = match population {
            ResolutionPopulation::Base => world.base.facts.direct_dependencies(&node).is_some(),
            ResolutionPopulation::Session(_) => world
                .session
                .as_ref()
                .is_some_and(|session| session.facts.direct_dependencies(&node).is_some()),
        };
        if !owns_children {
            // No child decision has ever been published for this owner,
            // so there is no owner-scoped node to root on. Fail closed
            // rather than hand back a node with no edges — nothing could
            // ever propagate into it.
            return None;
        }
        let version = world.fact_version(&node);
        Some(crate::FactVersionRef::ResolveImports(
            crate::ResolveImportsFactRef::Resolution(
                crate::resolution_currency::ResolutionFactRef::new(node, version),
            ),
        ))
    }

    fn replace_world_exact_resolutions(
        &self,
        world: &mut ResolutionWorldRoot,
        canonical_id: &str,
        resolutions: &[ExactResolution],
    ) -> bool {
        if world.owner_exacts_equal(canonical_id, resolutions) {
            return false;
        }
        let mut affected = world.owner_exact_fact_keys(canonical_id);
        affected.extend(resolutions.iter().map(|resolution| {
            ResolutionFactKey::exact_importer(
                canonical_id,
                &resolution.specifier,
                verter_semantic::resolver_core::ResolutionContext {
                    phase: resolution.phase,
                    kind: resolution.kind,
                },
                ResolutionPopulation::Base,
            )
        }));
        affected.sort();
        affected.dedup();
        for key in affected {
            world
                .facts
                .advance(key, self.next_resolution_fact_version());
        }
        world.replace_owner_exacts(canonical_id, resolutions);
        true
    }

    fn advance_resolution_fact(
        &self,
        facts: &mut crate::resolution_currency::ResolutionFactRoot,
        key: ResolutionFactKey,
    ) {
        facts.advance(key, self.next_resolution_fact_version());
    }

    /// Mutation-side fact keys for one precise path transition: the exact
    /// `PathProbe`, the exact `Realpath`, and the enumerated parent
    /// `DirectoryMembers`. Deliberately NO `RecoveryScope` keys — witnesses
    /// OBSERVE recovery scopes, but only IMPRECISE watcher mutations ADVANCE
    /// them ([`Self::mutate_content_subtree`] / `DirectoryTreeDirty`); a
    /// precise per-path mutation advancing an ancestor scope would destroy
    /// every sibling witness under that ancestor.
    fn path_fact_keys(
        canonical_id: &str,
        population: ResolutionPopulation,
    ) -> Vec<ResolutionFactKey> {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        let mut keys = vec![
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new(canonical.clone()),
                population,
            },
            ResolutionFactKey::Realpath {
                requested: CanonicalResolutionId::new(canonical.clone()),
                population,
            },
        ];
        if let Some(index) = canonical.rfind('/') {
            let parent = if index == 0 { "/" } else { &canonical[..index] };
            keys.push(ResolutionFactKey::DirectoryMembers {
                canonical: CanonicalResolutionId::new(parent),
                population,
            });
        }
        keys
    }

    fn update_base_path_facts(
        &self,
        world: &mut ResolutionWorldRoot,
        canonical_id: &str,
        outcome: verter_semantic::resolver_core::PathProbe,
    ) -> bool {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        if world.path_probes.get(&canonical).copied() == Some(outcome) {
            return false;
        }
        world.path_probes.insert(canonical.clone(), outcome);
        for key in Self::path_fact_keys(&canonical, ResolutionPopulation::Base) {
            self.advance_resolution_fact(&mut world.facts, key);
        }
        if matches!(
            outcome,
            verter_semantic::resolver_core::PathProbe::File
                | verter_semantic::resolver_core::PathProbe::Directory
        ) {
            self.advance_absent_realpath_ancestors(world, &canonical);
        }
        true
    }

    /// A path APPEARING beneath an ancestor the world recorded as having
    /// no realpath changes that ancestor's value too: the directory it
    /// names now exists.
    ///
    /// Only ancestors recorded as KNOWN-ABSENT (`Some(None)`) are
    /// touched. An unrecorded ancestor contradicts nothing, and one
    /// recorded with a realpath already existed. The recorded value is
    /// dropped alongside the advance so the next live observation refills
    /// it as a first observation rather than as a conflict.
    ///
    /// `O(path depth × point lookup)` in the persistent maps already
    /// present — no ancestor index is added.
    fn advance_absent_realpath_ancestors(&self, world: &mut ResolutionWorldRoot, canonical: &str) {
        let absent: Vec<String> = crate::resolution_currency::ancestor_scopes(canonical)
            .into_iter()
            .filter(|prefix| matches!(world.realpaths.get(prefix), Some(None)))
            .collect();
        for prefix in absent {
            world.realpaths.remove(&prefix);
            self.advance_resolution_fact(
                &mut world.facts,
                ResolutionFactKey::Realpath {
                    requested: CanonicalResolutionId::new(prefix),
                    population: ResolutionPopulation::Base,
                },
            );
        }
    }

    /// Precise per-path transition whose post-mutation observed values are
    /// unknown (e.g. a watcher `FileChanged` without content): drop the
    /// recorded evidence baselines and advance the exact path facts so
    /// stale witnesses fail. Never touches recovery scopes.
    fn advance_base_path_facts_unknown(&self, world: &mut ResolutionWorldRoot, canonical_id: &str) {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        world.path_probes.remove(&canonical);
        world.realpaths.remove(&canonical);
        for key in Self::path_fact_keys(&canonical, ResolutionPopulation::Base) {
            self.advance_resolution_fact(&mut world.facts, key);
        }
    }

    /// Value-sensitive base realpath MUTATION, mirroring
    /// [`Self::update_base_path_facts`]: records the new value in the world
    /// baseline and advances the exact `Realpath` fact only when the value
    /// actually changed.
    ///
    /// An unrecorded baseline counts as a change HERE, and must: a mutation
    /// says "this path's value is now X", and a witness that observed the
    /// path while no baseline was recorded holds the fact at
    /// [`ResolutionFactVersion::INITIAL`] — not advancing would leave that
    /// witness validating against a value the mutation just replaced.
    ///
    /// That is the opposite of the rule an OBSERVATION follows
    /// ([`Self::fold_observed_baseline`]), and deliberately so: an
    /// observation says "I looked, and it is X", which contradicts nothing
    /// when nothing was recorded. Mutations advance on a first write;
    /// observations never advance on a first read. Two domains, two rules,
    /// each stated where it applies — this function is the mutation half and
    /// is never reachable from the observation path.
    fn update_base_realpath_fact(
        &self,
        world: &mut ResolutionWorldRoot,
        canonical_id: &str,
        resolved: Option<String>,
    ) -> bool {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        let resolved =
            resolved.map(|path| verter_semantic::resolver_core::normalize_canonical_id(&path));
        if world.realpaths.get(&canonical) == Some(&resolved) {
            return false;
        }
        world.realpaths.insert(canonical.clone(), resolved);
        self.advance_resolution_fact(
            &mut world.facts,
            ResolutionFactKey::Realpath {
                requested: CanonicalResolutionId::new(canonical),
                population: ResolutionPopulation::Base,
            },
        );
        true
    }

    /// Value-sensitive base manifest MUTATION — the manifest sibling of
    /// [`Self::update_base_realpath_fact`], and on the same mutation-domain
    /// rule: an unrecorded baseline counts as a change.
    fn update_base_manifest_fact(
        &self,
        world: &mut ResolutionWorldRoot,
        canonical_id: &str,
        fingerprint: Option<[u8; 16]>,
    ) -> bool {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        if !crate::resolution_currency::is_package_manifest_path(&canonical) {
            return false;
        }
        if world.manifest_fingerprints.get(&canonical) == Some(&fingerprint) {
            return false;
        }
        world
            .manifest_fingerprints
            .insert(canonical.clone(), fingerprint);
        self.advance_resolution_fact(
            &mut world.facts,
            ResolutionFactKey::Manifest {
                canonical: CanonicalResolutionId::new(canonical),
                population: ResolutionPopulation::Base,
            },
        );
        true
    }

    /// **The ONE first-observation rule**, for every reobservable family.
    ///
    /// An observation that fills an unrecorded baseline records the value and
    /// advances NOTHING: it contradicts no witness, because there was no
    /// recorded value for a witness's meaning to have depended on. An
    /// observation that disagrees with a recorded baseline reveals state
    /// newer than the recorded world and enters the mutation protocol.
    ///
    /// Both evidence consumers — the admission fold
    /// ([`Self::fold_observed_base_evidence`]) and the reuse-time refresh
    /// ([`Self::refresh_resolution_evidence`]) — fold through here, so the
    /// rule has one implementation. It used to have two, and they disagreed:
    /// the probe/realpath limbs guarded on `contains_key` while the manifest
    /// limb went straight to the mutation helper, whose `None != Some(fp)`
    /// comparison advances on a first observation.
    /// CLASSIFY one observation against the recorded baseline, writing
    /// nothing.
    ///
    /// The one classification rule, so the read-only pre-check that decides
    /// whether the world gate is worth entering and the write that follows it
    /// can never disagree about what "unchanged" means.
    fn baseline_fold_verdict(
        world: &ResolutionWorldRoot,
        observation: &ObservedBaselineValue<'_>,
    ) -> BaselineFold {
        let canonical = observation.canonical;
        match observation.value {
            ObservedFamilyValue::Probe(probe) => match world.path_probes.get(canonical) {
                Some(existing) if *existing == probe => BaselineFold::Unchanged,
                Some(_) => BaselineFold::Conflicted,
                None => BaselineFold::Filled,
            },
            ObservedFamilyValue::Realpath(resolved) => match world.realpaths.get(canonical) {
                Some(existing) if existing.as_deref() == resolved => BaselineFold::Unchanged,
                Some(_) => BaselineFold::Conflicted,
                None => BaselineFold::Filled,
            },
            ObservedFamilyValue::Manifest(fingerprint) => {
                if !crate::resolution_currency::is_package_manifest_path(canonical) {
                    return BaselineFold::Unchanged;
                }
                match world.manifest_fingerprints.get(canonical) {
                    Some(existing) if *existing == fingerprint => BaselineFold::Unchanged,
                    Some(_) => BaselineFold::Conflicted,
                    None => BaselineFold::Filled,
                }
            }
        }
    }

    fn fold_observed_baseline(
        &self,
        world: &mut ResolutionWorldRoot,
        observation: &ObservedBaselineValue<'_>,
    ) -> BaselineFold {
        let canonical = observation.canonical;
        let verdict = Self::baseline_fold_verdict(world, observation);
        match (verdict, observation.value) {
            (BaselineFold::Unchanged, _) => {}
            (BaselineFold::Conflicted, ObservedFamilyValue::Probe(probe)) => {
                self.update_base_path_facts(world, canonical, probe);
            }
            (BaselineFold::Filled, ObservedFamilyValue::Probe(probe)) => {
                world.path_probes.insert(canonical.to_owned(), probe);
            }
            (BaselineFold::Conflicted, ObservedFamilyValue::Realpath(resolved)) => {
                self.update_base_realpath_fact(world, canonical, resolved.map(ToOwned::to_owned));
            }
            (BaselineFold::Filled, ObservedFamilyValue::Realpath(resolved)) => {
                world
                    .realpaths
                    .insert(canonical.to_owned(), resolved.map(ToOwned::to_owned));
            }
            (BaselineFold::Conflicted, ObservedFamilyValue::Manifest(fingerprint)) => {
                self.update_base_manifest_fact(world, canonical, fingerprint);
            }
            (BaselineFold::Filled, ObservedFamilyValue::Manifest(fingerprint)) => {
                world
                    .manifest_fingerprints
                    .insert(canonical.to_owned(), fingerprint);
            }
        }
        verdict
    }

    fn base_manifest_fingerprint(&self, canonical_id: &str) -> Option<[u8; 16]> {
        let source = self.snapshot.read().read(canonical_id)?;
        Some(manifest_resolution_fingerprint(&source))
    }

    fn overlay_manifest_fingerprint(&self, canonical_id: &str) -> Option<[u8; 16]> {
        let source = self.overlay.read().get(canonical_id)?;
        Some(manifest_resolution_fingerprint(&source))
    }

    fn update_session_overlay_facts(
        &self,
        base: &ResolutionWorldRoot,
        session: &mut ResolutionSessionRoot,
        fingerprint: SessionFingerprint,
        canonical_id: &str,
        manifest_fingerprint: Option<[u8; 16]>,
    ) {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        let population = ResolutionPopulation::Session(fingerprint);
        let was_overlay = session.overlay_paths.contains(&canonical);
        let effective_before = if was_overlay {
            Some(verter_semantic::resolver_core::PathProbe::File)
        } else {
            base.path_probes.get(&canonical).copied()
        };
        let path_changed =
            effective_before != Some(verter_semantic::resolver_core::PathProbe::File);
        // An overlay's effective realpath is its canonical overlay path.
        // Compare against the recorded base realpath value: only a base
        // value known to already equal the canonical leaves the effective
        // realpath meaning unchanged; a differing or unrecorded base value
        // means opening the overlay changed (or may have changed) it.
        let realpath_meaning_changed = !was_overlay
            && !matches!(
                base.realpaths.get(&canonical),
                Some(Some(resolved)) if resolved == &canonical
            );
        for key in Self::path_fact_keys(&canonical, population) {
            let realpath_changed =
                realpath_meaning_changed && matches!(&key, ResolutionFactKey::Realpath { .. });
            if path_changed || realpath_changed {
                self.advance_resolution_fact(&mut session.facts, key);
            } else if session.facts.version(&key) == ResolutionFactVersion::INITIAL {
                let base_version =
                    base.fact_version(&key.in_population(ResolutionPopulation::Base));
                if base_version != ResolutionFactVersion::INITIAL {
                    session.facts.mirror_base_version(key, base_version);
                }
            }
        }
        session.overlay_paths.insert(canonical.clone());

        if canonical.ends_with("/package.json") {
            let previous = if was_overlay {
                session.manifest_fingerprints.get(&canonical).copied()
            } else {
                // An unrecorded base baseline and one recorded as absent are
                // the same thing for an OVERLAY comparison: either way the
                // overlay's value is what the session now sees.
                base.manifest_fingerprints
                    .get(&canonical)
                    .copied()
                    .flatten()
            };
            if previous != manifest_fingerprint {
                self.advance_resolution_fact(
                    &mut session.facts,
                    ResolutionFactKey::Manifest {
                        canonical: CanonicalResolutionId::new(canonical.clone()),
                        population,
                    },
                );
            } else {
                let key = ResolutionFactKey::Manifest {
                    canonical: CanonicalResolutionId::new(canonical.clone()),
                    population,
                };
                if session.facts.version(&key) == ResolutionFactVersion::INITIAL {
                    let base_version =
                        base.fact_version(&key.in_population(ResolutionPopulation::Base));
                    if base_version != ResolutionFactVersion::INITIAL {
                        session.facts.mirror_base_version(key, base_version);
                    }
                }
            }
            match manifest_fingerprint {
                Some(fingerprint) => {
                    session.manifest_fingerprints.insert(canonical, fingerprint);
                }
                None => {
                    session.manifest_fingerprints.remove(&canonical);
                }
            }
        }
    }

    fn reveal_session_overlay_facts(
        session: &mut ResolutionSessionRoot,
        fingerprint: SessionFingerprint,
        canonical_id: &str,
    ) -> bool {
        let canonical = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        if session.overlay_paths.remove(&canonical).is_none() {
            return false;
        }
        let population = ResolutionPopulation::Session(fingerprint);
        for key in Self::path_fact_keys(&canonical, population) {
            session.facts.remove(&key);
        }
        session.facts.remove(&ResolutionFactKey::Manifest {
            canonical: CanonicalResolutionId::new(canonical.clone()),
            population,
        });
        session.manifest_fingerprints.remove(&canonical);
        true
    }

    fn remove_file_edges_in_world(
        &self,
        world: &mut ResolutionWorldRoot,
        canonical_id: &str,
    ) -> bool {
        self.edges.write().remove_file(canonical_id);
        self.replace_world_exact_resolutions(world, canonical_id, &[])
    }

    fn remove_edges_under_in_world(&self, world: &mut ResolutionWorldRoot, prefix: &str) -> bool {
        let exact_owners = world.exact_owners_under(prefix);
        self.edges.write().remove_under(prefix);
        let mut changed = false;
        for owner in exact_owners {
            changed |= self.replace_world_exact_resolutions(world, &owner, &[]);
        }
        changed
    }

    /// Execute one canonical-scoped content mutation while the resolution
    /// epoch is odd. The closure returns whether effective state changed;
    /// no-ops retain the prior world identity and generation.
    pub(crate) fn mutate_content_for<R>(
        &self,
        canonical_id: &str,
        remove_edges: bool,
        path_after: Option<verter_semantic::resolver_core::PathProbe>,
        realpath_after: BaseRealpathTransition,
        mutation: impl FnOnce() -> (R, bool),
    ) -> R {
        let _strict_transition = self.strict_self_root_transition();
        self.mutate_resolution_world(|world| {
            let (result, changed) = mutation();
            if !changed {
                return (result, false);
            }
            if remove_edges {
                self.remove_file_edges_in_world(world, canonical_id);
            }
            if let Some(path_after) = path_after {
                self.update_base_path_facts(world, canonical_id, path_after);
            }
            match realpath_after {
                BaseRealpathTransition::Unknown => {
                    world.realpaths.remove(
                        &verter_semantic::resolver_core::normalize_canonical_id(canonical_id),
                    );
                }
                BaseRealpathTransition::Known(resolved) => {
                    self.update_base_realpath_fact(world, canonical_id, resolved);
                }
            }
            let manifest_fingerprint = self.base_manifest_fingerprint(canonical_id);
            self.update_base_manifest_fact(world, canonical_id, manifest_fingerprint);
            let generation = self.bump_content_generation_in_world();
            self.record_content_transition_at(canonical_id, generation);
            (result, true)
        })
    }

    pub(crate) fn mutate_overlay_upsert<R>(
        &self,
        canonical_id: &str,
        mutation: impl FnOnce() -> (R, bool),
    ) -> R {
        crate::probe_scope!(MUTATE_OVERLAY_UPSERT);
        let _strict_transition = self.strict_self_root_transition();
        let fingerprint = self.default_resolution_session;
        self.mutate_resolution_session(fingerprint, |base, session| {
            let (result, changed) = mutation();
            if !changed {
                return (result, false);
            }
            let manifest_fingerprint = self.overlay_manifest_fingerprint(canonical_id);
            self.update_session_overlay_facts(
                base,
                session,
                fingerprint,
                canonical_id,
                manifest_fingerprint,
            );
            let generation = self.bump_content_generation_in_world();
            self.record_content_transition_at(canonical_id, generation);
            (result, true)
        })
    }

    pub(crate) fn mutate_overlay_close<R>(
        &self,
        canonical_id: &str,
        mutation: impl FnOnce() -> (R, bool),
    ) -> R {
        let _strict_transition = self.strict_self_root_transition();
        let fingerprint = self.default_resolution_session;
        self.mutate_resolution_session(fingerprint, |_base, session| {
            let (result, changed) = mutation();
            if !changed {
                return (result, false);
            }
            let revealed = Self::reveal_session_overlay_facts(session, fingerprint, canonical_id);
            let generation = self.bump_content_generation_in_world();
            self.record_content_transition_at(canonical_id, generation);
            (result, revealed || changed)
        })
    }

    /// Execute an enumerable in-memory subtree deletion under one publication
    /// fence, synchronising dependency/exact state and per-member transition
    /// generations.
    pub(crate) fn mutate_content_files_under<R>(
        &self,
        prefix: &str,
        mutation: impl FnOnce() -> (R, Vec<String>, bool),
    ) -> R {
        let _strict_transition = self.strict_self_root_transition();
        self.mutate_resolution_world(|world| {
            let (result, transitioned, changed) = mutation();
            if !changed {
                return (result, false);
            }
            self.remove_edges_under_in_world(world, prefix);
            let generation = self.bump_content_generation_in_world();
            for canonical_id in transitioned {
                self.update_base_path_facts(
                    world,
                    &canonical_id,
                    verter_semantic::resolver_core::PathProbe::Absent,
                );
                self.update_base_realpath_fact(world, &canonical_id, None);
                self.update_base_manifest_fact(world, &canonical_id, None);
                self.record_content_transition_at(&canonical_id, generation);
            }
            (result, true)
        })
    }

    /// Execute an unknown-member subtree mutation (disk recursion or watcher
    /// recovery) under one publication fence.
    ///
    /// Native-only: subtree removal is driven by the filesystem workspace's
    /// `delete_dir_all` / watcher recovery, neither of which exists on
    /// `wasm32`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn mutate_content_subtree<R>(
        &self,
        prefix: &str,
        remove_edges: bool,
        mutation: impl FnOnce() -> (R, bool),
    ) -> R {
        let _strict_transition = self.strict_self_root_transition();
        self.mutate_resolution_world(|world| {
            let (result, changed) = mutation();
            if !changed {
                return (result, false);
            }
            if remove_edges {
                self.remove_edges_under_in_world(world, prefix);
            }
            let normalized = verter_semantic::resolver_core::normalize_canonical_id(prefix);
            self.advance_resolution_fact(
                &mut world.facts,
                ResolutionFactKey::RecoveryScope {
                    canonical_prefix: CanonicalResolutionId::new(normalized.clone()),
                    population: ResolutionPopulation::Base,
                },
            );
            if let Some(index) = normalized.rfind('/') {
                let parent = if index == 0 {
                    "/"
                } else {
                    &normalized[..index]
                };
                self.advance_resolution_fact(
                    &mut world.facts,
                    ResolutionFactKey::DirectoryMembers {
                        canonical: CanonicalResolutionId::new(parent),
                        population: ResolutionPopulation::Base,
                    },
                );
            }
            let generation = self.bump_content_generation_in_world();
            self.record_subtree_content_transition_at(prefix, generation);
            (result, true)
        })
    }

    fn capture_resolution_world(
        &self,
        population: ResolutionPopulation,
    ) -> Option<CapturedResolutionFence> {
        for _ in 0..16 {
            let base_before = crate::resolution_currency::ResolutionEpoch::from_raw(
                self.resolution_epoch.load(Ordering::Acquire),
            );
            if !base_before.is_stable() {
                std::hint::spin_loop();
                continue;
            }
            let base = self.resolution_world.load_full();
            let (session_domain, session_epoch, session) = match population {
                ResolutionPopulation::Base => (None, None, None),
                ResolutionPopulation::Session(fingerprint) => {
                    let domain = self.session_resolution_domain(fingerprint);
                    let before = ResolutionEpoch::from_raw(domain.epoch.load(Ordering::Acquire));
                    if !before.is_stable() {
                        std::hint::spin_loop();
                        continue;
                    }
                    let root = domain.root.load_full();
                    let after = ResolutionEpoch::from_raw(domain.epoch.load(Ordering::Acquire));
                    if before != after || !after.is_stable() {
                        continue;
                    }
                    (Some(domain), Some(after), Some(root))
                }
            };
            let base_after = crate::resolution_currency::ResolutionEpoch::from_raw(
                self.resolution_epoch.load(Ordering::Acquire),
            );
            if base_before == base_after && base_after.is_stable() {
                return Some(CapturedResolutionFence {
                    base_epoch: base_after,
                    session_epoch,
                    session_domain,
                    world: Arc::new(CapturedResolutionWorld {
                        base,
                        session,
                        population,
                    }),
                });
            }
        }
        None
    }

    /// Capture the immutable published resolution world for `population` as
    /// a standalone validity root, without the transaction fence.
    ///
    /// O(1): two `ArcSwap` loads plus the even-epoch check — no owner,
    /// artifact, or fact enumeration, independent of workspace size. The
    /// result is the composition `.DECISION.md` §4 requires a consumer view
    /// to capture: the current base root plus, for a session population,
    /// that session's overlay root.
    ///
    /// `None` under sustained publication churn (the bounded stable-epoch
    /// retry never observed a settled world). A consumer that captured
    /// nothing validates no resolution fact — fail-closed, never a live
    /// registry read at validation time.
    pub(crate) fn capture_published_resolution_world(
        &self,
        population: ResolutionPopulation,
    ) -> Option<Arc<CapturedResolutionWorld>> {
        self.capture_resolution_world(population)
            .map(|captured| captured.world)
    }

    /// Capture the current STABLE world, yielding briefly while some writer is
    /// inside the odd-epoch publication window.
    ///
    /// A failed capture is NOT a world attempt. It means "somebody is
    /// publishing right now" — transient contention with no mixed-world
    /// hazard — whereas a world attempt is spent on "the world I captured was
    /// superseded", which is a real coherence event. Charging contention to
    /// the same eight-attempt budget conflates them and starves the resolution
    /// under concurrency: a workspace with several resolutions in flight burns
    /// all eight on odd-epoch windows and returns `ResolutionRetryExhausted`
    /// for a request nothing was wrong with. That refusal is not cosmetic —
    /// the LSP's carrier-import closure treats a refused resolution as "not
    /// live", so the workspace-symbol frontier never completes and rename
    /// silently returns no edits.
    ///
    /// Bounded, and bounded by YIELDS rather than by time: a publisher that
    /// never finishes still fails closed here instead of hanging, and a
    /// publisher that is merely slow is waited out without spending a
    /// coherence retry.
    fn capture_stable_resolution_world(
        &self,
        population: ResolutionPopulation,
    ) -> Option<CapturedResolutionFence> {
        const CAPTURE_YIELDS: usize = 1024;
        for _ in 0..CAPTURE_YIELDS {
            if let Some(captured) = self.capture_resolution_world(population) {
                return Some(captured);
            }
            std::thread::yield_now();
        }
        None
    }

    fn resolution_world_still_current(&self, captured: &CapturedResolutionFence) -> bool {
        let live_epoch = crate::resolution_currency::ResolutionEpoch::from_raw(
            self.resolution_epoch.load(Ordering::Acquire),
        );
        let base_current = live_epoch == captured.base_epoch
            && live_epoch.is_stable()
            && self.resolution_world.load().id == captured.world.base.id;
        if !base_current {
            return false;
        }
        match (
            captured.session_domain.as_ref(),
            captured.session_epoch,
            captured.world.session.as_ref(),
        ) {
            (None, None, None) => true,
            (Some(domain), Some(epoch), Some(root)) => {
                let live = ResolutionEpoch::from_raw(domain.epoch.load(Ordering::Acquire));
                live == epoch && live.is_stable() && domain.root.load().id == root.id
            }
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn resolution_epoch_for_test(
        &self,
        population: ResolutionPopulation,
    ) -> ResolutionEpoch {
        match population {
            ResolutionPopulation::Base => {
                ResolutionEpoch::from_raw(self.resolution_epoch.load(Ordering::Acquire))
            }
            ResolutionPopulation::Session(fingerprint) => {
                let domain = self.session_resolution_domain(fingerprint);
                ResolutionEpoch::from_raw(domain.epoch.load(Ordering::Acquire))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn resolution_fact_version_for_test(
        &self,
        population: ResolutionPopulation,
        key: &ResolutionFactKey,
    ) -> ResolutionFactVersion {
        self.capture_resolution_world(population)
            .map(|captured| captured.world.fact_version(key))
            .unwrap_or(ResolutionFactVersion::INITIAL)
    }

    /// One derived node's COMPLETE direct dependency set, read through a
    /// freshly captured world.
    #[cfg(test)]
    pub(crate) fn decision_direct_dependencies_for_test(
        &self,
        population: ResolutionPopulation,
        node: &ResolutionFactKey,
    ) -> Option<Vec<ResolutionFactKey>> {
        let captured = self.capture_resolution_world(population)?;
        match population {
            ResolutionPopulation::Base => captured.world.base.facts.direct_dependencies(node),
            ResolutionPopulation::Session(_) => captured
                .world
                .session
                .as_ref()
                .and_then(|session| session.facts.direct_dependencies(node)),
        }
    }

    /// Drop a derived node from the owning root — the removal half of the
    /// removal/reintroduction contract.
    #[cfg(test)]
    pub(crate) fn remove_derived_node_for_test(
        &self,
        population: ResolutionPopulation,
        node: &ResolutionFactKey,
    ) -> bool {
        match population {
            ResolutionPopulation::Base => self.mutate_resolution_world(|world| {
                let removed = world
                    .facts
                    .remove_derived(node, self.next_resolution_fact_version());
                (
                    removed,
                    if removed {
                        WorldWrite::Publish
                    } else {
                        WorldWrite::Discard
                    },
                )
            }),
            ResolutionPopulation::Session(fingerprint) => {
                let domain = self.session_resolution_domain(fingerprint);
                let _base_read_fence = self.resolution_world_write.lock();
                let base = self.resolution_world.load_full();
                self.mutate_resolution_session_locked(&domain, base.as_ref(), |_base, session| {
                    let removed = session
                        .facts
                        .remove_derived(node, self.next_resolution_fact_version());
                    (
                        removed,
                        if removed {
                            WorldWrite::Publish
                        } else {
                            WorldWrite::Discard
                        },
                    )
                })
            }
        }
    }

    /// The resolution domain's aggregate stamp, read through a freshly
    /// captured world — the same value a compacted signature is minted
    /// from and validated against.
    #[cfg(test)]
    pub(crate) fn captured_resolution_stamp_for_test(
        &self,
        population: ResolutionPopulation,
    ) -> Option<crate::AggregateStamp> {
        self.capture_resolution_world(population)
            .and_then(|captured| captured.world.resolution_stamp(population))
    }

    #[cfg(test)]
    pub(crate) fn cached_resolution_query_for_test(
        &self,
        importer_id: &str,
        specifier: &str,
        context: verter_semantic::resolver_core::ResolutionContext,
        population: ResolutionPopulation,
    ) -> Option<ResolutionQueryKey> {
        self.lazy_resolution_cache
            .read()
            .get(&LazyResolutionCacheKey {
                importer_id: importer_id.to_owned(),
                specifier: specifier.to_owned(),
                phase: context.phase,
                kind: context.kind,
                population,
            })
            .and_then(|slot| slot.last())
            .map(|entry| entry.query.clone())
    }

    /// Number of candidates currently retained in one resolution slot.
    /// Bounded by [`crate::CANDIDATE_CAP`].
    #[cfg(test)]
    pub(crate) fn lazy_resolution_slot_len_for_test(
        &self,
        importer_id: &str,
        specifier: &str,
        context: verter_semantic::resolver_core::ResolutionContext,
        population: ResolutionPopulation,
    ) -> usize {
        self.lazy_resolution_cache
            .read()
            .get(&LazyResolutionCacheKey {
                importer_id: importer_id.to_owned(),
                specifier: specifier.to_owned(),
                phase: context.phase,
                kind: context.kind,
                population,
            })
            .map_or(0, |slot| slot.len())
    }

    /// Load the current published state (lock-free).
    ///
    /// Always returns `Some` after `Engine::new()`. Check
    /// `ownership_ready` to distinguish bootstrap from real snapshots.
    pub(crate) fn load_published(&self) -> Option<Arc<PublishedRoot>> {
        self.published_state.load_full()
    }

    pub(crate) fn resource_snapshot(&self) -> WorkspaceResourceSnapshot {
        let overlay = self.overlay.read();
        let snapshot = self.snapshot.read();
        let edges = self.edges.read();
        let package_index = self.package_index.read();
        let published = self.load_published();

        WorkspaceResourceSnapshot {
            overlay_entries: overlay.len(),
            overlay_bytes: overlay.approx_bytes(),
            snapshot_entries: snapshot.len(),
            snapshot_bytes: snapshot.approx_bytes(),
            edge_file_count: edges.file_count(),
            reverse_dep_bucket_count: edges.reverse_dep_bucket_count(),
            package_manifest_count: package_index.found_count(),
            published_project_count: published
                .as_ref()
                .map(|root| root.snapshot.projects.len())
                .unwrap_or(0),
        }
    }

    /// Build and publish a snapshot from the current project graph.
    ///
    /// Derives a `WorkspaceSnapshot` + `ModuleResolverCore` from the current
    /// `project_graph` and atomically publishes them to `published_state`.
    /// Called by `set_project_graph()` and `configure_resolver()`.
    ///
    /// **Env-hash composition (project-scoped env-hash API).** Computes per-project
    /// `[parse, resolve, type_, lib]` env-hash arrays and project-identity
    /// hashes ONCE here, before publication, so the published snapshot
    /// carries its env-hash tables atomically. Producer reads from the
    /// project graph's `compiler_options` and the engine-level resolve
    /// extensions; consumers look up tables on the published snapshot.
    pub(crate) fn rebuild_and_publish(&self) {
        let _strict_transition = self.strict_self_root_transition();
        // The second env-table republication path (the first is
        // `publish_snapshot`): this recomposes `env_hashes_by_project` /
        // `project_identity_hashes` from the rebuilt project set, with no
        // content bump. Over-bumping a monotonic counter is conservative;
        // MISSING a bump here would leave a source-env-compacted signature
        // valid across a project reconfiguration.
        self.bump_source_env_generation();
        self.mutate_resolution_world(|world| {
            let configured_projects = self.configured_resolver_projects.read().clone();
            let graph = self.project_graph.read();
            let resolver = configured_projects
                .clone()
                .map(verter_semantic::resolver_core::ModuleResolverCore::new)
                .unwrap_or_else(|| graph.to_module_resolver_core());

            // Build a WorkspaceSnapshot from the graph's projects
            let projects: Vec<_> = graph
                .iter()
                .enumerate()
                .map(|(i, config)| {
                    crate::snapshot_builder::ownership_project_from_vfs_config(
                        config,
                        crate::workspace_snapshot::ProjectId(i as u32),
                    )
                })
                .collect();

            let generation = SnapshotGeneration(graph.generation());

            drop(graph);

            let env_inputs_resolve_extensions = self.default_resolve_extensions.load_full();

            let (env_hashes_by_project, project_identity_hashes) =
                if let Some(configured_projects) = configured_projects.as_deref() {
                    compose_env_hash_tables_from_configs(
                        &projects,
                        configured_projects,
                        &env_inputs_resolve_extensions,
                    )
                } else {
                    compose_env_hash_tables(&projects, &env_inputs_resolve_extensions)
                };

            let snapshot = WorkspaceSnapshot {
                owners_memo: Default::default(),
                projects,
                resolver,
                generation,
            };

            let published = Arc::new(PublishedRoot::with_env_hash_tables(
                Arc::new(snapshot),
                env_hashes_by_project,
                project_identity_hashes,
            ));
            self.published_state.store(Some(Arc::clone(&published)));
            #[cfg(any(test, feature = "test-support"))]
            self.notify_published(published.snapshot.generation.0);
            world.replace_published(published, &self.registered_session_context_keys(), || {
                self.next_resolution_fact_version()
            });
            ((), true)
        });
    }

    pub(crate) fn read_package_manifest(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
    ) -> Option<crate::types::PackageManifest> {
        use crate::package_index::ManifestEntry;

        let canonical_id = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        {
            let cache = self.package_index.read();
            match cache.get_cached(&canonical_id) {
                Some(ManifestEntry::Found(manifest)) => return Some((**manifest).clone()),
                Some(ManifestEntry::NotFound) => return None, // negative cache hit
                None => {}                                    // cache miss — proceed to read
            }
        }

        match reader.read_file(&canonical_id) {
            Some(source) => {
                let mut cache = self.package_index.write();
                Some(cache.get_or_parse(&canonical_id, &source).clone())
            }
            None => {
                // Cache the negative result so repeated probes are free.
                let mut cache = self.package_index.write();
                cache.insert_not_found(&canonical_id);
                None
            }
        }
    }

    pub(crate) fn invalidate_package_manifest(&self, canonical_id: &str) {
        let canonical_id = verter_semantic::resolver_core::normalize_canonical_id(canonical_id);
        if canonical_id.ends_with("/package.json") {
            self.package_index.write().invalidate(&canonical_id);
        }
    }

    fn mark_parent_dir_dirty(&self, canonical_id: &str) {
        if let Some((parent, _)) = canonical_id.rsplit_once('/') {
            self.dir_index.write().mark_dirty(parent);
        }
    }

    /// Apply a batch of workspace changes.
    pub(crate) fn apply_changes(&self, changes: Vec<WorkspaceChange>) -> ChangeResult {
        self.apply_changes_with_preflight(changes, |_| {})
    }

    /// Filesystem counterpart of [`Self::apply_changes`] which lets the
    /// owning backend invalidate realpath state inside the same odd/even
    /// publication window as overlay/snapshot changes.
    pub(crate) fn apply_changes_with_preflight(
        &self,
        changes: Vec<WorkspaceChange>,
        preflight: impl FnOnce(&[WorkspaceChange]),
    ) -> ChangeResult {
        let _strict_transition = self.strict_self_root_transition();
        self.mutate_resolution_world(|world| {
            preflight(&changes);
            let mut result = ChangeResult::default();
            let mut content_changed = false;
            let mut base_changed = false;
            let mut subtree_transitions = Vec::new();
            let session_fingerprint = self.default_resolution_session;
            let session_domain = self.session_resolution_domain(session_fingerprint);

            for change in changes {
                match change {
                    WorkspaceChange::OverlaySet {
                        canonical_id,
                        source,
                    } => {
                        let changed = self.mutate_resolution_session_locked(
                            &session_domain,
                            world,
                            |base, session| {
                                self.invalidate_package_manifest(&canonical_id);
                                let changed =
                                    self.overlay.write().set(canonical_id.clone(), source);
                                if changed {
                                    let manifest_fingerprint =
                                        self.overlay_manifest_fingerprint(&canonical_id);
                                    self.update_session_overlay_facts(
                                        base,
                                        session,
                                        session_fingerprint,
                                        &canonical_id,
                                        manifest_fingerprint,
                                    );
                                }
                                (changed, changed)
                            },
                        );
                        if changed {
                            result.invalidated_files.push(canonical_id);
                            content_changed = true;
                        }
                    }
                    WorkspaceChange::OverlayClear { canonical_id } => {
                        let changed = self.mutate_resolution_session_locked(
                            &session_domain,
                            world,
                            |_base, session| {
                                self.invalidate_package_manifest(&canonical_id);
                                let changed = self.overlay.write().clear(&canonical_id);
                                if changed {
                                    Self::reveal_session_overlay_facts(
                                        session,
                                        session_fingerprint,
                                        &canonical_id,
                                    );
                                }
                                (changed, changed)
                            },
                        );
                        if changed {
                            result.invalidated_files.push(canonical_id);
                            content_changed = true;
                        }
                    }
                    WorkspaceChange::FileChanged {
                        canonical_id,
                        source,
                    } => {
                        self.invalidate_package_manifest(&canonical_id);
                        self.mark_parent_dir_dirty(&canonical_id);
                        if !self.overlay.read().has_overlay(&canonical_id) {
                            if let Some(content) = source {
                                self.snapshot.write().inject(canonical_id.clone(), content);
                                self.update_base_path_facts(
                                    world,
                                    &canonical_id,
                                    verter_semantic::resolver_core::PathProbe::File,
                                );
                                // A disk-backed change cannot cheaply prove
                                // the post-change realpath; comparisons stay
                                // conservative until re-observed.
                                world.realpaths.remove(
                                    &verter_semantic::resolver_core::normalize_canonical_id(
                                        &canonical_id,
                                    ),
                                );
                                let manifest_fingerprint =
                                    self.base_manifest_fingerprint(&canonical_id);
                                self.update_base_manifest_fact(
                                    world,
                                    &canonical_id,
                                    manifest_fingerprint,
                                );
                            } else {
                                self.snapshot.write().remove(&canonical_id);
                                self.advance_base_path_facts_unknown(world, &canonical_id);
                                if canonical_id.ends_with("/package.json") {
                                    let canonical =
                                        verter_semantic::resolver_core::normalize_canonical_id(
                                            &canonical_id,
                                        );
                                    world.manifest_fingerprints.remove(&canonical);
                                    self.advance_resolution_fact(
                                        &mut world.facts,
                                        ResolutionFactKey::Manifest {
                                            canonical: CanonicalResolutionId::new(canonical),
                                            population: ResolutionPopulation::Base,
                                        },
                                    );
                                }
                            }
                            result.invalidated_files.push(canonical_id);
                            content_changed = true;
                            base_changed = true;
                        }
                    }
                    WorkspaceChange::FileDeleted { canonical_id } => {
                        self.invalidate_package_manifest(&canonical_id);
                        self.mark_parent_dir_dirty(&canonical_id);
                        self.remove_file_edges_in_world(world, &canonical_id);
                        self.snapshot.write().remove(&canonical_id);
                        self.update_base_path_facts(
                            world,
                            &canonical_id,
                            verter_semantic::resolver_core::PathProbe::Absent,
                        );
                        self.update_base_realpath_fact(world, &canonical_id, None);
                        self.update_base_manifest_fact(world, &canonical_id, None);
                        result.invalidated_files.push(canonical_id);
                        content_changed = true;
                        base_changed = true;
                    }
                    WorkspaceChange::DirectoryTreeDirty { prefix } => {
                        self.package_index.write().invalidate_under(&prefix);
                        self.dir_index.write().mark_dirty_under(&prefix);
                        // The member set is unknown (an out-of-band disk
                        // change), so record the narrowest subtree scope at
                        // the batch's post-mutation generation.
                        let normalized =
                            verter_semantic::resolver_core::normalize_canonical_id(&prefix);
                        self.advance_resolution_fact(
                            &mut world.facts,
                            ResolutionFactKey::RecoveryScope {
                                canonical_prefix: CanonicalResolutionId::new(normalized),
                                population: ResolutionPopulation::Base,
                            },
                        );
                        subtree_transitions.push(prefix);
                        content_changed = true;
                        base_changed = true;
                    }
                    WorkspaceChange::ConfigChanged { canonical_id: _ } => {
                        result.graph_rebuilt = true;
                        result.generation = Some(self.project_graph.read().generation() + 1);
                        base_changed = true;
                        // A config change republishes the env-hash tables
                        // WITHOUT touching content, so the source-env
                        // domain must advance on its own counter here.
                        self.bump_source_env_generation();
                    }
                }
            }

            if content_changed {
                let generation = self.bump_content_generation_in_world();
                for canonical_id in &result.invalidated_files {
                    self.record_content_transition_at(canonical_id, generation);
                }
                for prefix in subtree_transitions {
                    self.record_subtree_content_transition_at(&prefix, generation);
                }
            }

            (result, base_changed)
        })
    }

    /// Set exact resolutions for a file.
    pub(crate) fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        let retained = resolutions.clone();
        self.mutate_resolution_world(|world| {
            let result = self
                .edges
                .write()
                .replace_exact_resolutions(canonical_id, resolutions);
            if !result.changed {
                return (result, false);
            }
            let world_changed =
                self.replace_world_exact_resolutions(world, canonical_id, &retained);
            assert!(
                world_changed,
                "changed exact edge state must change the resolution-world exact table"
            );
            (result, true)
        })
    }

    /// Replace owner's transitive-semantic dep set. Always fires.
    pub(crate) fn replace_semantic_transitive(&self, canonical_id: &str, deps: BTreeSet<String>) {
        self.edges
            .write()
            .replace_semantic_transitive(canonical_id, deps);
    }

    /// Inspection — clone of an owner's dependency snapshot.
    #[allow(dead_code)]
    pub(crate) fn dependency_snapshot(&self, canonical_id: &str) -> Option<DependencySnapshotView> {
        self.edges.read().snapshot(canonical_id)
    }

    /// Add a single ambient-resolved dep (incremental). Routes ambient
    /// dependencies into the dedicated `ambient_resolved` class so they
    /// survive `record_parsed_edges` re-records.
    pub(crate) fn add_ambient_resolved_dep(&self, canonical_id: &str, virtual_id: &str) -> bool {
        self.edges
            .write()
            .add_ambient_resolved_dep(canonical_id, virtual_id)
    }

    /// Resolve an import under a READER-AUTHORITATIVE evidence capability:
    /// the reader's own reads ARE the live source. Test-only — the only readers for which that holds without a
    /// backend saying so are the crate's synthetic in-memory readers. A
    /// production backend states its capability explicitly through
    /// [`Self::resolve_import_with_evidence`], which is why this entry is
    /// `cfg(test)`: production cannot name it.
    #[cfg(test)]
    pub(crate) fn resolve_import(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> Option<verter_semantic::resolver_core::ResolveResult> {
        self.resolve_import_outcome(reader, importer_id, specifier, ctx)
            .into_transient_result()
    }

    /// See [`Self::resolve_import`] — the `cfg(test)` reader-authoritative
    /// entry, in outcome form.
    ///
    /// Resolution priority is unchanged for every entry: exact resolutions
    /// (authoritative, no fallthrough on match), then the published snapshot
    /// resolver, then `None` — never a heuristic fallback.
    #[cfg(test)]
    pub(crate) fn resolve_import_outcome(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> ResolutionOutcome {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        self.resolve_import_outcome_in_published(
            reader,
            crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
            None,
            importer_id,
            specifier,
            ctx,
            &mut input_ledger,
            &|| true,
        )
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn resolve_import_with_evidence(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> Option<verter_semantic::resolver_core::ResolveResult> {
        self.resolve_import_outcome_with_evidence(reader, evidence, importer_id, specifier, ctx)
            .into_transient_result()
    }

    /// Sealed outer boundary around exact lookup, selection, cache lookup,
    /// resolver observations, provider projection, and completion admission.
    ///
    /// `evidence` is REQUIRED, and that is the whole point: the backend that
    /// owns this Engine states its live-observation capability here, once. A
    /// reader composed on top of it — an overlay snapshot, a transaction
    /// recorder, a frozen replay — cannot forward it wrongly, because it does
    /// not carry it at all.
    pub(crate) fn resolve_import_outcome_with_evidence(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> ResolutionOutcome {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        self.resolve_import_outcome_in_published(
            reader,
            evidence,
            None,
            importer_id,
            specifier,
            ctx,
            &mut input_ledger,
            &|| true,
        )
    }

    /// Resolve only when `expected_published` is the published state pinned by
    /// the captured Engine world. This is the LSP snapshot bridge: a resolver
    /// snapshot superseded before or during the transaction returns a typed
    /// refusal instead of silently resolving through a newer world.
    pub(crate) fn resolve_import_outcome_for_published(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        expected_published: &Arc<crate::published_state::PublishedRoot>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> ResolutionOutcome {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        self.resolve_import_outcome_for_published_in_operation(
            reader,
            evidence,
            expected_published,
            importer_id,
            specifier,
            ctx,
            &mut input_ledger,
            &|| true,
        )
    }

    pub(crate) fn resolve_import_outcome_for_published_in_operation(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        expected_published: &Arc<crate::published_state::PublishedRoot>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
        input_ledger: &mut crate::resolver::InputResolutionLedger,
        final_validate: &dyn Fn() -> bool,
    ) -> ResolutionOutcome {
        self.resolve_import_outcome_in_published(
            reader,
            evidence,
            Some(expected_published),
            importer_id,
            specifier,
            ctx,
            input_ledger,
            final_validate,
        )
    }

    /// Reader-driven, value-sensitive evidence refresh: re-observe the
    /// canonicals a candidate's witness actually recorded — strictly
    /// O(witness facts) — and advance only facts whose observed value
    /// changed. Returns whether the resolution world changed (the caller then
    /// retries against the new world through the ordinary mutation-protocol
    /// path).
    ///
    /// Two rules select what is re-observed, and a canonical qualifying under
    /// either is re-observed once:
    ///
    /// * the pending-transition ledger — a canonical whose content
    ///   transitioned through [`Self::bump_content_generation_for`], which
    ///   advances zero facts speculatively and leaves the re-observation to
    ///   the resolve path where a reader is in scope;
    /// * a backend whose declared capability is
    ///   [`crate::resolution_currency::ResolutionEvidenceSource::Uncovered`],
    ///   for every witness canonical whose evidence has not been read live at
    ///   the CURRENT content generation. That backend receives
    ///   resolver-visible changes with no event at all — an installed package
    ///   under `node_modules` is the case — so a recorded `Absent` would
    ///   otherwise keep validating for the process's lifetime.
    ///
    /// The second rule costs nothing in the steady state: with no content
    /// transition every stamp is current and the pass selects no target. It
    /// heals at the first resolution after the next content transition, which
    /// is precisely where clearing the whole resolution memo per content
    /// generation used to heal — reached here by re-reading one candidate's
    /// own witness instead of discarding every candidate in the workspace.
    /// Whether `signature` is unusable as the re-observation plan the
    /// declared evidence source requires — in which case the candidate it
    /// belongs to must NOT be reused.
    ///
    /// Only [`crate::resolution_currency::ResolutionEvidenceSource::Uncovered`]
    /// is affected. That backend receives resolver-visible changes with no
    /// event at all, so its healing rule is "re-read every witness canonical
    /// whose evidence has not been read live at the current content
    /// generation" — a rule stated over the witness's OWN path
    /// observations, which an un-enumerable resolution witness does not
    /// expose. Unlike the pending ledger there is no sound superset to
    /// fall back to (no ledger records "every path canonical ever
    /// observed"), so the honest answer is that this witness cannot be
    /// certified under this backend. Declining costs a cold recompute;
    /// reusing it would serve a candidate whose uncovered evidence was
    /// never re-read.
    fn witness_evidence_is_unenumerable(
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        signature: &crate::ReadSetSignature,
    ) -> bool {
        use crate::resolution_currency::ResolutionEvidenceSource;
        matches!(evidence, ResolutionEvidenceSource::Uncovered(_))
            && signature.resolution_evidence_is_unenumerable()
    }

    fn refresh_resolution_evidence(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        signature: &crate::ReadSetSignature,
    ) -> bool {
        use crate::resolution_currency::ResolutionEvidenceSource;
        // Fail closed: a caller that declared no live source re-observes
        // nothing and stamps nothing. It can only ever fail to heal — it can
        // never certify stale state as freshly verified.
        let reobserve_reused = match evidence {
            ResolutionEvidenceSource::Inert => return false,
            ResolutionEvidenceSource::ReaderAuthoritative => false,
            ResolutionEvidenceSource::Uncovered(_) => true,
        };
        let generation = self.current_content_generation();
        // NEVER hold both ledgers at once, here or at the settle below. They
        // are independent maps under independent locks, and `parking_lot`
        // grants neither reentrancy nor a global order — one site taking
        // pending-then-verified while another takes verified-then-pending is
        // an ABBA deadlock between two concurrent resolutions, and it
        // presents as the worst possible failure: the request wedges with no
        // CPU burn, no timeout and no panic.
        // A resolution witness can name none of the path canonicals it
        // depends on either because its bucket compacted or because its
        // derived fact is not itself a path observation.
        // Projecting canonicals from either shape yields an
        // UNDER-APPROXIMATION: the pass silently heals nothing and a recorded
        // `Absent` can keep validating for the life of the process. The
        // pending ledger is the sound bounded superset and drains as it is
        // read.
        let unenumerable_resolution = signature.resolution_evidence_is_unenumerable();
        let mut targets: Vec<Arc<str>> = {
            let pending = self.pending_resolution_refresh.read();
            if pending.is_empty() && !reobserve_reused {
                return false;
            }
            if unenumerable_resolution {
                pending
                    .iter()
                    .map(|canonical| Arc::from(canonical.as_str()))
                    .collect()
            } else {
                signature
                    .canonical_ids()
                    .into_iter()
                    .filter(|canonical| pending.contains(canonical.as_ref()))
                    .collect()
            }
        };
        if reobserve_reused {
            let verified = self.evidence_verified_generation.read();
            for canonical in signature.resolution_path_canonical_ids() {
                if verified.get(canonical.as_ref()).copied() == Some(generation)
                    || targets.contains(&canonical)
                {
                    continue;
                }
                targets.push(canonical);
            }
        }
        if targets.is_empty() {
            return false;
        }

        // Re-observe OUTSIDE the resolution-world write gate. The gate clones
        // the whole immutable root, and the overwhelmingly common outcome is
        // that nothing moved; entering it per candidate per generation would
        // trade a cache clear for a world clone.
        //
        // ONE live read per canonical, through the reader's single evidence
        // primitive, and the value it returns is the value that gets folded.
        // Re-probing afterwards through the ordinary accessors is what made
        // this path both expensive and wrong: those accessors answer from the
        // very event-invalidated caches the live read exists to check.
        let recorded_before = self.resolution_world.load_full();
        let mut observations = Vec::with_capacity(targets.len());
        for canonical in &targets {
            let canonical = canonical.as_ref();
            // An overlay-shadowed canonical's reader observation is
            // overlay-effective; it must not overwrite base evidence.
            if self.overlay.read().has_overlay(canonical) {
                continue;
            }
            let key = verter_semantic::resolver_core::normalize_canonical_id(canonical);
            let recorded = Self::recorded_baseline(&recorded_before, &key);
            // A source that could not produce a trustworthy live observation
            // gets no stamp and no fold. The stamp certifies an actual live
            // read; certifying anything else re-labels stale state as freshly
            // verified, which is worse than not healing. An `Inaccessible` or
            // `Unknown` probe is NOT that case — those are observed values,
            // and they arrive here as values.
            let observed = match evidence {
                ResolutionEvidenceSource::Inert => None,
                ResolutionEvidenceSource::ReaderAuthoritative => {
                    Self::observe_through_reader(reader, canonical)
                }
                ResolutionEvidenceSource::Uncovered(source) => {
                    source.observe_live_resolution_evidence(canonical, recorded.as_ref())
                }
            };
            let Some(live) = observed else {
                continue;
            };
            observations.push(ReobservedEvidence {
                canonical: key,
                live,
            });
        }
        if observations.is_empty() {
            return false;
        }

        // These canonicals — and only these — were read live at `generation`,
        // so the stamp and the ledger are settled for them whether or not
        // anything moved. Settled one ledger at a time; see the ABBA note at
        // the target selection above.
        {
            let mut verified = self.evidence_verified_generation.write();
            for observation in &observations {
                verified.insert(observation.canonical.clone(), generation);
            }
        }
        {
            let mut pending = self.pending_resolution_refresh.write();
            for observation in &observations {
                pending.remove(&observation.canonical);
            }
        }

        // Decide READ-ONLY whether the gate is worth entering. Every entry to
        // `mutate_resolution_world` drives the resolution epoch odd for the
        // duration of a whole world clone, and every concurrent
        // `capture_resolution_world` inside that window returns `None` and
        // burns one of its eight attempts. This pass runs per retained
        // candidate per resolution and its overwhelmingly common outcome is
        // that nothing moved, so entering unconditionally made a workspace
        // with concurrent resolutions starve its own captures: attempts
        // exhausted, `ResolutionRetryExhausted` returned for a resolution that
        // had nothing wrong with it, and — because the LSP's carrier-import
        // closure treats a refusal as "not live" — a rename that silently
        // returned no edits. The classification is the SAME function the
        // write uses, and the write re-evaluates under the lock, so this is a
        // fast path and not a second rule.
        let current = self.resolution_world.load_full();
        let needs_write = observations.iter().any(|observation| {
            Self::observed_families(observation).iter().any(|value| {
                Self::baseline_fold_verdict(&current, value) != BaselineFold::Unchanged
            })
        });
        if !needs_write {
            return false;
        }

        self.mutate_resolution_world(|world| {
            let mut fold = BaselineFold::Unchanged;
            for observation in &observations {
                for value in Self::observed_families(observation) {
                    fold = fold.merge(self.fold_observed_baseline(world, &value));
                }
            }
            // A FILL retains but publishes nothing: no fact moved, so no
            // captured root is superseded and the in-flight attempt does not
            // retry. Only a conflict is a reason to retry.
            (fold == BaselineFold::Conflicted, fold.write())
        })
    }

    /// The content generation at which `canonical`'s base resolution evidence
    /// was last read LIVE, or `None` when it never has been.
    #[cfg(test)]
    pub(crate) fn evidence_verified_generation_for_test(&self, canonical: &str) -> Option<u64> {
        let key = verter_semantic::resolver_core::normalize_canonical_id(canonical);
        self.evidence_verified_generation.read().get(&key).copied()
    }

    /// Whether `canonical` is still awaiting evidence re-observation.
    #[cfg(test)]
    pub(crate) fn pending_resolution_refresh_for_test(&self, canonical: &str) -> bool {
        let key = verter_semantic::resolver_core::normalize_canonical_id(canonical);
        self.pending_resolution_refresh.read().contains(&key)
    }

    /// Live observation for a backend whose reader IS its own live source —
    /// [`crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative`].
    ///
    /// Reachable ONLY through that arm. It reads the reader's ordinary
    /// accessors, which is a live read exactly when nothing event-invalidated
    /// stands behind them; for any other backend it would read caches back and
    /// certify them, which is why no default ever routes here.
    fn observe_through_reader(
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
    ) -> Option<crate::resolution_currency::LiveResolutionObservation> {
        Some(crate::resolution_currency::LiveResolutionObservation {
            probe: reader.probe_path(canonical_id),
            realpath: reader
                .realpath(canonical_id)
                .map(|path| verter_semantic::resolver_core::normalize_canonical_id(&path)),
            manifest: crate::resolution_currency::is_package_manifest_path(canonical_id).then(
                || {
                    reader.read_file(canonical_id).map(|source| {
                        crate::resolution_currency::manifest_resolution_fingerprint(&source)
                    })
                },
            ),
        })
    }

    /// The world's recorded baseline for `canonical`, PER FAMILY, so a reader
    /// can decide "did my memo lie?" without knowing anything about the
    /// world — and without a family nobody has ever observed counting as a
    /// belief. `None` when NO family has a recorded value.
    fn recorded_baseline(
        world: &ResolutionWorldRoot,
        canonical: &str,
    ) -> Option<crate::resolution_currency::RecordedResolutionBaseline> {
        let baseline = crate::resolution_currency::RecordedResolutionBaseline {
            probe: world.path_probes.get(canonical).copied(),
            realpath: world.realpaths.get(canonical).cloned(),
            manifest: world.manifest_fingerprints.get(canonical).copied(),
        };
        (!baseline.is_empty()).then_some(baseline)
    }

    /// The families one live observation carries, as fold inputs. A manifest
    /// limb is present exactly when the canonical is a manifest path.
    fn observed_families(observation: &ReobservedEvidence) -> Vec<ObservedBaselineValue<'_>> {
        let mut values = vec![
            ObservedBaselineValue {
                canonical: &observation.canonical,
                value: ObservedFamilyValue::Probe(observation.live.probe),
            },
            ObservedBaselineValue {
                canonical: &observation.canonical,
                value: ObservedFamilyValue::Realpath(observation.live.realpath.as_deref()),
            },
        ];
        if let Some(fingerprint) = observation.live.manifest {
            values.push(ObservedBaselineValue {
                canonical: &observation.canonical,
                value: ObservedFamilyValue::Manifest(fingerprint),
            });
        }
        values
    }

    /// Fold an admitted attempt's raw observed values into the world's
    /// recorded evidence baseline through the mutation protocol. A value that
    /// only fills an unrecorded baseline advances no fact version and
    /// supersedes no captured root; a value CONFLICTING with the recorded
    /// baseline reveals state newer than the captured root — the conflict
    /// enters the mutation protocol (facts advance) and the caller must retry
    /// instead of admitting. Returns whether a conflict was published.
    ///
    /// TOTAL over the reobservable families: probes, realpaths AND manifests.
    /// The totality is what makes "unrecorded baseline at refresh time"
    /// impossible for a canonical any admitted witness names, which in turn
    /// is what makes the first-observation arm of
    /// [`Self::fold_observed_baseline`] a residual rather than load-bearing
    /// policy. While manifests were missing, no manifest baseline was ever
    /// recorded on the resolve path, so an `exports`/`types` rewrite could
    /// never be detected as a change by anything downstream.
    fn fold_observed_base_evidence(
        &self,
        held_session: Option<SessionFingerprint>,
        observed: ObservedResolutionValues,
    ) -> bool {
        if observed.is_empty() {
            return false;
        }
        // These values were just read live by the attempt that is admitting,
        // so the evidence stamp is current for them. Without this the FIRST
        // reuse of every freshly-admitted candidate would re-read evidence
        // the admitting attempt had already read in the same generation.
        {
            let generation = self.current_content_generation();
            let mut verified = self.evidence_verified_generation.write();
            for canonical in observed
                .path_probes
                .iter()
                .map(|(canonical, _)| canonical)
                .chain(observed.realpaths.iter().map(|(canonical, _)| canonical))
                .chain(observed.manifests.iter().map(|(canonical, _)| canonical))
            {
                verified.insert(canonical.clone(), generation);
            }
        }
        let mut conflict = false;
        self.mutate_resolution_world_locked_with_held_session(held_session, |world| {
            let mut fold = BaselineFold::Unchanged;
            let overlay = self.overlay.read();
            let families = observed
                .path_probes
                .iter()
                .map(|(canonical, outcome)| ObservedBaselineValue {
                    canonical,
                    value: ObservedFamilyValue::Probe(*outcome),
                })
                .chain(observed.realpaths.iter().map(|(canonical, resolved)| {
                    ObservedBaselineValue {
                        canonical,
                        value: ObservedFamilyValue::Realpath(resolved.as_deref()),
                    }
                }))
                .chain(observed.manifests.iter().map(|(canonical, fingerprint)| {
                    ObservedBaselineValue {
                        canonical,
                        value: ObservedFamilyValue::Manifest(*fingerprint),
                    }
                }));
            for family in families {
                // Overlay-shadowed observations are overlay-effective.
                if overlay.has_overlay(family.canonical) {
                    continue;
                }
                fold = fold.merge(self.fold_observed_baseline(world, &family));
            }
            conflict = fold == BaselineFold::Conflicted;
            ((), fold.write())
        });
        conflict
    }

    fn resolve_import_outcome_in_published(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        expected_published: Option<&Arc<crate::published_state::PublishedRoot>>,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
        input_ledger: &mut crate::resolver::InputResolutionLedger,
        final_validate: &dyn Fn() -> bool,
    ) -> ResolutionOutcome {
        crate::probe_scope!(RESOLVE_IN_PUBLISHED);
        let population = reader.resolution_population();
        let cache_key = LazyResolutionCacheKey {
            importer_id: importer_id.to_string(),
            specifier: specifier.to_string(),
            phase: ctx.phase,
            kind: ctx.kind,
            population,
        };
        let request_local_snapshot = reader.resolution_snapshot_is_request_local();
        loop {
            crate::probe_scope!(RESOLVE_ATTEMPT);
            let captured = {
                crate::probe_scope!(RESOLVE_CAPTURE_WORLD);
                self.capture_stable_resolution_world(population)
            };
            let Some(captured) = captured else {
                #[cfg(test)]
                resolution_test_hooks::record_return_only();
                return ResolutionOutcome::refused(
                    None,
                    verter_audit::NonAdmissionReason::ResolutionRetryExhausted,
                );
            };
            if expected_published.is_some_and(|expected| {
                !captured
                    .world
                    .base
                    .published
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, expected))
            }) {
                return ResolutionOutcome::new(
                    None,
                    SignatureAdmission::NonCacheable(
                        verter_audit::NonAdmissionReason::ResolutionViewSuperseded,
                    ),
                    Vec::new(),
                    false,
                    false,
                    false,
                );
            }
            #[cfg(test)]
            resolution_test_hooks::capture_attempt_world();

            let transaction = Mutex::new(ResolutionTransaction::new(Arc::clone(&captured.world)));
            let exact_fact =
                ResolutionFactKey::exact_importer(importer_id, specifier, ctx, population);
            transaction.lock().observe(exact_fact.clone());
            let observed_exact_version = captured.world.fact_version(&exact_fact);

            let candidates: LazyResolutionCandidates = {
                crate::probe_scope!(RESOLVE_CANDIDATE_READ);
                if request_local_snapshot {
                    LazyResolutionCandidates::new()
                } else {
                    self.lazy_resolution_cache
                        .read()
                        .get(&cache_key)
                        .cloned()
                        .unwrap_or_default()
                }
            };
            // Every retained candidate is screened against the captured
            // world's exact fact, not just the most recent one: a slot that
            // holds a superseded target still owns that target's witness, and
            // the demand that supersedes it must name every witness it
            // rejected.
            let mut rejected_exact_targets = Vec::new();
            {
                crate::probe_scope!(RESOLVE_SCREEN_EXACT);
                for candidate in candidates.iter() {
                    let candidate_exact_version =
                        candidate.signature.resolution_fact_version(&exact_fact);
                    if candidate_exact_version.is_some()
                        && candidate_exact_version != Some(observed_exact_version)
                    {
                        rejected_exact_targets.push(
                            candidate
                                .result
                                .as_ref()
                                .map(|result| result.source_id.clone()),
                        );
                    }
                }
            }

            // Exact lookup is rooted in the captured immutable world, including
            // the miss. The hook fires after the observation but before the
            // attempt can select a resolver or admit.
            let exact = {
                crate::probe_scope!(RESOLVE_EXACT_LOOKUP);
                captured
                    .world
                    .base
                    .exact(importer_id, specifier, ctx)
                    .cloned()
            };
            let exact_hit = exact.is_some();
            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::ExactTableLookup);

            let context_fact = ResolutionFactKey::context_importer(importer_id, population);
            transaction.lock().observe(context_fact);
            let selected_context = {
                crate::probe_scope!(RESOLVE_CONTEXT_SELECT);
                match selected_context_for_path(captured.world.base.as_ref(), importer_id) {
                    Ok(context) => Some(context),
                    Err(_) => {
                        transaction.lock().mark_incomplete_provenance();
                        None
                    }
                }
            };
            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::ProjectSelection);

            let mut reused = false;
            let mut publish_candidate = false;

            // Reader-driven evidence refresh for the retained candidates' own
            // recorded canonicals; a refreshed (changed) world means the
            // captured root is superseded — retry against the new world.
            let mut refreshed = false;
            {
                crate::probe_scope!(RESOLVE_REFRESH_EVID);
                for entry in candidates.iter() {
                    refreshed |=
                        self.refresh_resolution_evidence(reader, evidence, &entry.signature);
                }
            }
            if refreshed {
                let tracked = TransactionReader::new(reader, &transaction);
                if input_ledger.charge_outer_restart(&tracked).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        rejected_exact_targets,
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }
            // One owner-edge authority for every resolution producer, the
            // caller-supplied exact table included. A warm candidate is
            // reused only when its query identity matches and its witness
            // still validates against the captured world; because the
            // witness records the `ExactResolution` fact, a caller-supplied
            // exact change invalidates the candidate through the same rail
            // as any other resolution input, and the recomputed result —
            // exact or resolver-derived — republishes through the same slot.
            let reusable = {
                crate::probe_scope!(RESOLVE_REUSE_FIND);
                candidates.iter().find(|entry| {
                    let candidate_context = {
                        crate::probe_scope!(RESOLVE_REUSE_CTX);
                        Self::complete_provider_context(
                            captured.world.base.as_ref(),
                            selected_context.clone(),
                            entry.result.as_ref(),
                            population,
                            &transaction,
                        )
                    };
                    let Some(candidate_context) = candidate_context else {
                        return false;
                    };
                    let query_matches = {
                        crate::probe_scope!(RESOLVE_REUSE_QUERY);
                        let query = ResolutionQueryKey::importer(
                            importer_id,
                            specifier,
                            ctx,
                            candidate_context,
                            population,
                        );
                        entry.query == query
                    };
                    if !query_matches {
                        return false;
                    }
                    crate::probe_scope!(RESOLVE_REUSE_VALIDATE);
                    entry.signature.validates(captured.world.as_ref())
                })
            };
            // A witness the declared evidence source cannot re-observe is
            // not a witness this attempt may stand on. See
            // `witness_evidence_is_unenumerable`.
            let reusable = reusable.filter(|entry| {
                !Self::witness_evidence_is_unenumerable(evidence, &entry.signature)
            });
            let result = if let Some(entry) = reusable {
                // The DAG's reuse seam. The reused candidate's signature
                // is NOT folded in: the outcome roots on this query's own
                // decision node, whose version reverse propagation keeps
                // honest, so a warm answer no longer restates every leaf
                // the candidate transitively touched.
                transaction.lock().set_query(entry.query.clone());
                reused = true;
                entry.result.clone()
            } else {
                publish_candidate = true;
                if let Some(exact) = exact {
                    exact.resolved_canonical_id.as_ref().map(|id| {
                        captured
                            .world
                            .base
                            .published
                            .as_ref()
                            .map(|root| {
                                root.snapshot.resolver.project_exact_result(
                                    importer_id,
                                    specifier,
                                    id.clone(),
                                    ctx,
                                )
                            })
                            .unwrap_or_else(|| {
                                transaction.lock().mark_incomplete_provenance();
                                ResolveResult {
                                    source_id: id.clone(),
                                    provider_id: id.clone(),
                                    provider_specifier: specifier.to_string(),
                                    provider_target:
                                        verter_semantic::resolver_core::ProviderTarget::SourceFile,
                                    resolution_kind:
                                        verter_semantic::resolver_core::ResolutionKind::Bundler,
                                    owner_tsconfig_path: None,
                                }
                            })
                    })
                } else {
                    crate::probe_scope!(RESOLVE_TRACKED);
                    let tracked = TransactionReader::new(reader, &transaction);
                    let capability = TrackedResolutionCapability::new();
                    let driven = captured
                        .world
                        .base
                        .published
                        .as_ref()
                        .map_or(Ok(None), |root| {
                            let request = verter_semantic::resolver_core::ResolveRequest {
                                importer_id: importer_id.to_string(),
                                specifier: specifier.to_string(),
                                kind: ctx.kind,
                                phase: ctx.phase,
                            };
                            crate::resolver::resolve_tracked(
                                &root.snapshot.resolver,
                                &capability,
                                &tracked,
                                input_ledger,
                                &request,
                            )
                        });
                    match driven {
                        Ok(result) => result,
                        Err(_) => {
                            transaction.lock().mark_incomplete_provenance();
                            #[cfg(test)]
                            resolution_test_hooks::record_return_only();
                            return ResolutionOutcome::new(
                                None,
                                transaction.into_inner().finish(),
                                rejected_exact_targets,
                                true,
                                false,
                                false,
                            );
                        }
                    }
                }
            };
            let terminal_admission = transaction.lock().input_resolution_terminal_admission();
            if let Some(admission) = terminal_admission {
                #[cfg(test)]
                resolution_test_hooks::record_return_only();
                input_ledger.release_applied_outputs();
                return ResolutionOutcome::new(
                    result,
                    admission,
                    rejected_exact_targets,
                    true,
                    false,
                    false,
                );
            }
            if !reused {
                let complete_context = Self::complete_provider_context(
                    captured.world.base.as_ref(),
                    selected_context,
                    result.as_ref(),
                    population,
                    &transaction,
                );
                if let Some(complete_context) = complete_context {
                    transaction.lock().set_query(ResolutionQueryKey::importer(
                        importer_id,
                        specifier,
                        ctx,
                        complete_context,
                        population,
                    ));
                }
            }
            #[cfg(test)]
            resolution_test_hooks::fire(
                resolution_test_hooks::ResolutionPhase::PreAdmissionValidation,
            );
            if !self.resolution_world_still_current(&captured) {
                let tracked = TransactionReader::new(reader, &transaction);
                if input_ledger.charge_outer_restart(&tracked).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        rejected_exact_targets,
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }

            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::RequestCompletion);

            // The final fence and publication are serialized against all world
            // writers. No mutation can land between validation and insertion.
            let (_publication, _session_publication) = {
                crate::probe_scope!(RESOLVE_PUBLISH_LOCK);
                (
                    self.resolution_world_write.lock(),
                    captured
                        .session_domain
                        .as_ref()
                        .map(|domain| domain.write.lock()),
                )
            };
            if !self.resolution_world_still_current(&captured) {
                let tracked = TransactionReader::new(reader, &transaction);
                if input_ledger.charge_outer_restart(&tracked).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        rejected_exact_targets,
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }
            if !final_validate() {
                input_ledger.release_applied_outputs();
                return ResolutionOutcome::new(
                    None,
                    SignatureAdmission::NonCacheable(
                        verter_audit::NonAdmissionReason::ResolutionViewSuperseded,
                    ),
                    rejected_exact_targets,
                    true,
                    false,
                    false,
                );
            }
            if !reader.resolution_event_bridge_complete() {
                transaction.lock().mark_untracked_backend();
            }
            #[cfg(test)]
            resolution_test_hooks::record_completed_outputs_at_final_fence(
                input_ledger.applied_output_count_for_test(),
            );
            let query = transaction.lock().query().cloned();
            let mut transaction = transaction.into_inner();
            // The decision's COMPLETE direct edge set, taken before
            // finalisation consumes the transaction. Direct only: the
            // primitive facts this attempt observed plus the child
            // decisions it reused, never a child's own edges.
            let direct_edges = transaction.direct_edges();
            let observed_values = transaction.take_observed_values();
            let mut admission = {
                crate::probe_scope!(RESOLVE_TXN_FINISH);
                transaction.finish()
            };
            if matches!(
                &admission,
                SignatureAdmission::Cacheable(signature)
                    if !signature.validates(captured.world.as_ref())
            ) {
                admission = SignatureAdmission::NonCacheable(
                    verter_audit::NonAdmissionReason::ResolutionWorldChanged,
                );
            }
            if publish_candidate
                && !request_local_snapshot
                && matches!(&admission, SignatureAdmission::Cacheable(_))
                && {
                    crate::probe_scope!(RESOLVE_FOLD_EVIDENCE);
                    self.fold_observed_base_evidence(
                        Self::held_session_of(&captured),
                        observed_values,
                    )
                }
            {
                // An observed value conflicted with the recorded baseline:
                // state newer than the captured root entered through the
                // mutation protocol. Retry against the new world.
                if input_ledger.charge_outer_restart(reader).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        SignatureAdmission::NonCacheable(
                            verter_audit::NonAdmissionReason::BudgetExceeded,
                        ),
                        rejected_exact_targets,
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }
            let cacheable_signature = match &admission {
                SignatureAdmission::Cacheable(signature) => Some(signature.clone()),
                SignatureAdmission::NonCacheable(_) => None,
            };

            let mut published = false;
            if reused {
                self.vfs_provenance
                    .import_resolution_cache_hit_count
                    .fetch_add(1, Ordering::Relaxed);
            } else if !exact_hit {
                self.vfs_provenance
                    .import_resolution_cache_miss_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            if publish_candidate && !request_local_snapshot {
                if let (Some(signature), Some(query)) = (cacheable_signature, query.clone()) {
                    crate::probe_scope!(RESOLVE_ADMIT);
                    let evicted = admit_resolution_candidate(
                        self.lazy_resolution_cache
                            .write()
                            .entry(cache_key.clone())
                            .or_default(),
                        LazyResolutionCacheEntry {
                            result: result.clone(),
                            query: query.clone(),
                            signature,
                        },
                    );
                    // The candidate, its decision node and the removal of
                    // every aged-out sibling's decision all land under the
                    // same fence, so a slot can never hold an answer whose
                    // decision has no edges recorded, and no decision can
                    // outlive the candidate that serves it.
                    for query in evicted {
                        self.remove_resolution_decision(&captured, query);
                    }
                    self.publish_resolution_decision(&captured, query, direct_edges);
                    published = true;
                }
            }
            // **The DAG's consumer-facing product.** A cacheable outcome
            // whose decision node is in the graph roots on THAT node —
            // one typed derived fact — instead of on the attempt's own
            // leaf set. Three properties follow, and all three are
            // load bearing:
            //
            // * BOUNDED. An owner witness is one fact per specifier
            //   rather than the union of every specifier's transitive
            //   closure, which is the growth the decision DAG removes.
            // * WARMTH-INDEPENDENT. Cold and warm answers to the same
            //   demand produce the identical witness, so a producer's
            //   identical-recomputation dedupe still recognises itself.
            // * VALID AGAINST THE CAPTURED VIEW. Publication mints no
            //   version, so the node reads exactly what this attempt's
            //   own captured world says — a consumer holding a pinned
            //   request view can root on it and warm-hit through that
            //   same view.
            //
            // A request-local snapshot publishes no node, so it keeps its
            // precise observation set: rooting on a node that does not
            // exist would be a witness nothing can ever invalidate.
            if !request_local_snapshot
                && matches!(&admission, SignatureAdmission::Cacheable(_))
                && (published || reused)
            {
                if let Some(query) = query.clone() {
                    let node = ResolutionFactKey::decision(query);
                    let version = captured.world.fact_version(&node);
                    admission =
                        SignatureAdmission::Cacheable(crate::ReadSetSignature::new(Arc::from([
                            crate::FactVersionRef::ResolveImports(
                                crate::ResolveImportsFactRef::Resolution(
                                    crate::resolution_currency::ResolutionFactRef {
                                        key: node,
                                        version,
                                    },
                                ),
                            ),
                        ])));
                }
            }
            if !request_local_snapshot && matches!(&admission, SignatureAdmission::Cacheable(_)) {
                input_ledger.commit_loaded_inputs(reader);
                if let Some(ref result) = result {
                    self.edges
                        .write()
                        .add_lazy_resolved_dep(importer_id, &result.source_id);
                }
            }

            #[cfg(test)]
            match &admission {
                SignatureAdmission::Cacheable(_) => {
                    resolution_test_hooks::record_cacheable_admission();
                }
                SignatureAdmission::NonCacheable(_) => resolution_test_hooks::record_return_only(),
            }

            #[cfg(test)]
            resolution_test_hooks::record_completed_outputs_at_publication(
                input_ledger.applied_output_count_for_test(),
            );
            input_ledger.release_applied_outputs();

            return ResolutionOutcome::new(
                result,
                admission,
                rejected_exact_targets,
                !reused,
                published,
                reused,
            );
        }
    }

    fn complete_provider_context(
        world: &ResolutionWorldRoot,
        selected: Option<crate::resolution_currency::ResolveContextId>,
        result: Option<&ResolveResult>,
        population: ResolutionPopulation,
        transaction: &Mutex<ResolutionTransaction>,
    ) -> Option<crate::resolution_currency::ResolveContextId> {
        let selected = selected?;
        let Some(result) = result else {
            return Some(selected);
        };
        transaction
            .lock()
            .observe(ResolutionFactKey::context_importer(
                &result.source_id,
                population,
            ));
        match selected_context_for_path(world, &result.source_id) {
            Ok(target)
                if target == ResolveContextId::unowned()
                    && result.owner_tsconfig_path.is_none()
                    && result.provider_target
                        == verter_semantic::resolver_core::ProviderTarget::SourceFile =>
            {
                Some(selected.with_external_provider_projection(result))
            }
            Ok(target) => Some(selected.with_provider_projection(&target)),
            Err(_) => {
                transaction.lock().mark_incomplete_provenance();
                None
            }
        }
    }

    pub(crate) fn resolve_import_for_project(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> Option<verter_semantic::resolver_core::ResolveResult> {
        self.resolve_import_for_project_outcome(reader, owner, specifier, ctx)
            .into_transient_result()
    }

    pub(crate) fn resolve_import_for_project_outcome(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
    ) -> ResolutionOutcome {
        let population = reader.resolution_population();
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        loop {
            let Some(captured) = self.capture_stable_resolution_world(population) else {
                #[cfg(test)]
                resolution_test_hooks::record_return_only();
                return ResolutionOutcome::refused(
                    None,
                    verter_audit::NonAdmissionReason::ResolutionRetryExhausted,
                );
            };
            #[cfg(test)]
            resolution_test_hooks::capture_attempt_world();

            let transaction = Mutex::new(ResolutionTransaction::new(Arc::clone(&captured.world)));
            let selected = match explicit_context(captured.world.base.as_ref(), owner) {
                Ok(context) => Some(context),
                Err(_) => {
                    transaction.lock().mark_incomplete_provenance();
                    None
                }
            };
            if let Some((project_identity, _)) = selected.as_ref() {
                let mut transaction = transaction.lock();
                transaction.observe(ResolutionFactKey::exact_explicit(
                    *project_identity,
                    specifier,
                    ctx,
                    population,
                ));
                transaction.observe(ResolutionFactKey::context_explicit(
                    *project_identity,
                    population,
                ));
            }
            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::ExactTableLookup);
            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::ProjectSelection);

            let tracked = TransactionReader::new(reader, &transaction);
            let capability = TrackedResolutionCapability::new();
            let driven = captured
                .world
                .base
                .published
                .as_ref()
                .map_or(Ok(None), |root| {
                    crate::resolver::resolve_for_project_tracked(
                        &root.snapshot.resolver,
                        &capability,
                        &tracked,
                        &mut input_ledger,
                        owner,
                        specifier,
                        ctx,
                    )
                });
            let result = match driven {
                Ok(result) => result,
                Err(_) => {
                    transaction.lock().mark_incomplete_provenance();
                    #[cfg(test)]
                    resolution_test_hooks::record_return_only();
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        Vec::new(),
                        true,
                        false,
                        false,
                    );
                }
            };
            let terminal_admission = transaction.lock().input_resolution_terminal_admission();
            if let Some(admission) = terminal_admission {
                #[cfg(test)]
                resolution_test_hooks::record_return_only();
                input_ledger.release_applied_outputs();
                return ResolutionOutcome::new(result, admission, Vec::new(), true, false, false);
            }
            let complete_context = Self::complete_provider_context(
                captured.world.base.as_ref(),
                selected.as_ref().map(|(_, context)| context.clone()),
                result.as_ref(),
                population,
                &transaction,
            );
            if let (Some((project_identity, _)), Some(complete_context)) =
                (selected, complete_context)
            {
                transaction.lock().set_query(ResolutionQueryKey::explicit(
                    project_identity,
                    specifier,
                    ctx,
                    complete_context,
                    population,
                ));
            }
            #[cfg(test)]
            resolution_test_hooks::fire(
                resolution_test_hooks::ResolutionPhase::PreAdmissionValidation,
            );
            if !self.resolution_world_still_current(&captured) {
                let tracked = TransactionReader::new(reader, &transaction);
                if input_ledger.charge_outer_restart(&tracked).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        Vec::new(),
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }
            #[cfg(test)]
            resolution_test_hooks::fire(resolution_test_hooks::ResolutionPhase::RequestCompletion);

            let _publication = self.resolution_world_write.lock();
            let _session_publication = captured
                .session_domain
                .as_ref()
                .map(|domain| domain.write.lock());
            if !self.resolution_world_still_current(&captured) {
                let tracked = TransactionReader::new(reader, &transaction);
                if input_ledger.charge_outer_restart(&tracked).is_err() {
                    return ResolutionOutcome::new(
                        None,
                        transaction.into_inner().finish(),
                        Vec::new(),
                        true,
                        false,
                        false,
                    );
                }
                continue;
            }
            if !reader.resolution_event_bridge_complete() {
                transaction.lock().mark_untracked_backend();
            }
            #[cfg(test)]
            resolution_test_hooks::record_completed_outputs_at_final_fence(
                input_ledger.applied_output_count_for_test(),
            );
            let mut admission = transaction.into_inner().finish();
            if matches!(
                &admission,
                SignatureAdmission::Cacheable(signature)
                    if !signature.validates(captured.world.as_ref())
            ) {
                admission = SignatureAdmission::NonCacheable(
                    verter_audit::NonAdmissionReason::ResolutionWorldChanged,
                );
            }
            #[cfg(test)]
            match &admission {
                SignatureAdmission::Cacheable(_) => {
                    resolution_test_hooks::record_cacheable_admission();
                }
                SignatureAdmission::NonCacheable(_) => resolution_test_hooks::record_return_only(),
            }
            if matches!(&admission, SignatureAdmission::Cacheable(_)) {
                input_ledger.commit_loaded_inputs(reader);
            }
            input_ledger.release_applied_outputs();
            return ResolutionOutcome::new(result, admission, Vec::new(), true, false, false);
        }
    }

    /// Whether `canonical_id` (or its realpath) is claimed by any
    /// registered workspace project.
    ///
    /// This consults the published snapshot's `OwnershipProject`
    /// list. A path is workspace-owned when it sits inside some
    /// project's `root` AND the suffix between that root and the
    /// path contains no further `/node_modules/` segment. The
    /// suffix-check preserves two important corner cases:
    ///
    /// - A workspace package whose root happens to live inside
    ///   `node_modules/` (uncommon but legal in pnpm) IS
    ///   workspace-owned for files under that root.
    /// - A third-party `node_modules/` source under an outer
    ///   project's root is NOT workspace-owned, because the
    ///   suffix between the outer root and the file contains
    ///   `/node_modules/`.
    ///
    /// Pnpm-symlink hops are handled by the caller resolving
    /// `realpath` before invoking this method.
    ///
    /// Returns `false` before the workspace publishes its first
    /// snapshot.
    pub(crate) fn is_workspace_owned(&self, canonical_id: &str) -> bool {
        let Some(root) = self.published_state.load_full() else {
            return false;
        };
        let path = crate::canonical_path::CanonicalPath::new(canonical_id);
        let path_str = path.as_str();
        root.snapshot.projects.iter().any(|project| {
            let project_root = project.root.as_str();
            if !path.starts_with_dir(&project.root) {
                return false;
            }
            // Suffix between project.root and path. Empty suffix
            // (path == project.root) is workspace-owned.
            let Some(suffix) = path_str.strip_prefix(project_root) else {
                return false;
            };
            !suffix_crosses_node_modules(suffix)
        })
    }

    /// Whether `canonical_id` (or its realpath) sits inside a
    /// `node_modules/` directory AND no registered project root
    /// claims the path.
    ///
    /// Discriminating cases:
    /// - third-party `node_modules/lodash/...` → `true`
    /// - workspace package linked via `node_modules/` → `false`
    ///   (the project root claims it via [`is_workspace_owned`])
    /// - workspace source outside `node_modules/` → `false`
    /// - path with no `node_modules/` segment → `false`
    pub(crate) fn is_package_backed(&self, canonical_id: &str) -> bool {
        let path = crate::canonical_path::CanonicalPath::new(canonical_id);
        if !path.as_str().contains(NODE_MODULES_SEGMENT) {
            return false;
        }
        !self.is_workspace_owned(canonical_id)
    }

    /// Compute the preferred alias-based import specifier for a target file.
    pub(crate) fn preferred_specifier(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        evidence: crate::resolution_currency::ResolutionEvidenceSource<'_>,
        importer_id: &str,
        target_id: &str,
    ) -> Option<String> {
        let root = self.published_state.load_full()?;
        let candidates = root
            .snapshot
            .resolver
            .preferred_specifier_candidates(importer_id, target_id)?;
        let normalized_target = verter_semantic::resolver_core::normalize_canonical_id(target_id);
        let context = verter_semantic::resolver_core::ResolutionContext {
            phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
            kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
        };
        let mut best: Option<String> = None;
        for candidate in candidates {
            let outcome = self.resolve_import_outcome_with_evidence(
                reader,
                evidence,
                importer_id,
                &candidate,
                context,
            );
            if outcome.into_transient_result().is_some_and(|result| {
                verter_semantic::resolver_core::normalize_canonical_id(&result.source_id)
                    == normalized_target
            }) {
                match &best {
                    Some(current) if current.len() <= candidate.len() => {}
                    _ => best = Some(candidate),
                }
            }
        }
        best
    }

    /// Resolve one parser-owned edge against an already captured world.
    ///
    /// This path deliberately skips exact resolutions and the workspace lazy
    /// cache (R5), but it does not bypass the sealed transaction: context
    /// selection, the complete query, filesystem observations, and result
    /// projection all flow through `ResolutionTransaction` and
    /// `TransactionReader`.
    fn resolve_parsed_edge_in_world(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        specifier: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
        captured_world: &Arc<CapturedResolutionWorld>,
        input_ledger: &mut crate::resolver::InputResolutionLedger,
    ) -> ResolutionOutcome {
        let population = captured_world.population;
        let transaction = Mutex::new(ResolutionTransaction::new(Arc::clone(captured_world)));
        let selected_context =
            match selected_context_for_path(captured_world.base.as_ref(), importer_id) {
                Ok(context) => Some(context),
                Err(_) => {
                    transaction.lock().mark_incomplete_provenance();
                    None
                }
            };
        {
            let mut transaction = transaction.lock();
            transaction.observe(ResolutionFactKey::context_importer(importer_id, population));
        }
        let tracked = TransactionReader::new(reader, &transaction);
        let capability = TrackedResolutionCapability::new();
        let driven = captured_world
            .base
            .published
            .as_ref()
            .map_or(Ok(None), |root| {
                let request = verter_semantic::resolver_core::ResolveRequest {
                    importer_id: importer_id.to_string(),
                    specifier: specifier.to_string(),
                    kind: ctx.kind,
                    phase: ctx.phase,
                };
                crate::resolver::resolve_tracked(
                    &root.snapshot.resolver,
                    &capability,
                    &tracked,
                    input_ledger,
                    &request,
                )
            });
        let result = match driven {
            Ok(result) => result,
            Err(_) => {
                transaction.lock().mark_incomplete_provenance();
                if !reader.resolution_event_bridge_complete() {
                    transaction.lock().mark_untracked_backend();
                }
                return ResolutionOutcome::new(
                    None,
                    transaction.into_inner().finish(),
                    Vec::new(),
                    true,
                    false,
                    false,
                );
            }
        };
        let terminal_admission = transaction.lock().input_resolution_terminal_admission();
        if let Some(admission) = terminal_admission {
            return ResolutionOutcome::new(result, admission, Vec::new(), true, false, false);
        }
        let complete_context = Self::complete_provider_context(
            captured_world.base.as_ref(),
            selected_context,
            result.as_ref(),
            population,
            &transaction,
        );
        if let Some(complete_context) = complete_context {
            transaction.lock().set_query(ResolutionQueryKey::importer(
                importer_id,
                specifier,
                ctx,
                complete_context,
                population,
            ));
        }
        if !reader.resolution_event_bridge_complete() {
            transaction.lock().mark_untracked_backend();
        }
        let mut admission = transaction.into_inner().finish();
        if matches!(
            &admission,
            SignatureAdmission::Cacheable(signature)
                if !signature.validates(captured_world.as_ref())
        ) {
            admission = SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionWorldChanged,
            );
        }
        ResolutionOutcome::new(result, admission, Vec::new(), true, false, false)
    }

    fn admit_provided_parsed_target_in_world(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        importer_id: &str,
        specifier: &str,
        target_id: &str,
        ctx: verter_semantic::resolver_core::ResolutionContext,
        captured_world: &Arc<CapturedResolutionWorld>,
    ) -> ResolutionOutcome {
        let population = captured_world.population;
        let transaction = Mutex::new(ResolutionTransaction::new(Arc::clone(captured_world)));
        let selected_context =
            match selected_context_for_path(captured_world.base.as_ref(), importer_id) {
                Ok(context) => Some(context),
                Err(_) => {
                    transaction.lock().mark_incomplete_provenance();
                    None
                }
            };
        transaction
            .lock()
            .observe(ResolutionFactKey::context_importer(importer_id, population));
        let tracked = TransactionReader::new(reader, &transaction);
        let _ = crate::traits::WorkspaceRead::probe_path(&tracked, target_id);
        let _ = crate::traits::WorkspaceRead::realpath(&tracked, target_id);
        let result = captured_world
            .base
            .published
            .as_ref()
            .map(|root| {
                root.snapshot.resolver.project_exact_result(
                    importer_id,
                    specifier,
                    target_id.to_owned(),
                    ctx,
                )
            })
            .or_else(|| {
                transaction.lock().mark_incomplete_provenance();
                Some(ResolveResult {
                    source_id: target_id.to_owned(),
                    provider_id: target_id.to_owned(),
                    provider_specifier: specifier.to_owned(),
                    provider_target: verter_semantic::resolver_core::ProviderTarget::SourceFile,
                    resolution_kind: verter_semantic::resolver_core::ResolutionKind::Bundler,
                    owner_tsconfig_path: None,
                })
            });
        let complete_context = Self::complete_provider_context(
            captured_world.base.as_ref(),
            selected_context,
            result.as_ref(),
            population,
            &transaction,
        );
        if let Some(complete_context) = complete_context {
            transaction.lock().set_query(ResolutionQueryKey::importer(
                importer_id,
                specifier,
                ctx,
                complete_context,
                population,
            ));
        }
        if !reader.resolution_event_bridge_complete() {
            transaction.lock().mark_untracked_backend();
        }
        let mut admission = transaction.into_inner().finish();
        if matches!(
            &admission,
            SignatureAdmission::Cacheable(signature)
                if !signature.validates(captured_world.as_ref())
        ) {
            admission = SignatureAdmission::NonCacheable(
                verter_audit::NonAdmissionReason::ResolutionWorldChanged,
            );
        }
        ResolutionOutcome::new(result, admission, Vec::new(), true, false, false)
    }

    /// Perform exactly the resolver observations the parsed-edge recorders
    /// make, publishing nothing.
    ///
    /// A backend whose live reader is not resolution-event-bridge complete
    /// cannot admit any parsed-edge resolution through it, so it must first
    /// capture an immutable evidence snapshot and rerun the recording against
    /// that. This is the discovery half of that protocol: it resolves the
    /// batch once so the caller's recorder observes every filesystem read the
    /// admitted replay will need, and deliberately discards the product.
    pub(crate) fn observe_parsed_edge_evidence_in_operation(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        input_ledger: &mut crate::resolver::InputResolutionLedger,
    ) {
        let population = reader.resolution_population();
        if let Some(captured) = self.capture_resolution_world(population) {
            let _ = self.resolve_parsed_edge_inputs_in_world(
                reader,
                canonical_id,
                edges,
                &captured.world,
                input_ledger,
            );
        }
    }

    /// Record parsed edges, eagerly resolving relative/src edges via the
    /// parsed-edge resolver (R5 bypasses `exact_resolutions`). The entire
    /// batch resolves against one captured world and commits only if that
    /// exact world is still current under the publication gate.
    pub(crate) fn record_parsed_edges(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
    ) {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        let _ = self.record_parsed_edges_in_operation(
            reader,
            canonical_id,
            edges,
            &mut input_ledger,
            &|| true,
        );
    }

    pub(crate) fn record_parsed_edges_in_operation(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        input_ledger: &mut crate::resolver::InputResolutionLedger,
        final_validate: &dyn Fn() -> bool,
    ) -> bool {
        crate::probe_scope!(RECORD_PARSED_EDGES);
        loop {
            let population = reader.resolution_population();
            let Some(captured) = self.capture_stable_resolution_world(population) else {
                if input_ledger.charge_outer_restart(reader).is_err() {
                    return false;
                }
                continue;
            };
            let Ok(inputs) = self.resolve_parsed_edge_inputs_in_world(
                reader,
                canonical_id,
                edges,
                &captured.world,
                input_ledger,
            ) else {
                return false;
            };

            #[cfg(test)]
            resolution_test_hooks::fire(
                resolution_test_hooks::ResolutionPhase::ParsedEdgePreCommit,
            );

            // Per R4 lifecycle: replace_parsed_edges CLEARS exact_resolved +
            // exact_resolutions + lazy_resolved + semantic_transitive.
            // ambient_resolved survives. Bundler must re-call
            // set_import_dependencies after every upsert.
            let committed = self.mutate_resolution_world_if_current(&captured, |world| {
                if !final_validate() {
                    return (false, false);
                }
                let retained = {
                    let mut edges = self.edges.write();
                    edges.replace_parsed_edges(
                        canonical_id,
                        inputs.parsed_resolved,
                        inputs.unresolved_pairs,
                        inputs.bare_specifiers,
                    );
                    edges.exact_resolutions_for_owner(canonical_id)
                };
                let changed = self.replace_world_exact_resolutions(world, canonical_id, &retained);
                input_ledger.commit_loaded_inputs(reader);
                (true, changed)
            });
            if let Ok(committed) = committed {
                return committed;
            }
            if input_ledger.charge_outer_restart(reader).is_err() {
                return false;
            }
        }
    }

    /// Atomic variant: record parsed edges AND re-apply bundler exact
    /// resolutions under ONE edge-store write lock, so no concurrent
    /// resolver can observe the intermediate parsed-recorded /
    /// exacts-cleared state. Resolution pre-work runs outside the lock
    /// (same as `record_parsed_edges`); only the two store mutations are
    /// fused into the critical section.
    pub(crate) fn record_parsed_edges_with_exact_resolutions(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        resolutions: Vec<crate::types::ExactResolution>,
    ) -> crate::types::ExactResolutionResult {
        let mut input_ledger =
            crate::resolver::InputResolutionLedger::new(self.input_resolution_budgets);
        self.record_parsed_edges_with_exact_resolutions_in_operation(
            reader,
            canonical_id,
            edges,
            resolutions,
            &mut input_ledger,
            &|| true,
        )
        .unwrap_or_default()
    }

    pub(crate) fn record_parsed_edges_with_exact_resolutions_in_operation(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        resolutions: Vec<crate::types::ExactResolution>,
        input_ledger: &mut crate::resolver::InputResolutionLedger,
        final_validate: &dyn Fn() -> bool,
    ) -> Option<crate::types::ExactResolutionResult> {
        loop {
            let population = reader.resolution_population();
            let Some(captured) = self.capture_stable_resolution_world(population) else {
                if input_ledger.charge_outer_restart(reader).is_err() {
                    return None;
                }
                continue;
            };
            let Ok(inputs) = self.resolve_parsed_edge_inputs_in_world(
                reader,
                canonical_id,
                edges,
                &captured.world,
                input_ledger,
            ) else {
                return None;
            };

            #[cfg(test)]
            resolution_test_hooks::fire(
                resolution_test_hooks::ResolutionPhase::ParsedEdgePreCommit,
            );
            let attempt_resolutions = resolutions.clone();
            let committed = self.mutate_resolution_world_if_current(&captured, |world| {
                if !final_validate() {
                    return (None, false);
                }
                let mut edge_store = self.edges.write();
                edge_store.replace_parsed_edges(
                    canonical_id,
                    inputs.parsed_resolved,
                    inputs.unresolved_pairs,
                    inputs.bare_specifiers,
                );
                let result =
                    edge_store.replace_exact_resolutions(canonical_id, attempt_resolutions);
                let retained = edge_store.exact_resolutions_for_owner(canonical_id);
                let changed = self.replace_world_exact_resolutions(world, canonical_id, &retained);
                drop(edge_store);
                input_ledger.commit_loaded_inputs(reader);
                (Some(result), changed)
            });
            if let Ok(result) = committed {
                return result;
            }
            if input_ledger.charge_outer_restart(reader).is_err() {
                return None;
            }
        }
    }

    /// Shared pre-lock resolution work for the parsed-edge recorders:
    /// eagerly resolves `Relative` / `ExternalSrc` edges via the
    /// parsed-edge resolver (R5 bypasses `exact_resolutions`) and
    /// classifies the rest.
    fn resolve_parsed_edge_inputs_in_world(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        canonical_id: &str,
        edges: &[crate::types::ParsedEdge],
        captured_world: &Arc<CapturedResolutionWorld>,
        input_ledger: &mut crate::resolver::InputResolutionLedger,
    ) -> Result<ParsedEdgeInputs, crate::resolution_currency::ResolutionPublicationRefusal> {
        let mut parsed_resolved: BTreeSet<String> = BTreeSet::new();
        let mut bare_specifiers: Vec<(String, ResolveRequestKind)> = Vec::new();
        let mut unresolved_pairs: Vec<((String, ResolveRequestKind), String)> = Vec::new();

        for edge in edges {
            match edge {
                crate::types::ParsedEdge::Relative { specifier, kind } => {
                    let ctx = verter_semantic::resolver_core::ResolutionContext {
                        phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                        kind: *kind,
                    };
                    let outcome = self.resolve_parsed_edge_in_world(
                        reader,
                        canonical_id,
                        specifier,
                        ctx,
                        captured_world,
                        input_ledger,
                    );
                    match outcome.into_publication() {
                        crate::ResolutionPublication::Admitted(admitted)
                            if admitted.result().is_some() =>
                        {
                            let result =
                                admitted.into_result().expect("guarded admitted resolution");
                            parsed_resolved.insert(result.source_id);
                        }
                        crate::ResolutionPublication::Admitted(_) if specifier.starts_with('.') => {
                            let normalized =
                                crate::relative_path::normalize_relative_specifier(specifier);
                            let stem =
                                crate::relative_path::join_relative(canonical_id, &normalized);
                            unresolved_pairs.push(((normalized, *kind), stem));
                        }
                        crate::ResolutionPublication::Admitted(_) => {}
                        crate::ResolutionPublication::Refused(refusal) => return Err(refusal),
                    }
                }
                crate::types::ParsedEdge::ExternalSrc {
                    specifier,
                    resolved_path,
                } => {
                    if let Some(path) = resolved_path {
                        let outcome = self.admit_provided_parsed_target_in_world(
                            reader,
                            canonical_id,
                            specifier,
                            path,
                            verter_semantic::resolver_core::ResolutionContext {
                                phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                                kind:
                                    verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                            },
                            captured_world,
                        );
                        match outcome.into_publication() {
                            crate::ResolutionPublication::Admitted(admitted) => {
                                let Some(result) = admitted.into_result() else {
                                    continue;
                                };
                                parsed_resolved.insert(result.source_id);
                            }
                            crate::ResolutionPublication::Refused(refusal) => {
                                return Err(refusal);
                            }
                        }
                    } else {
                        let ctx = verter_semantic::resolver_core::ResolutionContext {
                            phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                            kind: verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                        };
                        let outcome = self.resolve_parsed_edge_in_world(
                            reader,
                            canonical_id,
                            specifier,
                            ctx,
                            captured_world,
                            input_ledger,
                        );
                        match outcome.into_publication() {
                            crate::ResolutionPublication::Admitted(admitted) => {
                                let Some(result) = admitted.into_result() else {
                                    continue;
                                };
                                parsed_resolved.insert(result.source_id);
                            }
                            crate::ResolutionPublication::Refused(refusal) => {
                                return Err(refusal);
                            }
                        }
                    }
                }
                crate::types::ParsedEdge::Bare { specifier, kind } => {
                    bare_specifiers.push((specifier.clone(), *kind));
                }
            }
        }

        Ok(ParsedEdgeInputs {
            parsed_resolved,
            unresolved_pairs,
            bare_specifiers,
        })
    }

    /// Query reverse deps (files that import this file). Strips the
    /// configured longest-suffix-first extension list and consults BOTH
    /// the canonical and stem reverse axes.
    pub(crate) fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        // Lock-free read of the configured extension list (already sorted
        // longest-first at set-time).
        let exts = self.default_resolve_extensions.load();
        let stripped = crate::relative_path::strip_extension_first(canonical_id, &exts);
        self.edges
            .read()
            .reverse_deps_for_target(canonical_id, stripped)
    }

    /// Query forward deps (files this file imports).
    pub(crate) fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.edges.read().forward_deps(canonical_id)
    }

    /// Enumerate every canonical the workspace currently holds content for:
    /// open/upserted overlay buffers UNIONED with injected/published snapshot
    /// content. These are the program members an ambient `declare module`
    /// declarer can live in (program-completeness for external module
    /// augmentation, where the declarer may be a root `.d.ts` that nothing
    /// imports). Deduplicated; order is unspecified.
    pub(crate) fn known_canonicals(&self) -> Vec<String> {
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        seen.extend(self.overlay.read().ids().map(str::to_owned));
        seen.extend(self.snapshot.read().ids().map(str::to_owned));
        seen.into_iter().collect()
    }

    // ── Ambient lib registration ──

    /// Register an ambient lib via the CAS loop (`ambient_lib::cas_register`).
    ///
    /// Resolves `spec.project_id` against the published snapshot to compute a
    /// `ProjectStableKey`. Honors A5 user-wins shadowing by querying
    /// `WorkspaceAccess::file_exists` for non-ambient collisions. Bumps
    /// `content_generation` on actual content change so dep validators
    /// invalidate downstream caches.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn register_ambient_lib(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        spec: crate::ambient_lib::AmbientLibSpec,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        use crate::ambient_lib::{
            cas_register, compute_ambient_hash16, normalize_canonical_id, AmbientLibError,
        };

        let published = self.load_published().ok_or(AmbientLibError::NotPublished)?;
        if published.snapshot.projects.is_empty() {
            return Err(AmbientLibError::NotPublished);
        }
        let stable_key = match spec.project_id {
            Some(pid) => published
                .snapshot
                .projects
                .iter()
                .find(|p| p.id == pid)
                .map(|p| crate::project_key::project_stable_key_from_project(p, &p.workspace_root))
                .ok_or(AmbientLibError::UnknownOrAmbiguousProject)?,
            None if published.snapshot.projects.len() == 1 => {
                let p = &published.snapshot.projects[0];
                crate::project_key::project_stable_key_from_project(p, &p.workspace_root)
            }
            None => return Err(AmbientLibError::UnknownOrAmbiguousProject),
        };

        let canonical = normalize_canonical_id(&spec.canonical_id);

        // A5: shadowing check — a real user file at this canonical_id wins.
        if reader.file_exists(canonical.as_ref()) {
            return Err(AmbientLibError::NonAmbientCollision(canonical));
        }

        // A6 eager step: cheap shallow parse for top-level export names.
        let top_level_exports: Arc<[Arc<str>]> = {
            let names = crate::ambient_parse::parse_top_level_exports(
                canonical.as_ref(),
                spec.source.as_ref(),
            )
            .map_err(AmbientLibError::ParseFailure)?;
            names.into_boxed_slice().into()
        };

        let content_hash = compute_ambient_hash16(spec.source.as_bytes());
        let changed = cas_register(
            &self.ambient_libs,
            stable_key,
            canonical.clone(),
            Arc::clone(&spec.source),
            content_hash,
            top_level_exports,
        );
        if changed {
            // An ambient-lib (re)registration changes the content served
            // for that canonical — a per-canonical content transition,
            // recorded so retained artifacts built from the previous
            // registration stop validating as fresh.
            self.bump_content_generation_for(&canonical);
        }
        Ok(())
    }

    /// Unregister an ambient lib by `(stable_key, canonical_id)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn unregister_ambient_lib(
        &self,
        stable_key: verter_semantic::resolver_core::ProjectStableKey,
        canonical_id: &str,
    ) -> Result<(), crate::ambient_lib::AmbientLibError> {
        use crate::ambient_lib::{cas_unregister, normalize_canonical_id};

        let canonical = normalize_canonical_id(canonical_id);
        let removed = cas_unregister(&self.ambient_libs, stable_key, canonical.clone());
        if removed {
            // Unregistration removes the content served for the ambient
            // canonical — a per-canonical content transition, same as
            // registration above.
            self.bump_content_generation_for(&canonical);
        }
        Ok(())
    }

    /// Read an ambient lib's source. A5: returns `None` when a non-ambient
    /// user file exists at the canonical_id (shadowing).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_ambient_lib(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        stable_key: verter_semantic::resolver_core::ProjectStableKey,
        canonical_id: &str,
    ) -> Option<Arc<str>> {
        let canonical = crate::ambient_lib::normalize_canonical_id(canonical_id);
        if reader.file_exists(canonical.as_ref()) {
            return None;
        }
        let ambient = self.ambient_libs.load_full();
        ambient
            .by_project
            .get(&stable_key)?
            .libs
            .get(canonical.as_ref())
            .map(|entry| Arc::clone(&entry.source))
    }

    /// O(1) symbol → `(stable_key, canonical, lib_order)` lookup against the
    /// project's registered ambient libs (A2). Returns the first lib (by
    /// `lib_order`) that exposes the symbol.
    pub(crate) fn lookup_ambient_symbol(
        &self,
        consumer_project: verter_semantic::resolver_core::ProjectStableKey,
        symbol: &str,
    ) -> Option<verter_semantic::resolver_core::AmbientSymbolHit> {
        let ambient = self.ambient_libs.load_full();
        let p = ambient.by_project.get(&consumer_project)?;
        let candidates = p.symbol_index.get(symbol)?;
        let (canonical_id, lib_order) = candidates.first()?.clone();
        let virtual_id = crate::ambient_lib::ambient_virtual_canonical_id(
            consumer_project,
            canonical_id.as_ref(),
        );
        Some(verter_semantic::resolver_core::AmbientSymbolHit {
            project: consumer_project,
            canonical_id,
            virtual_id,
            lib_order,
        })
    }

    /// Resolve a `ProjectId` to its stable key against the published snapshot.
    pub(crate) fn project_stable_key(
        &self,
        project_id: crate::workspace_snapshot::ProjectId,
    ) -> Option<verter_semantic::resolver_core::ProjectStableKey> {
        let published = self.load_published()?;
        published
            .snapshot
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| crate::project_key::project_stable_key_from_project(p, &p.workspace_root))
    }

    /// Lock-free read of the ambient lib registry — used by validators.
    pub(crate) fn ambient_libs_view(&self) -> Arc<crate::ambient_lib::AmbientLibsByProject> {
        self.ambient_libs.load_full()
    }
}

// Debug implementation that doesn't require Debug on RwLock contents
impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

// ── Env-hash composition for published snapshots (project-scoped env-hash API) ──

/// Stable parser-flag identifiers mixed into every project's
/// `parse_env_hash` regardless of tsconfig. Today the parser feature
/// surface is a single fixed identifier; new flags extend this slice in
/// declaration order without breaking determinism.
// The rune-ambient prelude version is a parse-env input: a Svelte rune module
// (`.svelte.ts`/`.svelte.js`) merges the module-valid runes into its eval env
// (Channel A), so a prelude-surface change must invalidate that module's stale
// inferred exports. Folding the version into `parser_flags` carries it into
// `parse_env_hash` (the dimension the eval-env cache key uses). The literal is
// kept in lockstep with `RUNE_AMBIENT_PRELUDE_VERSION` by a freshness guard.
const WORKSPACE_PARSER_FLAGS: &[&str] = &["verter-parser-v1", SVELTE_RUNE_AMBIENT_PARSER_FLAG];

/// The Svelte rune-ambient parse-env flag mixed into `parse_env_hash`. Its
/// version suffix MUST track `verter_compiler`'s `RUNE_AMBIENT_PRELUDE_VERSION`
/// so a rune-prelude surface change invalidates a rune module's stale inferred
/// exports. A `verter_session` freshness guard pins the two in lockstep (the
/// version constant lives in `verter_compiler`, which `verter_workspace` does
/// not depend on, so the link is asserted from the crate that sees both).
pub const SVELTE_RUNE_AMBIENT_PARSER_FLAG: &str = "svelte-rune-ambient-v1";

/// Workspace-level ambient corpus fingerprint mixed into every project's
/// `lib_env_hash`. Replaced with the real ambient-registry fingerprint
/// once the substrate carries one; for now the constant ensures the
/// composed lib hash is deterministic and distinguishable from
/// "no lib data".
const WORKSPACE_AMBIENT_FINGERPRINT: u64 = 0xC0DE_BABE_0000_0001;

/// Workspace-default `exports`/`imports` condition set mixed into every
/// project's `resolve_env_hash`. This is the resolve-domain default the
/// composer feeds until the published project payload carries the project's
/// own `moduleResolution` / conditions (the full per-project resolution
/// matrix is a `verter_session::resolver_core` concern — see
/// `.claude/skills/type-resolution/SKILL.md`). The default mirrors the
/// TS provider-graph condition order so the composed `resolve_env_hash` is
/// deterministic.
fn workspace_default_export_conditions() -> ConditionSet {
    ConditionSet::new(["types", "import", "default"])
}

/// Compose per-project `[parse, resolve, type_, lib]` env-hash arrays
/// and project-identity hashes from the published `OwnershipProject`s.
///
/// Producer-side composition: reads `compiler_options` directly from each
/// `Configured` project payload; for `Fallback` projects, uses
/// `crate::resolver::ide_project_config(root, workspace_root, None)` so the resulting
/// `project_identity` distinguishes fallback identities (a fallback
/// project at root `/A` is not the same identity as a configured project
/// at root `/A`).
///
/// The `resolve_extensions` slice flows through `EnvHashInputs` so
/// extension-priority changes invalidate every project's
/// `resolve_env_hash` (single producer site — engine reads
/// `default_resolve_extensions` once before iterating projects).
pub(crate) fn compose_env_hash_tables(
    projects: &[OwnershipProject],
    resolve_extensions: &[String],
) -> (
    FxHashMap<ProjectId, ProjectEnvHashArray>,
    FxHashMap<ProjectId, Hash16>,
) {
    let extensions_refs: Vec<&str> = resolve_extensions.iter().map(String::as_str).collect();
    let export_conditions = workspace_default_export_conditions();
    let inputs = EnvHashInputs {
        parser_flags: WORKSPACE_PARSER_FLAGS,
        resolve_extensions: &extensions_refs,
        type_strict: false,
        type_no_implicit_any: false,
        lib_names: &[],
        type_roots: &[],
        module_resolution_mode: ModuleResolutionMode::default(),
        export_conditions: &export_conditions,
        ambient_corpus_fingerprint: WORKSPACE_AMBIENT_FINGERPRINT,
    };

    let mut env_hashes_by_project: FxHashMap<ProjectId, ProjectEnvHashArray> = FxHashMap::default();
    env_hashes_by_project.reserve(projects.len());
    let mut project_identity_hashes: FxHashMap<ProjectId, Hash16> = FxHashMap::default();
    project_identity_hashes.reserve(projects.len());

    for project in projects {
        let config = ide_project_config_from_ownership(project);
        let arr: ProjectEnvHashArray = [
            config.parse_env_hash(&inputs),
            config.resolve_env_hash(&inputs),
            config.type_env_hash(&inputs),
            config.lib_env_hash(&inputs),
        ];
        env_hashes_by_project.insert(project.id, arr);
        project_identity_hashes.insert(project.id, config.project_identity());
    }

    (env_hashes_by_project, project_identity_hashes)
}

fn compose_env_hash_tables_from_configs(
    projects: &[OwnershipProject],
    configs: &[IdeProjectConfig],
    resolve_extensions: &[String],
) -> (
    FxHashMap<ProjectId, ProjectEnvHashArray>,
    FxHashMap<ProjectId, Hash16>,
) {
    let extension_refs: Vec<&str> = resolve_extensions.iter().map(String::as_str).collect();
    let export_conditions = workspace_default_export_conditions();
    let inputs = EnvHashInputs {
        parser_flags: WORKSPACE_PARSER_FLAGS,
        resolve_extensions: &extension_refs,
        type_strict: false,
        type_no_implicit_any: false,
        lib_names: &[],
        type_roots: &[],
        module_resolution_mode: ModuleResolutionMode::default(),
        export_conditions: &export_conditions,
        ambient_corpus_fingerprint: WORKSPACE_AMBIENT_FINGERPRINT,
    };
    let mut env_hashes = FxHashMap::default();
    let mut identities = FxHashMap::default();
    for project in projects {
        let expected_tsconfig = match &project.payload {
            ProjectPayload::Configured { tsconfig_path, .. } => Some(tsconfig_path.as_str()),
            ProjectPayload::Fallback { .. } => None,
        };
        let selected = configs.iter().find(|config| {
            verter_semantic::resolver_core::normalize_canonical_id(&config.root)
                == project.root.as_str()
                && verter_semantic::resolver_core::normalize_canonical_id(&config.workspace_root)
                    == project.workspace_root.as_str()
                && config
                    .tsconfig_path
                    .as_deref()
                    .map(verter_semantic::resolver_core::normalize_canonical_id)
                    .as_deref()
                    == expected_tsconfig
        });
        let fallback;
        let config = if let Some(config) = selected {
            config
        } else {
            fallback = ide_project_config_from_ownership(project);
            &fallback
        };
        env_hashes.insert(
            project.id,
            [
                config.parse_env_hash(&inputs),
                config.resolve_env_hash(&inputs),
                config.type_env_hash(&inputs),
                config.lib_env_hash(&inputs),
            ],
        );
        identities.insert(project.id, config.project_identity());
    }
    (env_hashes, identities)
}

/// Project the `IdeProjectConfig` shape from an `OwnershipProject` for
/// env-hash composition. Configured projects carry their own
/// compiler_options / references / workspace_aliases / membership;
/// fallback projects produce an empty config rooted at their own root /
/// workspace root.
fn ide_project_config_from_ownership(project: &OwnershipProject) -> IdeProjectConfig {
    match &project.payload {
        ProjectPayload::Configured {
            tsconfig_path,
            compiler_options,
            references,
            workspace_aliases,
            membership,
        } => {
            let mut config = crate::resolver::ide_project_config(
                project.root.as_str().to_string(),
                project.workspace_root.as_str().to_string(),
                Some(tsconfig_path.as_str().to_string()),
            );
            config.compiler_options = compiler_options.clone();
            config.references = references.iter().map(|r| r.as_str().to_string()).collect();
            config.workspace_aliases = workspace_aliases.clone();
            config.membership = membership.clone();
            config
        }
        ProjectPayload::Fallback { .. } => crate::resolver::ide_project_config(
            project.root.as_str().to_string(),
            project.workspace_root.as_str().to_string(),
            None,
        ),
    }
}

/// Workspace-default env-hash array used when a canonical has no owning
/// project. Composed from the engine's resolve-extension list mixed with
/// a stable "no project" identity so the default is non-zero and changes
/// when the workspace-level extension list changes.
///
/// The array is a pure function of `default_resolve_extensions` (every
/// other input is a workspace constant), and session callers read it on
/// EVERY store-view build (`host_view_env_hashes` plus the no-owner
/// fallback of `host_view_env_hashes_for`), so the engine caches the
/// derived array in [`Engine::workspace_default_env_hashes`]. Validity is
/// pointer identity against the live extensions `Arc`: the ONLY mutation
/// point, [`Engine::set_default_resolve_extensions`], swaps that `Arc`,
/// which invalidates the cache implicitly — no separate hook, no window
/// where a reader can pair the new extension list with stale hashes (a
/// read always returns hashes derived from the extensions `Arc` it loaded
/// at entry, exactly the uncached semantics).
pub(crate) fn workspace_default_env_hash_array_for_engine(engine: &Engine) -> ProjectEnvHashArray {
    let extensions = engine.default_resolve_extensions.load_full();
    if let Some(cached) = engine.workspace_default_env_hashes.load_full() {
        if Arc::ptr_eq(&cached.extensions, &extensions) {
            return cached.hashes;
        }
    }
    let hashes = compute_workspace_default_env_hash_array(&extensions);
    engine
        .workspace_default_env_hashes
        .store(Some(Arc::new(WorkspaceDefaultEnvHashes {
            extensions,
            hashes,
        })));
    hashes
}

/// Cached workspace-default env-hash array plus the exact extensions
/// `Arc` it was derived from. Concurrent racers may transiently store an
/// entry for a superseded extensions `Arc` (last-wins); the next read's
/// pointer check self-heals with one recompute — returned values are
/// always consistent with the reader's own loaded extension list.
struct WorkspaceDefaultEnvHashes {
    extensions: Arc<Vec<String>>,
    hashes: ProjectEnvHashArray,
}

/// Uncached derivation of the workspace-default env-hash array from a
/// merged extension list. Single compute site backing the cached read
/// path above (and the cache-parity tests).
fn compute_workspace_default_env_hash_array(extensions: &[String]) -> ProjectEnvHashArray {
    let extensions_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let export_conditions = workspace_default_export_conditions();
    let inputs = EnvHashInputs {
        parser_flags: WORKSPACE_PARSER_FLAGS,
        resolve_extensions: &extensions_refs,
        type_strict: false,
        type_no_implicit_any: false,
        lib_names: &[],
        type_roots: &[],
        module_resolution_mode: ModuleResolutionMode::default(),
        export_conditions: &export_conditions,
        ambient_corpus_fingerprint: WORKSPACE_AMBIENT_FINGERPRINT,
    };
    let config = crate::resolver::ide_project_config(String::new(), String::new(), None);
    [
        config.parse_env_hash(&inputs),
        config.resolve_env_hash(&inputs),
        config.type_env_hash(&inputs),
        config.lib_env_hash(&inputs),
    ]
}

/// Workspace-default project-identity hash for canonicals with no owning
/// project. See [`workspace_default_env_hash_array_for_engine`] for the
/// rationale on producing a deterministic non-zero default that depends
/// on workspace configuration rather than collapsing to all-zero.
///
/// The value is a process-wide constant (an empty `IdeProjectConfig` has
/// no engine-dependent input), computed once via `OnceLock` — session
/// callers read it on every store-view build (`host_view_project_identity`).
pub(crate) fn workspace_default_project_identity_hash_for_engine(_engine: &Engine) -> Hash16 {
    // Default project identity carries an empty (workspace_root, root,
    // tsconfig) tuple — distinguishes "no owning project" from any
    // published project (which always has a non-empty workspace_root /
    // root) without colliding across workspaces.
    static DEFAULT_PROJECT_IDENTITY: OnceLock<Hash16> = OnceLock::new();
    *DEFAULT_PROJECT_IDENTITY.get_or_init(|| {
        crate::resolver::ide_project_config(String::new(), String::new(), None).project_identity()
    })
}

#[cfg(test)]
#[path = "resolution_candidate_slot_tests.rs"]
mod resolution_candidate_slot_tests;
#[cfg(test)]
#[path = "resolution_concurrency_contract_tests.rs"]
mod resolution_concurrency_contract_tests;
#[cfg(test)]
#[path = "resolution_currency_contract_tests.rs"]
mod resolution_currency_contract_tests;
#[cfg(test)]
#[path = "resolution_test_hooks.rs"]
pub(crate) mod resolution_test_hooks;
#[cfg(test)]
#[path = "resolution_world_capture_tests.rs"]
mod resolution_world_capture_tests;
#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
