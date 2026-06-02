//! Driver-owned scheduling DAG.
//!
//! `SchedulerDag` is the SOLE readiness authority for the scheduler. It
//! holds work-node identities, their work kind, dependency edges, and
//! the priority data needed to dispatch ready work. All admission,
//! dedup, generation supersession, dependency gating, blocker
//! resolution, and capacity accounting flow through this single
//! authority.
//!
//! # Typed identity
//!
//! Each work node carries a [`WorkNodeIdentity`] — a typed sum with
//! exactly three variants. The variants are disjoint: a file-stage
//! node never collapses with an artifact node, an artifact node never
//! collapses with a cache-node entry. Illegal states (an artifact
//! with no profile, a cache node with no cache id, a file-stage with
//! a content hash) are not representable.
//!
//! # Capacity reservation
//!
//! [`DagCapacityReservation`] is the single accounting source for
//! admission permits. Permits are released exactly once: either on
//! the normal completion path (when a node is removed via
//! [`SchedulerDag::complete`] or [`SchedulerDag::cancel`]) or on
//! `Drop` if the caller forgets. The two paths are mutually
//! exclusive — there is no double-release semantics.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::cache_id::SchedulerCacheId;
use crate::job::{CompletionSender, CompletionState, RequestResult, SchedulerError};
use crate::stage::{Priority, TargetStage, TaskKind};

/// Artifact blocker-dep registry typed API. The storage stays on
/// [`SchedulerDag`] (see `artifact_blocker_deps`); the child module
/// owns the `record` / `drain` / `peek` / `clear` /
/// `scrub_referencing` / `remove_owner` impls.
mod blocker_registry;

/// Terminal-failure store + dependency-failure fan-out — owns the
/// `terminal_dep_failures` persistent store, the per-stage fan-out
/// helpers, the [`FailedDepRecord`] type, and the
/// `attach_failed_dep` admission helper. Storage stays on
/// [`SchedulerDag`] (see `terminal_dep_failures`); the child module
/// wraps the typed API + the waiter-graph fan-out. Re-export the
/// type at the parent module's path so existing
/// `crate::dag::FailedDepRecord` callers keep compiling without
/// the child-module split being visible.
mod terminal_failures;
pub use terminal_failures::FailedDepRecord;

/// Capacity-budget types: aging configuration, split CPU/IO budget,
/// per-`WorkKind` resource class, and the typed
/// [`DagCapacityReservation`]. Re-exported at this module's level
/// so existing `crate::dag::DagCapacityBudget` etc. paths keep
/// working without the submodule split being visible to callers.
mod capacity;
pub use capacity::{DagAgingConfig, DagCapacityBudget, DagCapacityReservation, ResourceClass};

/// Combined late-blocker registry entry for an
/// `(owner_canonical, owner_generation)` slot. The Artifact
/// blocker-dep registry holds both the still-gating
/// [`DepKey`]s and any [`FailedDepRecord`]s for blockers whose
/// producers terminalized BEFORE the owner's Artifact admission —
/// the latter case the prior single-`BTreeSet` storage could not
/// represent. Stored as one entry per slot so the
/// drain → admit cycle in `admit_artifact_with_blockers` operates
/// atomically: a future Artifact admission either sees the entire
/// set (deps + failed) or nothing.
///
/// Empty (no deps AND no failed records) is semantically "no
/// pending blockers" and is stored as a registry-level absence.
#[derive(Clone, Debug, Default)]
pub struct PendingBlockerSet {
    /// Still-gating prerequisite identities the Artifact must wait
    /// on (each will become a [`DepKey`] in
    /// [`SchedulerDag::submit`]'s `deps` arg).
    pub deps: BTreeSet<DepKey>,
    /// Prerequisites whose producers terminalized BEFORE this
    /// Artifact slot was admitted. Each record attaches to the
    /// Artifact via [`SchedulerDag::attach_failed_dep`] so the
    /// pre-dispatch chokepoint in `execute_stage_on_worker`
    /// surfaces a typed `DependencyFailed`.
    pub failed: Vec<FailedDepRecord>,
}

impl PendingBlockerSet {
    /// `true` when the set carries no live gating deps AND no
    /// failed records. The registry treats an empty set as a
    /// remove on `record_artifact_blockers`.
    pub fn is_empty(&self) -> bool {
        self.deps.is_empty() && self.failed.is_empty()
    }

    /// Convenience constructor for the common path where only
    /// gating deps are recorded (no failure side-channel).
    pub fn from_deps(deps: BTreeSet<DepKey>) -> Self {
        Self {
            deps,
            failed: Vec::new(),
        }
    }
}

/// Stage of a file-staged work node.
///
/// Mirrors [`TaskKind`] for non-artifact stages; the artifact stage is
/// uniquely identified by its profile hash via the
/// [`WorkNodeIdentity::Artifact`] variant. Keeping artifact out of
/// `FileStageKey` makes the variant split structural rather than
/// an optional payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileStageKey {
    /// Load + parse stage.
    Source,
    /// Analysis stage.
    Analysis,
}

/// Opaque hash used by artifact and cache-node identities. The
/// scheduler does not interpret the bytes — the typed variant is
/// the discriminator.
pub type Hash16 = [u8; 16];

/// Snapshot pin id for cache-node work. Same opacity contract as
/// [`SchedulerCacheId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PinId(pub u64);

/// Typed identity for a scheduling work node.
///
/// Exactly three variants. Each variant carries the fields needed to
/// identify that kind of work; illegal combinations (e.g. an artifact
/// with no profile, or a cache node with no cache id) are
/// unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WorkNodeIdentity {
    /// File-staged work for a canonical file at a generation, at one
    /// of the [`FileStageKey`] stages.
    FileStage {
        canonical: Arc<str>,
        generation: u64,
        stage: FileStageKey,
    },
    /// Artifact work for a canonical file at a generation, parameterised
    /// by a profile hash and bound to a content hash (lets two compile
    /// requests at the same generation but different content disambiguate).
    Artifact {
        canonical: Arc<str>,
        generation: u64,
        profile_hash: Hash16,
        content_hash: Hash16,
    },
    /// Cache-node work scoped to a session-owned cache id, identified by
    /// a key hash + view epoch + snapshot pin id.
    CacheNode {
        cache_id: SchedulerCacheId,
        key_hash: Hash16,
        view_epoch: u64,
        snapshot_pin_id: PinId,
    },
}

/// Work kind discriminant.
///
/// Final set: five variants. Persistent / content / fact distinctions
/// belong on typed identity keys and [`SchedulerCacheId`], not on this
/// discriminant — splitting them here leaks session semantics into the
/// scheduler crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkKind {
    /// Source content load (I/O-bound).
    Load,
    /// Source parsing (CPU-bound).
    Parse,
    /// Analysis pass (CPU-bound).
    Analysis,
    /// Artifact compilation (CPU-bound).
    Artifact,
    /// Cache-node materialisation (CPU-bound).
    CacheNode,
}

/// Opaque submission token returned by [`SchedulerDag::submit`].
///
/// Tokens are monotonic per dag and remain stable across the lifetime
/// of the corresponding node. A token does not refer to a moving slot;
/// the dag's internal storage is keyed on tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubmissionToken(u64);

impl SubmissionToken {
    /// Bare numeric value, for diagnostic display only.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// BFS instrumentation counters returned by
/// [`SchedulerDag::dep_reaches_owner_with_metrics`].
///
/// `enqueue_count` tracks the number of `frontier.push_back` calls;
/// under enqueue-time visited semantics this MUST equal the number
/// of distinct reachable nodes the BFS sees (each node enters the
/// frontier at most once). `max_frontier_len` tracks the peak
/// `frontier.len()` observed during the walk; it is bounded by the
/// reachable-node count.
///
/// Pop-time visited would push the same node into the frontier
/// through every fan-in edge before the first pop dedups it, so
/// both counters would grow toward O(|edges|) on dense graphs.
/// This struct lets tests assert the O(V) bound directly rather
/// than indirectly via wall-clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BfsMetrics {
    /// Total `frontier.push_back` invocations across the walk.
    pub(crate) enqueue_count: usize,
    /// Peak `frontier.len()` observed across the walk.
    pub(crate) max_frontier_len: usize,
}

/// Request-group bookkeeping co-located on a DAG node.
///
/// A node may serve multiple callers at the same `(generation, target)`
/// — they are stored together in one group so completion fans out to
/// every waiter. Multiple target groups can live on one node: e.g. a
/// file-stage Analysis node can serve both an Analysis-targeted and a
/// Source-targeted request at the same generation.
struct RequestGroup {
    target: TargetStage,
    senders: Vec<CompletionSender<RequestResult>>,
    /// First-arrived caller's optional session-side context. Joiner
    /// callers observe this via [`SchedulerDag::winner_context_for`].
    winner_context: Option<crate::request_context::OpaqueRequestContext>,
}

impl std::fmt::Debug for RequestGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestGroup")
            .field("target", &self.target)
            .field("senders", &self.senders.len())
            .finish()
    }
}

impl RequestGroup {
    fn signal_all(&mut self, state: CompletionState<RequestResult>) {
        for sender in self.senders.drain(..) {
            sender.send(state.clone());
        }
    }
}

/// A deferred dedup-join observation produced by
/// [`SchedulerDag::register_request`] when a request joins an existing
/// waiter group.
///
/// The session-side `on_dedup_joiner` callback may re-enter the
/// scheduler (it records a share-reuse fact and can touch host-owned
/// state), so it must NOT run while admission holds `dag.lock()`.
/// `register_request` returns this event instead of firing the
/// callback; the admission path collects every event and calls
/// [`Self::fire`] AFTER the DAG lock is released. This keeps the
/// callback-under-lock hazard out of both the single-request and the
/// atomic-batch admission paths through one shared mechanism.
pub struct DedupJoinerEvent {
    canonical: Arc<str>,
    joiner_context: crate::request_context::OpaqueRequestContext,
    winner_request_id: u64,
    winner_audited: bool,
}

impl DedupJoinerEvent {
    /// Invoke the joiner's `on_dedup_joiner` callback. MUST be called
    /// after the DAG lock has been released — the callback may re-enter
    /// the scheduler.
    pub fn fire(self) {
        self.joiner_context.0.on_dedup_joiner(
            self.canonical,
            self.winner_request_id,
            self.winner_audited,
        );
    }
}

impl std::fmt::Debug for DedupJoinerEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DedupJoinerEvent")
            .field("canonical", &self.canonical)
            .field("winner_request_id", &self.winner_request_id)
            .field("winner_audited", &self.winner_audited)
            .finish()
    }
}

/// Per-node bookkeeping inside the DAG. `pub(in crate::dag)` so the
/// `terminal_failures` child module can mutate
/// `failed_blocker_deps` and `deps_remaining` directly when fanning
/// out terminal failures to admitted waiters.
#[derive(Debug)]
pub(in crate::dag) struct DagNode {
    pub(in crate::dag) identity: WorkNodeIdentity,
    pub(in crate::dag) kind: WorkKind,
    pub(in crate::dag) base_priority: Priority,
    pub(in crate::dag) enqueue_time: Instant,
    /// Optional session-side context propagated by the driver. Carried
    /// as opaque bytes; the dispatch loop reads it when installing TLS.
    pub(in crate::dag) request_context: Option<crate::request_context::OpaqueRequestContext>,
    /// Dependency identities this node is gated on. Each entry is the
    /// identity of another work node whose readiness must clear before
    /// this node is dispatchable.
    pub(in crate::dag) deps_remaining: BTreeSet<DepKey>,
    /// Dependency records for [`DepKey`]s whose producer failed
    /// terminally before this node became dispatchable. Two
    /// population paths:
    ///
    /// 1. **Fan-out path** ([`SchedulerDag::fanout_source_failure_to_analysis_waiters`]):
    ///    a Source terminalization releases the Analysis [`DepKey`]
    ///    from the waiter's `deps_remaining` and records a
    ///    [`FailedDepRecord`] here so the dispatched waiter does not
    ///    silently resolve `Ready` over a missing prerequisite.
    /// 2. **Pre-admission path** ([`SchedulerDag::attach_failed_dep`]):
    ///    a fresh admission discovers a recorded terminal failure
    ///    against one of its planned blockers (via the matrix
    ///    consulting [`SchedulerDag::terminal_dep_failures`]) and
    ///    attaches the [`FailedDepRecord`] to the freshly-submitted
    ///    node BEFORE it dispatches. This covers the case where
    ///    the producer failed BEFORE the consumer was admitted —
    ///    the fan-out path would otherwise miss the consumer
    ///    entirely.
    ///
    /// The map carries each [`FailedDepRecord`] keyed by its
    /// [`DepKey`] so the executor can build the typed
    /// [`crate::job::SchedulerError::DependencyFailed`] with the
    /// producer's terminal cause attached. `BTreeMap` for
    /// deterministic dispatch ordering — the executor uses the
    /// first entry to populate the typed error.
    ///
    /// Empty for every node that has no failed prerequisites — the
    /// default fast path. Drained by [`SchedulerDag::next_ready`] into
    /// the [`ReadyJob`].
    pub(in crate::dag) failed_blocker_deps: BTreeMap<DepKey, FailedDepRecord>,
    /// `true` once the node has been published to dispatch and removed
    /// from `iter_ready`. Used as a soft sentinel — final removal goes
    /// through [`SchedulerDag::complete`] / [`SchedulerDag::cancel`].
    pub(in crate::dag) dispatched: bool,
    /// Marker for nodes superseded by a higher generation. Excluded
    /// from `iter_ready`; pruned via cancel paths.
    pub(in crate::dag) cancelled: bool,
    /// Capacity reservation acquired at dispatch time. Stored as
    /// `Some` between `next_ready` and `complete` / `cancel`; the
    /// type-level by-value release semantics ensure permits return
    /// to the pool exactly once.
    pub(in crate::dag) reservation: Option<DagCapacityReservation>,
}

/// Lightweight dependency key — the subset of [`WorkNodeIdentity`]
/// the DAG uses for edge gating. Cheap to clone, hashable.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DepKey {
    /// A specific file-stage completion.
    FileStage {
        canonical: Arc<str>,
        generation: u64,
        stage: FileStageKey,
    },
    /// A specific artifact completion. Kept symmetric with the identity
    /// variant for completeness even though artifact-on-artifact gating
    /// is rare in current callers.
    Artifact {
        canonical: Arc<str>,
        generation: u64,
        profile_hash: Hash16,
        content_hash: Hash16,
    },
    /// A specific cache-node completion.
    CacheNode {
        cache_id: SchedulerCacheId,
        key_hash: Hash16,
        view_epoch: u64,
        snapshot_pin_id: PinId,
    },
}

impl DepKey {
    /// Build a dep key from a work-node identity. The reverse
    /// conversion is intentionally lossy — identities carry the same
    /// data as dep keys today, but having a separate type makes the
    /// "this is a gating edge, not a node" intent explicit at every
    /// call site.
    pub fn from_identity(identity: &WorkNodeIdentity) -> Self {
        match identity {
            WorkNodeIdentity::FileStage {
                canonical,
                generation,
                stage,
            } => DepKey::FileStage {
                canonical: Arc::clone(canonical),
                generation: *generation,
                stage: *stage,
            },
            WorkNodeIdentity::Artifact {
                canonical,
                generation,
                profile_hash,
                content_hash,
            } => DepKey::Artifact {
                canonical: Arc::clone(canonical),
                generation: *generation,
                profile_hash: *profile_hash,
                content_hash: *content_hash,
            },
            WorkNodeIdentity::CacheNode {
                cache_id,
                key_hash,
                view_epoch,
                snapshot_pin_id,
            } => DepKey::CacheNode {
                cache_id: *cache_id,
                key_hash: *key_hash,
                view_epoch: *view_epoch,
                snapshot_pin_id: *snapshot_pin_id,
            },
        }
    }
}

/// A node that has been dequeued for dispatch.
#[derive(Debug, Clone)]
pub struct ReadyJob {
    pub token: SubmissionToken,
    pub identity: WorkNodeIdentity,
    pub kind: WorkKind,
    pub effective_priority: Priority,
    pub enqueue_time: Instant,
    pub request_context: Option<crate::request_context::OpaqueRequestContext>,
    /// [`FailedDepRecord`]s for blockers whose producer failed
    /// terminally before this node dispatched. Drained from the
    /// node's `failed_blocker_deps` at `next_ready` time. The pre-
    /// dispatch short-circuit at the top of
    /// `Scheduler::execute_stage_on_worker` surfaces a typed
    /// [`crate::job::SchedulerError::DependencyFailed`] when this
    /// map is non-empty so a dependency-failure cannot resolve a
    /// waiting downstream node as `Ready` — regardless of task
    /// kind (Source / Analysis / Artifact).
    ///
    /// Empty for the normal fast path. The carrier is `BTreeMap`
    /// keyed by [`DepKey`] so iteration is deterministic: the
    /// executor uses the first entry to populate the typed error.
    /// Each entry's [`FailedDepRecord::cause`] preserves the
    /// producer's terminal error so the surfaced
    /// `DependencyFailed` can carry it through (cf.
    /// [`crate::job::SchedulerError::DependencyFailed::cause`]).
    pub failed_blocker_deps: BTreeMap<DepKey, FailedDepRecord>,
}

/// Per-file aggregation of request groups, keyed by `(canonical, generation)`.
///
/// Each entry holds the per-target waiter groups for that file/generation.
/// The DAG owns this aggregation so request bookkeeping is co-located with
/// the readiness authority — the file-stage node and the request groups
/// share the same dag-side lock.
#[derive(Default)]
struct FileWaiterState {
    groups: Vec<RequestGroup>,
}

impl std::fmt::Debug for FileWaiterState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWaiterState")
            .field("groups", &self.groups.len())
            .finish()
    }
}

/// Driver-owned scheduling DAG: SOLE readiness authority.
///
/// Owns admission, dedup, generation supersession, dependency gating,
/// blocker resolution, priority aging, the capacity counter, and the
/// per-file request-group bookkeeping. There is exactly ONE of these
/// per scheduler.
pub struct SchedulerDag {
    /// Per-token node bookkeeping. Visibility is narrowed to
    /// `pub(in crate::dag)` because the `terminal_failures` child
    /// module mutates `DagNode.failed_blocker_deps` directly when
    /// fanning out terminal failures to admitted waiters. The
    /// child-module split is structural; no caller outside `dag`
    /// reaches in to mutate this map.
    pub(in crate::dag) nodes: FxHashMap<SubmissionToken, DagNode>,
    /// Dedup index — identity → token. A second `submit` with the
    /// same identity merges into the existing node (priority upgrade)
    /// rather than producing a new token. `pub(in crate::dag)` so
    /// the `terminal_failures` child module can resolve
    /// `(identity → token → node)` in `attach_failed_dep`.
    pub(in crate::dag) by_identity: FxHashMap<WorkNodeIdentity, SubmissionToken>,
    /// Reverse map: `DepKey` → list of tokens whose `deps_remaining`
    /// contains that key. Used to fan out the blocker-resolve sweep.
    /// `pub(in crate::dag)` so the `terminal_failures` child module
    /// can drain Analysis-keyed waiters on terminal failure.
    pub(in crate::dag) waiters: FxHashMap<DepKey, Vec<SubmissionToken>>,
    /// Per-`(canonical, generation)` request waiter groups. Each group
    /// holds the senders interested in a particular `TargetStage` plus
    /// the winner context for that group.
    file_waiters: FxHashMap<FileGenKey, FileWaiterState>,
    /// Late-discovered Artifact prerequisite blockers, keyed by
    /// `(owner_canonical, owner_generation)`.
    ///
    /// Populated when a `register_resolved_deps`-style flow discovers
    /// a blocker AFTER the owner's Analysis identity has already
    /// dispatched (or already completed): the dispatched Analysis
    /// node's incoming edges are immutable, so the blocker cannot be
    /// attached to that in-flight node and instead rides on the
    /// owner's downstream Artifact admissions until the blocker's
    /// Analysis lands.
    ///
    /// Race-safety: the registry lives behind the DAG mutex, so
    /// every read and write is serialized with the surrounding DAG
    /// state changes (dedup, completion, supersession). Callers
    /// hold the DAG lock when invoking the typed API methods below.
    ///
    /// Lifecycle is owned by [`Self::record_artifact_blockers`],
    /// [`Self::drain_artifact_blockers`],
    /// [`Self::clear_artifact_blockers`], and
    /// [`Self::scrub_artifact_blockers_referencing`] — all
    /// implemented in the [`blocker_registry`] child module. The
    /// child module accesses this field directly because Rust
    /// privacy makes parent-module fields visible to child modules
    /// only when declared with at least `pub(in crate::dag)`
    /// visibility. The scope is narrowed to `pub(in crate::dag)`
    /// rather than `pub(super)`: from a module under
    /// `crate`, `pub(super)` desugars to `pub(in crate)` — visible
    /// crate-wide — which is broader than the docstring claimed.
    /// `pub(in crate::dag)` restricts the field to the `dag`
    /// module and its descendants (i.e., the `blocker_registry`
    /// child module), keeping the child-module split structural
    /// without leaking access to the rest of the crate. Direct
    /// mutation outside the typed API is intentionally not
    /// exposed.
    pub(in crate::dag) artifact_blocker_deps: FxHashMap<(Arc<str>, u64), PendingBlockerSet>,
    /// Persistent record of terminal producer failures, keyed by the
    /// failed prerequisite's [`DepKey`]. A waiter admission consulting
    /// the dead-producer matrix BEFORE the matching producer
    /// terminalization fan-out would observe a `Resolved`-shaped
    /// snapshot (FileNode present, no current_*, no live DAG
    /// identity) and silently drop the blocker; this map closes that
    /// pre-admission race by recording every
    /// [`SchedulerDag::insert_terminal_dep_failure`] call so a later
    /// matrix consult sees the failure and the admission attaches
    /// the [`FailedDepRecord`] to the freshly-submitted node via
    /// [`SchedulerDag::attach_failed_dep`].
    ///
    /// Lifecycle: populated by [`SchedulerDag::insert_terminal_dep_failure`]
    /// (called from `Scheduler::terminalize_failure` for Source /
    /// Analysis failures). Cleared by [`SchedulerDag::clear`] on
    /// scheduler reset, by [`SchedulerDag::supersede_old_file_generations`]
    /// for the superseded generation's keys, and by
    /// [`SchedulerDag::scrub_terminal_dep_failures_referencing`] when a
    /// canonical is removed.
    ///
    /// `pub(in crate::dag)` so the `terminal_failures` child module
    /// owns the typed API around this map.
    pub(in crate::dag) terminal_dep_failures: FxHashMap<DepKey, FailedDepRecord>,
    aging: DagAgingConfig,
    next_token: u64,
    /// Aggregate in-flight permit counter — sum of cpu + io.
    /// Surfaced via [`Self::in_flight_permits`] for diagnostics; not
    /// used for admission decisions (the typed counters below own
    /// that path).
    capacity_counter: Arc<AtomicU64>,
    /// In-flight CPU-bound permits. Capped at `budget.cpu`.
    cpu_counter: Arc<AtomicU64>,
    /// In-flight I/O-bound permits. Capped at `budget.io`.
    io_counter: Arc<AtomicU64>,
    /// Per-class admission budget.
    budget: DagCapacityBudget,
}

/// `(canonical, generation)` lookup key for [`SchedulerDag::file_waiters`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FileGenKey {
    canonical: Arc<str>,
    generation: u64,
}

impl SchedulerDag {
    /// Create a new empty DAG with the default capacity budget.
    pub fn new(aging: DagAgingConfig) -> Self {
        Self::with_budget(aging, DagCapacityBudget::default())
    }

    /// Create a new empty DAG with an explicit capacity budget.
    pub fn with_budget(aging: DagAgingConfig, budget: DagCapacityBudget) -> Self {
        Self {
            nodes: FxHashMap::default(),
            by_identity: FxHashMap::default(),
            waiters: FxHashMap::default(),
            file_waiters: FxHashMap::default(),
            artifact_blocker_deps: FxHashMap::default(),
            terminal_dep_failures: FxHashMap::default(),
            aging,
            next_token: 1,
            capacity_counter: Arc::new(AtomicU64::new(0)),
            cpu_counter: Arc::new(AtomicU64::new(0)),
            io_counter: Arc::new(AtomicU64::new(0)),
            budget,
        }
    }

    /// Current in-flight permit count across both pools (diagnostic).
    pub fn in_flight_permits(&self) -> u64 {
        self.capacity_counter.load(Ordering::Acquire)
    }

    /// Current in-flight CPU permits (diagnostic).
    pub fn in_flight_cpu_permits(&self) -> u64 {
        self.cpu_counter.load(Ordering::Acquire)
    }

    /// Current in-flight I/O permits (diagnostic).
    pub fn in_flight_io_permits(&self) -> u64 {
        self.io_counter.load(Ordering::Acquire)
    }

    /// Per-class budget configured at construction.
    pub fn budget(&self) -> DagCapacityBudget {
        self.budget
    }

    /// Test-only peek at a node's `deps_remaining` set by token. Used
    /// by dedup tests to assert that the prerequisite set evolves only
    /// in the pre-dispatch arm (post-dispatch incoming edges are
    /// immutable).
    #[cfg(test)]
    pub(crate) fn deps_remaining_for_test(
        &self,
        token: SubmissionToken,
    ) -> Option<&BTreeSet<DepKey>> {
        self.nodes.get(&token).map(|n| &n.deps_remaining)
    }

    /// Test-only peek at a node's `base_priority` by token. Used by
    /// dedup tests to assert priority upgrades land even on a
    /// dispatched node (the one mutation the in-flight dedup arm still
    /// performs alongside winner-context fill-in).
    #[cfg(test)]
    pub(crate) fn base_priority_for_test(&self, token: SubmissionToken) -> Option<Priority> {
        self.nodes.get(&token).map(|n| n.base_priority)
    }

    /// Test-only peek at a node's `request_context.request_id()` by
    /// token. Returns `Some(0)` if the node exists but carries no
    /// context, `None` if no node is present at the token. Used by
    /// dedup tests to assert that the first-arrived submitter's
    /// context survives later dedup joins — i.e. a None context on
    /// the joiner side must NOT overwrite the winner's context.
    #[cfg(test)]
    pub(crate) fn request_context_id_for_test(&self, token: SubmissionToken) -> Option<u64> {
        self.nodes.get(&token).map(|n| {
            n.request_context
                .as_ref()
                .map(|c| c.0.request_id())
                .unwrap_or(0)
        })
    }

    /// Reserve `permits` admission slots untyped (legacy / explicit
    /// callers). Increments only the aggregate counter and leaves the
    /// per-class counters untouched. Prefer
    /// [`Self::try_reserve_for_class`] in dispatch paths so per-class
    /// accounting stays consistent.
    pub fn reserve_capacity(&self, permits: u32) -> DagCapacityReservation {
        if permits > 0 {
            self.capacity_counter
                .fetch_add(permits as u64, Ordering::AcqRel);
        }
        DagCapacityReservation {
            permits,
            class: ResourceClass::Cpu, // untyped reservations carry no class; Cpu is the Debug-only default
            class_counter: None,
            counter: Some(Arc::clone(&self.capacity_counter)),
        }
    }

    /// Try to reserve one admission slot for `class`. Returns `None`
    /// if the per-class budget is full. On success the returned
    /// reservation decrements both the per-class counter and the
    /// aggregate counter on release.
    ///
    /// This is the SINGLE admission-time accounting source. Permits
    /// are released EXACTLY ONCE — the by-value `release(self)`
    /// consume in [`DagCapacityReservation::release`] enforces it at
    /// the type level.
    pub fn try_reserve_for_class(&self, class: ResourceClass) -> Option<DagCapacityReservation> {
        let (counter, cap) = match class {
            ResourceClass::Cpu => (&self.cpu_counter, self.budget.cpu as u64),
            ResourceClass::Io => (&self.io_counter, self.budget.io as u64),
        };
        // Optimistic increment-then-rollback to avoid a CAS loop.
        let prev = counter.fetch_add(1, Ordering::AcqRel);
        if prev >= cap {
            counter.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        self.capacity_counter.fetch_add(1, Ordering::AcqRel);
        Some(DagCapacityReservation {
            permits: 1,
            class,
            class_counter: Some(Arc::clone(counter)),
            counter: Some(Arc::clone(&self.capacity_counter)),
        })
    }

    /// Reserve a CPU or I/O permit on top of an exhausted budget
    /// for the duration of an inline cooperative-pump execute.
    ///
    /// When a worker parks in `wait_or_drive` it still holds the
    /// permit it took to start its current job, but it is not
    /// actively doing useful work on that permit's resource —
    /// its thread is available to execute a transitive dependency
    /// inline. This loan bumps the per-class counter (and the
    /// aggregate) past the configured cap for the duration of the
    /// inline execute; the parked worker's original permit will
    /// release through the normal complete / cancel path once its
    /// own job resumes. The loan releases through the same
    /// single-release contract as a normal reservation.
    ///
    /// Both `Cpu` and `Io` classes are loanable. A CPU worker
    /// loans a CPU permit to inline-execute a CPU dep; an
    /// I/O worker loans an I/O permit to inline-execute an I/O
    /// dep (a Source-stage load). The caller's
    /// [`next_ready_for_pump`] only routes a loan when the
    /// `caller_kind` matches the class — IoWorker × Cpu is NOT
    /// routed to a loan (the IoWorker has no business running
    /// CPU-bound work inline).
    pub fn loan_capacity_for_class(&self, class: ResourceClass) -> Option<DagCapacityReservation> {
        let counter = match class {
            ResourceClass::Cpu => &self.cpu_counter,
            ResourceClass::Io => &self.io_counter,
        };
        counter.fetch_add(1, Ordering::AcqRel);
        self.capacity_counter.fetch_add(1, Ordering::AcqRel);
        Some(DagCapacityReservation {
            permits: 1,
            class,
            class_counter: Some(Arc::clone(counter)),
            counter: Some(Arc::clone(&self.capacity_counter)),
        })
    }

    /// Submit a new work node, or merge into an existing one with the
    /// same identity.
    ///
    /// Behaviour matrix:
    /// - New identity → new token, returned.
    /// - Identity already pending (not yet dispatched) → priority is
    ///   merged (`min` over base + inherited), incoming `deps` are
    ///   merged into `deps_remaining` plus the `waiters` reverse-index,
    ///   and the existing token is returned. The merged dispatch does
    ///   not start until all merged deps complete.
    /// - Identity already dispatched (in-flight) → joiner shares the
    ///   in-flight work. Priority upgrade still applies (the dispatched
    ///   node's effective priority drives stage-transition seeding) and
    ///   the first-arrived `request_context` is preserved. Incoming
    ///   `deps` are IGNORED: a dispatched node's incoming edges are
    ///   immutable. The worker has already begun executing under the
    ///   prerequisite set fixed at dispatch time; mutating that set
    ///   after the fact would let the result publish under a
    ///   dependency set the work never observed. New blockers
    ///   discovered after dispatch belong on downstream admissions
    ///   (e.g., later Artifact requests), never on this in-flight
    ///   node.
    /// - Identity recorded but already cancelled tombstone (race-window
    ///   only — `cancel()` removes the `by_identity` entry on the
    ///   normal path) → reject silently by returning the existing
    ///   token without modification.
    pub fn submit(
        &mut self,
        identity: WorkNodeIdentity,
        kind: WorkKind,
        priority: Priority,
        deps: Vec<DepKey>,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) -> SubmissionToken {
        // Dedup path: same identity. Three sub-cases — pre-dispatch
        // merge (priority + deps + winner-context), in-flight dedup
        // (priority + winner-context only; deps ignored because the
        // prerequisite set is closed once `next_ready` has dispatched
        // the node), cancelled-tombstone reject.
        if let Some(&existing_token) = self.by_identity.get(&identity) {
            // Decide the branch without holding a long mutable borrow.
            let (is_dispatched, is_cancelled) = match self.nodes.get(&existing_token) {
                Some(n) => (n.dispatched, n.cancelled),
                None => (false, false),
            };
            if is_cancelled {
                // Defensive: the cancel path removes `by_identity` so
                // a cancelled tombstone is normally unreachable here.
                // If observed, refuse to re-admit — the cancelled
                // node has no completion path the joiner could see.
                return existing_token;
            }
            if is_dispatched {
                // In-flight dedup. The joiner shares the in-flight
                // result. Priority upgrade still applies — the
                // in-flight node's `base_priority` already feeds
                // `highest_priority_for_file` so stage transitions
                // seeded from this identity carry the new urgency.
                // Incoming `deps` are deliberately not applied: the
                // worker is already executing under the prerequisite
                // set fixed at dispatch time. Producers that discover
                // a late blocker must record it on downstream
                // admissions instead of mutating this node's
                // incoming edges.
                if let Some(existing) = self.nodes.get_mut(&existing_token) {
                    existing.base_priority = std::cmp::min(existing.base_priority, priority);
                    if existing.request_context.is_none() {
                        existing.request_context = request_context;
                    }
                }
                let _ = deps;
                return existing_token;
            }
            // Pre-dispatch merge.
            if let Some(existing) = self.nodes.get_mut(&existing_token) {
                existing.base_priority = std::cmp::min(existing.base_priority, priority);
                // First-arrived request_context wins (winner); if the
                // existing node was admitted without one, populate it
                // from the merging caller so dispatch sees the most
                // informative context available.
                if existing.request_context.is_none() {
                    existing.request_context = request_context;
                }
                // Merge incoming deps into deps_remaining + waiters
                // reverse-index. New deps gate the same dispatch.
                for dep in deps {
                    if existing.deps_remaining.insert(dep.clone()) {
                        self.waiters.entry(dep).or_default().push(existing_token);
                    }
                }
                return existing_token;
            }
        }

        let token = SubmissionToken(self.next_token);
        self.next_token += 1;

        let mut deps_remaining = BTreeSet::new();
        for dep in deps {
            deps_remaining.insert(dep.clone());
            self.waiters.entry(dep).or_default().push(token);
        }

        let node = DagNode {
            identity: identity.clone(),
            kind,
            base_priority: priority,
            enqueue_time: Instant::now(),
            request_context,
            deps_remaining,
            failed_blocker_deps: BTreeMap::new(),
            dispatched: false,
            cancelled: false,
            reservation: None,
        };

        self.nodes.insert(token, node);
        self.by_identity.insert(identity, token);
        token
    }

    /// Look up the token currently associated with `identity`, if any.
    pub fn token_for(&self, identity: &WorkNodeIdentity) -> Option<SubmissionToken> {
        self.by_identity.get(identity).copied()
    }

    /// `true` if `owner_identity` is currently admitted in the DAG
    /// AND its `deps_remaining` set contains `dep`. Used by the
    /// Source-completion blocker registration to detect mutual
    /// cycles (`a depends on b` AND `b depends on a`) BEFORE
    /// admitting a self-deadlocking gating dep on either file's
    /// Analysis.
    pub fn has_dep_on(&self, owner_identity: &WorkNodeIdentity, dep: &DepKey) -> bool {
        let Some(&token) = self.by_identity.get(owner_identity) else {
            return false;
        };
        match self.nodes.get(&token) {
            Some(node) => node.deps_remaining.contains(dep),
            None => false,
        }
    }

    /// Bounded reachability check for the macro-type-dep cycle filter.
    ///
    /// Returns `true` iff a transitive walk over
    /// [`DagNode::deps_remaining`] starting from the `dep` identity
    /// can reach the `owner` identity. The walk is bounded by its
    /// own visited set — every reachable Analysis node is enqueued
    /// at most once via the `visited` HashSet dedup, so the walk
    /// terminates when the frontier drains.
    ///
    /// Catches three cycle classes uniformly:
    ///
    /// 1. **Self** (`A → A`): when `owner == dep`, the filter drops
    ///    the dep before any BFS hops.
    /// 2. **Direct mutual** (`A ↔ B`): one BFS hop lands on the
    ///    owner's Analysis identity in the dep's `deps_remaining`.
    /// 3. **Transitive** (`A → B → C → ... → A`): the BFS walks
    ///    the `FileStage(Analysis)` edges until the owner is
    ///    reached or the queue empties. There is no fixed hop cap
    ///    — a missed cycle behind any number of hops would admit
    ///    a mutually-blocking dep edge and deadlock at runtime,
    ///    so the BFS must walk the complete reachable subgraph.
    ///
    /// The walk only follows `DepKey::FileStage{Analysis}` edges —
    /// those are the gating edges the cycle filter is responsible
    /// for. Artifact, Source, and CacheNode edges are not traversed
    /// because the caller's gate is always an Analysis gate.
    ///
    /// The caller must hold the DAG lock for the duration of the
    /// call so the BFS sees a consistent dep graph. Used by
    /// [`crate::scheduler::Scheduler::filter_macro_cycle_deps`].
    pub fn dep_reaches_owner(
        &self,
        owner_canonical: &Arc<str>,
        owner_generation: u64,
        dep_canonical: &Arc<str>,
        dep_generation: u64,
    ) -> bool {
        self.dep_reaches_owner_with_metrics(
            owner_canonical,
            owner_generation,
            dep_canonical,
            dep_generation,
        )
        .0
    }

    /// Same BFS as [`Self::dep_reaches_owner`], paired with
    /// `BfsMetrics` so tests can assert enqueue-time-visited
    /// semantics directly (each reachable node enqueued exactly
    /// once; frontier bounded by O(V)).
    ///
    /// Pop-time visited would push a node into the frontier
    /// through every incoming fan-in edge before the first pop
    /// dedups, growing `enqueue_count` and `max_frontier_len`
    /// toward O(E). Enqueue-time visited (the production
    /// invariant) holds both at O(V).
    ///
    /// Public `dep_reaches_owner` discards the metrics tuple field
    /// — the cycle-filter contract is the bool, with no public API
    /// or result change from instrumentation. Metrics exist solely
    /// to make the enqueue-time-visited regression class
    /// discriminable in tests.
    pub(crate) fn dep_reaches_owner_with_metrics(
        &self,
        owner_canonical: &Arc<str>,
        owner_generation: u64,
        dep_canonical: &Arc<str>,
        dep_generation: u64,
    ) -> (bool, BfsMetrics) {
        let mut metrics = BfsMetrics::default();

        // Self-cycle: dep IS the owner.
        if owner_canonical.as_ref() == dep_canonical.as_ref() && owner_generation == dep_generation
        {
            return (true, metrics);
        }

        // The cycle filter walks Analysis-stage dep edges. Build the
        // owner identity once so the BFS termination check is a
        // single comparison.
        let owner_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(owner_canonical),
            generation: owner_generation,
            stage: FileStageKey::Analysis,
        };
        let owner_dep = DepKey::FileStage {
            canonical: Arc::clone(owner_canonical),
            generation: owner_generation,
            stage: FileStageKey::Analysis,
        };
        let start_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(dep_canonical),
            generation: dep_generation,
            stage: FileStageKey::Analysis,
        };

        // BFS bounded by the visited set. The walk terminates
        // naturally when the frontier drains — every reachable
        // node is enqueued at most once because `visited` is
        // populated AT ENQUEUE TIME, not at pop time. There is
        // no fixed hop cap: a missed cycle behind an arbitrary
        // number of hops would admit a mutually-blocking dep
        // edge and deadlock at runtime, so the BFS must be
        // complete-over-reachable-nodes rather than truncated.
        //
        // Enqueue-time visited semantics keep the frontier
        // bounded at O(|reachable Analysis nodes|) — i.e. O(V),
        // independent of edge count. Marking on pop would let
        // the same node be `push_back`'d through multiple fan-in
        // edges before the first pop dedups it, growing the
        // frontier toward O(|edges|) on dense graphs. Marking on
        // enqueue prevents that fan-in re-push.
        //
        // Both `visited` and `frontier` hold `WorkNodeIdentity`
        // so the comparison is structural identity (not token),
        // which survives any future tokenizer re-keying.
        let mut visited: std::collections::HashSet<WorkNodeIdentity> =
            std::collections::HashSet::with_capacity(8);
        let mut frontier: std::collections::VecDeque<WorkNodeIdentity> =
            std::collections::VecDeque::with_capacity(8);
        // Seed: mark the start identity as visited BEFORE enqueue
        // so a fan-in edge back to start can never re-enqueue it.
        visited.insert(start_id.clone());
        frontier.push_back(start_id);
        metrics.enqueue_count = metrics.enqueue_count.saturating_add(1);
        metrics.max_frontier_len = metrics.max_frontier_len.max(frontier.len());

        while let Some(current) = frontier.pop_front() {
            // Look up the current node's deps_remaining. A node that
            // is not in the DAG (e.g., not yet admitted) cannot
            // close a cycle through us at this instant.
            let Some(&token) = self.by_identity.get(&current) else {
                continue;
            };
            let Some(node) = self.nodes.get(&token) else {
                continue;
            };
            // Direct hit: this node's deps_remaining contains the
            // owner's Analysis identity → cycle closes here.
            if node.deps_remaining.contains(&owner_dep) {
                return (true, metrics);
            }
            // Otherwise enqueue every Analysis-stage dep for the
            // next layer of the BFS.
            for dep_key in node.deps_remaining.iter() {
                if let DepKey::FileStage {
                    canonical: c,
                    generation: g,
                    stage: FileStageKey::Analysis,
                } = dep_key
                {
                    let next = WorkNodeIdentity::FileStage {
                        canonical: Arc::clone(c),
                        generation: *g,
                        stage: FileStageKey::Analysis,
                    };
                    // Skip the owner; we test the closing edge via
                    // `node.deps_remaining.contains(&owner_dep)`
                    // above, and walking through the owner would
                    // hide a legitimate transitive cycle whose
                    // tail node carries the closing edge.
                    //
                    // `visited.insert(..)` returns `true` only on
                    // first sighting — every reachable node thus
                    // makes it onto the frontier exactly once.
                    if next != owner_id && visited.insert(next.clone()) {
                        frontier.push_back(next);
                        metrics.enqueue_count = metrics.enqueue_count.saturating_add(1);
                        metrics.max_frontier_len = metrics.max_frontier_len.max(frontier.len());
                    }
                }
            }
        }
        (false, metrics)
    }

    /// Whether `identity` has unresolved dependencies (gating).
    pub fn has_pending_deps(&self, identity: &WorkNodeIdentity) -> bool {
        match self.by_identity.get(identity) {
            Some(tok) => self
                .nodes
                .get(tok)
                .map(|n| !n.cancelled && !n.dispatched && !n.deps_remaining.is_empty())
                .unwrap_or(false),
            None => false,
        }
    }

    /// Upgrade the priority of the node identified by `identity`.
    /// Returns the new effective priority if the upgrade applied.
    pub fn upgrade_priority(
        &mut self,
        identity: &WorkNodeIdentity,
        new_priority: Priority,
    ) -> Option<Priority> {
        let tok = self.by_identity.get(identity).copied()?;
        let node = self.nodes.get_mut(&tok)?;
        if node.cancelled || node.dispatched {
            return None;
        }
        let old = node.base_priority;
        node.base_priority = std::cmp::min(node.base_priority, new_priority);
        if node.base_priority < old {
            Some(node.base_priority)
        } else {
            None
        }
    }

    /// Mark the node identified by `identity` as completed (terminal
    /// success). Removes its bookkeeping, releases any capacity
    /// reservation parked on the node, fans out dep-resolution to
    /// any waiters, and returns the list of waiter tokens that have
    /// now become ready.
    pub fn complete(&mut self, identity: &WorkNodeIdentity) -> Vec<SubmissionToken> {
        let tok = match self.by_identity.remove(identity) {
            Some(t) => t,
            None => return Vec::new(),
        };
        // Scrub this node's token from every `waiters` list its
        // outstanding deps still point at, BEFORE removing the node,
        // so the reverse-index never carries a stale entry for a
        // removed token. Snapshot the dep set first because the
        // helper needs `&mut self` on `self.waiters`.
        let outgoing_deps = self
            .nodes
            .get(&tok)
            .map(|n| n.deps_remaining.clone())
            .unwrap_or_default();
        self.remove_incoming_edges(tok, &outgoing_deps);
        if let Some(mut node) = self.nodes.remove(&tok) {
            // Release the dispatched-node's capacity reservation.
            // The by-value `release(self)` consume in
            // DagCapacityReservation makes a second release
            // statically impossible.
            if let Some(reservation) = node.reservation.take() {
                reservation.release();
            }
        }

        let dep_key = DepKey::from_identity(identity);
        let mut newly_ready = Vec::new();
        if let Some(waiters) = self.waiters.remove(&dep_key) {
            for waiter_tok in waiters {
                if let Some(waiter) = self.nodes.get_mut(&waiter_tok) {
                    if waiter.cancelled {
                        continue;
                    }
                    waiter.deps_remaining.remove(&dep_key);
                    if waiter.deps_remaining.is_empty() && !waiter.dispatched {
                        newly_ready.push(waiter_tok);
                    }
                }
            }
        }
        newly_ready
    }

    /// Cancel the node identified by `identity` (supersession or
    /// removal). Releases any capacity reservation parked on the
    /// node, releases waiters whose only remaining dep was this node
    /// so they can fail/retry, and returns the list of stranded
    /// waiter tokens.
    pub fn cancel(&mut self, identity: &WorkNodeIdentity) -> Vec<SubmissionToken> {
        let tok = match self.by_identity.remove(identity) {
            Some(t) => t,
            None => return Vec::new(),
        };
        if let Some(node) = self.nodes.get_mut(&tok) {
            node.cancelled = true;
        }

        let dep_key = DepKey::from_identity(identity);
        let mut stranded = Vec::new();
        if let Some(waiters) = self.waiters.remove(&dep_key) {
            for waiter_tok in waiters {
                if let Some(waiter) = self.nodes.get_mut(&waiter_tok) {
                    if waiter.cancelled {
                        continue;
                    }
                    waiter.deps_remaining.remove(&dep_key);
                    if waiter.deps_remaining.is_empty() && !waiter.dispatched {
                        stranded.push(waiter_tok);
                    }
                }
            }
        }
        // Scrub this node's token from every `waiters` list its own
        // unresolved deps still point at, so the reverse-index never
        // carries a stale entry for a removed token after the node's
        // entry is dropped. Snapshot the dep set first because the
        // helper needs `&mut self` on `self.waiters`.
        let outgoing_deps = self
            .nodes
            .get(&tok)
            .map(|n| n.deps_remaining.clone())
            .unwrap_or_default();
        self.remove_incoming_edges(tok, &outgoing_deps);
        // Drop the cancelled node entry after releasing waiters and
        // returning the permit (by-value release on the parked
        // reservation).
        if let Some(mut node) = self.nodes.remove(&tok) {
            if let Some(reservation) = node.reservation.take() {
                reservation.release();
            }
        }
        stranded
    }

    /// Scrub `token` from every `waiters` list keyed by the deps in
    /// `deps`. Idempotent — if `token` is not present in a list, the
    /// retain is a no-op. Called by [`Self::complete`] and
    /// [`Self::cancel`] immediately before removing the node so the
    /// reverse-index never carries stale entries for a removed token.
    fn remove_incoming_edges(&mut self, token: SubmissionToken, deps: &BTreeSet<DepKey>) {
        for dep in deps {
            if let Some(list) = self.waiters.get_mut(dep) {
                list.retain(|&t| t != token);
                if list.is_empty() {
                    self.waiters.remove(dep);
                }
            }
        }
    }

    /// Cancel every node whose identity matches the predicate. Returns
    /// the count of nodes cancelled and a list of stranded waiter
    /// tokens.
    pub fn cancel_matching<F>(&mut self, predicate: F) -> (usize, Vec<SubmissionToken>)
    where
        F: Fn(&WorkNodeIdentity) -> bool,
    {
        let to_cancel: Vec<WorkNodeIdentity> = self
            .nodes
            .values()
            .filter(|n| !n.cancelled && predicate(&n.identity))
            .map(|n| n.identity.clone())
            .collect();
        let count = to_cancel.len();
        let mut stranded = Vec::new();
        for identity in to_cancel {
            stranded.extend(self.cancel(&identity));
        }
        (count, stranded)
    }

    /// Mark a node as dispatched (its work is now executing on a
    /// pool). Future `iter_ready` calls skip the node; final removal
    /// goes through [`Self::complete`] or [`Self::cancel`] depending
    /// on outcome.
    pub fn mark_dispatched(&mut self, token: SubmissionToken) {
        if let Some(node) = self.nodes.get_mut(&token) {
            node.dispatched = true;
        }
    }

    /// Number of non-cancelled, non-dispatched nodes.
    pub fn pending_len(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| !n.cancelled && !n.dispatched)
            .count()
    }

    /// Total active (dispatched + pending) node count.
    pub fn total_active(&self) -> usize {
        self.nodes.values().filter(|n| !n.cancelled).count()
    }

    /// Dequeue the highest-priority ready node (no remaining deps,
    /// not dispatched, not cancelled, and whose resource class has
    /// admission capacity). Applies aging at dequeue time.
    ///
    /// Hybrid budget per resource class: a job is only yielded if the
    /// DAG can reserve a capacity permit against its resource class.
    /// The reservation is stored on the dispatched node and released
    /// by the by-value consume in [`Self::complete`] or
    /// [`Self::cancel`]. If no candidate has a free permit, returns
    /// `None` — the driver retries on the next pass.
    ///
    /// Thin sync wrapper around [`Self::next_ready_for_pump`] for
    /// driver-led pumps that do not yet supply a caller-kind or an
    /// active-path frame.
    pub fn next_ready(&mut self) -> Option<ReadyJob> {
        self.next_ready_for_pump(crate::caller_kind::CallerKind::Driver, &[])
    }

    /// Caller-aware ready-job selection used by the cooperative
    /// pump. Same readiness gating as [`Self::next_ready`] with
    /// three caller-aware behaviours layered on top:
    ///
    /// 1. **Active-path filter** — nodes whose identity appears
    ///    in `active_path` are SKIPPED. A worker that is itself
    ///    executing the work cannot be handed the same job again
    ///    through the cooperative pump.
    /// 2. **Caller-kind class preference** — `CpuWorker` callers
    ///    prefer CPU candidates; `IoWorker` callers prefer I/O
    ///    candidates. The preference biases an inline-execute
    ///    path through `dispatch_ready_job` so the calling
    ///    worker runs a dep on its own thread.
    /// 3. **Capacity loan** — when the per-class budget is full
    ///    and the calling worker is parked (active_path is non-
    ///    empty), the pump loans a permit on top of the budget
    ///    via [`Self::loan_capacity_for_class`]. Cpu × CpuWorker|Inline
    ///    and Io × IoWorker are the loanable combinations;
    ///    cross-class combinations skip the candidate.
    pub fn next_ready_for_pump(
        &mut self,
        caller_kind: crate::caller_kind::CallerKind,
        active_path: &[WorkNodeIdentity],
    ) -> Option<ReadyJob> {
        /// Sort key used to rank candidates in `next_ready` —
        /// (effective_priority, enqueue_time, kind_ord, token).
        type RankKey = (Priority, Instant, u8, u64);
        /// Per-candidate ranked entry: (sort key, token, class).
        type RankedCandidate = (RankKey, SubmissionToken, ResourceClass);

        let now = Instant::now();

        // First, iterate the candidates in priority order so we
        // attempt the most urgent jobs first. We pick the best
        // candidate whose resource class still has capacity.
        let mut ranked: Vec<RankedCandidate> = self
            .nodes
            .iter()
            .filter_map(|(tok, node)| {
                if node.cancelled || node.dispatched {
                    return None;
                }
                if !node.deps_remaining.is_empty() {
                    return None;
                }
                // Skip identities the caller is itself waiting on.
                // Dispatching them through the cooperative pump
                // would re-enter `execute_stage_on_worker` on a
                // stage that the calling worker is already running,
                // duplicating the dispatch and breaking the parked-
                // reservation single-release invariant. The base
                // [`Self::next_ready`] call site passes an empty
                // slice, so this filter only fires under the
                // cooperative pump.
                if active_path.contains(&node.identity) {
                    return None;
                }
                let effective = effective_priority(node, now, &self.aging);
                let kind_ord = match node.kind {
                    WorkKind::Load => 0,
                    WorkKind::Parse => 1,
                    WorkKind::Analysis => 2,
                    WorkKind::Artifact => 3,
                    WorkKind::CacheNode => 4,
                };
                Some((
                    (effective, node.enqueue_time, kind_ord, tok.0),
                    *tok,
                    ResourceClass::for_work_kind(node.kind),
                ))
            })
            .collect();
        ranked.sort_by(|a, b| {
            // CPU workers prefer CPU work so an inline-execute path
            // through `dispatch_ready_job` can run the dependency on
            // the SAME worker that is parked. I/O workers are
            // symmetric. Non-pool callers (Driver, Inline, External)
            // see the original priority-only ordering.
            let cpu_preference = matches!(caller_kind, crate::caller_kind::CallerKind::CpuWorker);
            let io_preference = matches!(caller_kind, crate::caller_kind::CallerKind::IoWorker);
            if cpu_preference {
                match (a.2, b.2) {
                    (ResourceClass::Cpu, ResourceClass::Io) => return std::cmp::Ordering::Less,
                    (ResourceClass::Io, ResourceClass::Cpu) => return std::cmp::Ordering::Greater,
                    _ => {}
                }
            } else if io_preference {
                match (a.2, b.2) {
                    (ResourceClass::Io, ResourceClass::Cpu) => return std::cmp::Ordering::Less,
                    (ResourceClass::Cpu, ResourceClass::Io) => return std::cmp::Ordering::Greater,
                    _ => {}
                }
            }
            a.0.cmp(&b.0)
        });

        for (_, tok, class) in ranked {
            // Try to reserve a permit for this candidate's class.
            // Parked workers (Cpu / Io / Inline) in the cooperative
            // pump can take a typed capacity LOAN for their matching
            // resource class when the configured budget is already
            // full — the calling worker is holding its own permit
            // but parked, so its thread is the inline-execute path
            // that runs the dep. The loan releases through the
            // same single-release Drop contract as a normal
            // reservation.
            //
            // Class × caller_kind loan matrix:
            // - Cpu × CpuWorker|Inline → loan a Cpu permit.
            // - Io  × IoWorker         → loan an Io permit.
            // - other combinations: no loan, skip the candidate.
            let loan_eligible = matches!(
                (caller_kind, class),
                (
                    crate::caller_kind::CallerKind::CpuWorker
                        | crate::caller_kind::CallerKind::Inline,
                    ResourceClass::Cpu,
                ) | (crate::caller_kind::CallerKind::IoWorker, ResourceClass::Io,)
            );
            let reservation = match self.try_reserve_for_class(class) {
                Some(r) => Some(r),
                None if loan_eligible && !active_path.is_empty() => {
                    self.loan_capacity_for_class(class)
                }
                None => None,
            };
            let reservation = match reservation {
                Some(r) => r,
                None => continue, // class budget full — skip
            };
            let node = match self.nodes.get_mut(&tok) {
                Some(n) => n,
                None => {
                    // Node disappeared mid-scan — return the permit.
                    reservation.release();
                    continue;
                }
            };
            let effective_priority = effective_priority(node, now, &self.aging);
            node.dispatched = true;
            // Park the reservation on the node so `complete` /
            // `cancel` returns the permit exactly once.
            node.reservation = Some(reservation);
            // Drain the failed-blocker-deps marker into the ReadyJob
            // so the pre-dispatch short-circuit in
            // `Scheduler::execute_stage_on_worker` can surface a typed
            // DependencyFailed when a prerequisite died terminally
            // before this node became dispatchable. `take` moves the
            // map out — the DagNode no longer owns it once the node
            // has been published to dispatch. The carried
            // `FailedDepRecord` values preserve each producer's
            // terminal cause so the surfaced `DependencyFailed`
            // carries it through verbatim.
            let failed_blocker_deps = std::mem::take(&mut node.failed_blocker_deps);
            return Some(ReadyJob {
                token: tok,
                identity: node.identity.clone(),
                kind: node.kind,
                effective_priority,
                enqueue_time: node.enqueue_time,
                request_context: node.request_context.clone(),
                failed_blocker_deps,
            });
        }
        None
    }

    /// Drop ALL state (nodes, edges, waiters, file waiters). Used by
    /// the driver's reset path. Reservations parked on nodes are
    /// dropped, which releases their permits via Drop (single
    /// release path). Counters are then explicitly zeroed so any
    /// undropped reservations held by callers do not underflow on
    /// later release.
    pub fn clear(&mut self) {
        // Drop reservations before clearing so each one's Drop hook
        // decrements the counters; the explicit zero below catches
        // any stragglers held outside the dag.
        for (_, mut node) in self.nodes.drain() {
            // Reservation::Drop returns permits to both counters.
            let _ = node.reservation.take();
        }
        self.by_identity.clear();
        self.waiters.clear();
        // Signal Shutdown on any outstanding waiters so handles don't
        // hang across reset.
        for (_, mut state) in self.file_waiters.drain() {
            for mut group in state.groups.drain(..) {
                group.signal_all(CompletionState::Shutdown);
            }
        }
        self.artifact_blocker_deps.clear();
        self.terminal_dep_failures.clear();
        self.capacity_counter.store(0, Ordering::Release);
        self.cpu_counter.store(0, Ordering::Release);
        self.io_counter.store(0, Ordering::Release);
    }

    // ─────────────────────────────────────────────────────────────────
    // Request-group bookkeeping co-located with the readiness authority.
    // The driver registers caller senders here and the dag fans out
    // completion / supersede / fail / shutdown signals.
    // ─────────────────────────────────────────────────────────────────

    /// Register a caller sender on the `(canonical, generation, target)`
    /// waiter group. If a matching group exists the sender joins;
    /// otherwise a new group is created.
    ///
    /// On dedup, the joiner observes the winner via a
    /// [`DedupJoinerEvent`] that the caller fires AFTER releasing the
    /// DAG lock (see [`DedupJoinerEvent::fire`]). The session-side
    /// `on_dedup_joiner` impl records a share-reuse fact keyed by
    /// `(canonical, winner_id, winner_audited)`; running that callback
    /// under the DAG lock would let session-side code re-enter the
    /// scheduler while admission still holds the mutex. This method
    /// therefore RETURNS the event instead of invoking it — admission
    /// (single or batch) collects the events and fires them once the
    /// lock has dropped.
    ///
    /// Returns `Some(event)` exactly when this registration was a
    /// dedup join AND the joiner carried a `request_context`;
    /// otherwise `None`.
    ///
    /// There is no group-level priority upgrade semantics: priority
    /// service lives on the DAG node and callers propagate urgency via
    /// [`Self::upgrade_priority`] on the matching identity directly.
    #[must_use = "the returned DedupJoinerEvent must be fired after the DAG lock is released"]
    pub fn register_request(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        target: TargetStage,
        sender: CompletionSender<RequestResult>,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) -> Option<DedupJoinerEvent> {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        let state = self.file_waiters.entry(key).or_default();
        for group in state.groups.iter_mut() {
            if group.target == target {
                // Dedup: capture the winner details so the caller can
                // observe them via `on_dedup_joiner` AFTER unlock. The
                // callback is NOT fired here.
                let event = request_context.as_ref().map(|joiner_ctx| {
                    let (winner_id, winner_audited) = group
                        .winner_context
                        .as_ref()
                        .map(|w| (w.0.request_id(), w.0.capture_enabled()))
                        .unwrap_or((0, false));
                    DedupJoinerEvent {
                        canonical: Arc::clone(canonical),
                        joiner_context: joiner_ctx.clone(),
                        winner_request_id: winner_id,
                        winner_audited,
                    }
                });
                group.senders.push(sender);
                return event;
            }
        }
        state.groups.push(RequestGroup {
            target,
            senders: vec![sender],
            winner_context: request_context,
        });
        None
    }

    /// Signal all waiter groups at `(canonical, generation)` matching
    /// the completed task kind.
    ///
    /// - Groups whose target is satisfied by `completed` → Ready(result), removed.
    /// - Groups with older generation are NOT touched here — use
    ///   [`Self::supersede_old_file_generations`] for that.
    ///
    /// As a side effect for successful Source / Analysis stages at
    /// this `(canonical, generation)`, clear any persistent
    /// terminal-dep-failure record at the same key. A Source-failed
    /// dep that gets retried at the same generation and succeeds
    /// (e.g. via an external `commit_source` / loader refresh) would
    /// otherwise leave a stale `Failed(record)` in the
    /// `terminal_dep_failures` store, misclassifying the dep as
    /// dead on any subsequent matrix consult. Artifact completions
    /// do not touch this store — Artifact failures terminalize the
    /// per-profile slot, not the canonical's Analysis key.
    pub fn signal_stage_complete(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        completed: &TaskKind,
        result: &RequestResult,
    ) {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        if let Some(state) = self.file_waiters.get_mut(&key) {
            state.groups.retain_mut(|group| {
                if group.target.is_satisfied_by(completed) {
                    group.signal_all(CompletionState::Ready(result.clone()));
                    false
                } else {
                    true
                }
            });
            if state.groups.is_empty() {
                self.file_waiters.remove(&key);
            }
        }
        // Same-generation recovery: clear any terminal failure
        // record for this canonical at this generation. A
        // previously-failed Source/Analysis that succeeds at the
        // same generation must not pin future matrix consults as
        // `Failed`.
        if matches!(completed, TaskKind::Source | TaskKind::Analysis) {
            self.clear_terminal_dep_failure_for_gen(canonical, generation);
        }
    }

    /// Signal `Superseded` to every waiter group at `(canonical, gen)`
    /// for `gen < current_gen`, AND cancel every DAG node (FileStage
    /// or Artifact) for the same canonical at an older generation.
    ///
    /// Without the node-cancel sweep, stale-generation nodes would
    /// linger in `nodes` / `by_identity` and could still be dispatched
    /// by `next_ready`, racing the live-generation work.
    pub fn supersede_old_file_generations(&mut self, canonical: &Arc<str>, current_gen: u64) {
        let stale_keys: Vec<FileGenKey> = self
            .file_waiters
            .keys()
            .filter(|k| k.canonical.as_ref() == canonical.as_ref() && k.generation < current_gen)
            .cloned()
            .collect();
        for key in stale_keys {
            if let Some(mut state) = self.file_waiters.remove(&key) {
                for mut group in state.groups.drain(..) {
                    group.signal_all(CompletionState::Superseded);
                }
            }
        }
        // Cancel every DAG node for this canonical at an older
        // generation. The cancel sweep covers both file-stage and
        // artifact identities so no work for the stale generation
        // dispatches after the bump.
        let canonical_for_match = Arc::clone(canonical);
        let (_count, _stranded) = self.cancel_matching(|identity| match identity {
            WorkNodeIdentity::FileStage {
                canonical: c,
                generation,
                ..
            }
            | WorkNodeIdentity::Artifact {
                canonical: c,
                generation,
                ..
            } => c.as_ref() == canonical_for_match.as_ref() && *generation < current_gen,
            WorkNodeIdentity::CacheNode { .. } => false,
        });
        // Drop any stale per-(owner, generation) Artifact blocker
        // entries from the superseded generations; the new
        // generation needs its own `record_artifact_blockers` call
        // to record its own blockers.
        self.artifact_blocker_deps.retain(|(owner, gen), _| {
            !(owner.as_ref() == canonical.as_ref() && *gen < current_gen)
        });
        // Drop any persistent terminal-dep-failure entries whose
        // DepKey references this canonical at a superseded
        // generation. Without this scrub a stale failure record
        // would pin a future admission as `Failed` even though the
        // invalidation produced a fresh generation that may yet
        // succeed.
        self.terminal_dep_failures.retain(|key, _record| match key {
            DepKey::FileStage {
                canonical: c,
                generation: g,
                ..
            }
            | DepKey::Artifact {
                canonical: c,
                generation: g,
                ..
            } => !(c.as_ref() == canonical.as_ref() && *g < current_gen),
            DepKey::CacheNode { .. } => true,
        });
    }

    /// Signal `Failed(error)` to every waiter group at
    /// `(canonical, generation)`. All groups removed.
    pub fn signal_file_failed(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        error: SchedulerError,
    ) {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        if let Some(mut state) = self.file_waiters.remove(&key) {
            for mut group in state.groups.drain(..) {
                group.signal_all(CompletionState::Failed(error.clone()));
            }
        }
    }

    /// Signal `Failed(error)` to only the waiter groups whose target
    /// is satisfied by the failed task kind. Other targets at the
    /// same `(canonical, generation)` remain. Used for per-profile
    /// Artifact failures so other profiles keep their pending state.
    pub fn signal_file_failed_for_stage(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        failed_stage: &TaskKind,
        error: SchedulerError,
    ) {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        if let Some(state) = self.file_waiters.get_mut(&key) {
            state.groups.retain_mut(|group| {
                if group.target.is_satisfied_by(failed_stage) {
                    group.signal_all(CompletionState::Failed(error.clone()));
                    false
                } else {
                    true
                }
            });
            if state.groups.is_empty() {
                self.file_waiters.remove(&key);
            }
        }
    }

    /// Signal `Shutdown` to every waiter group for `canonical` across
    /// all generations.
    pub fn signal_file_shutdown(&mut self, canonical: &Arc<str>) {
        let keys: Vec<FileGenKey> = self
            .file_waiters
            .keys()
            .filter(|k| k.canonical.as_ref() == canonical.as_ref())
            .cloned()
            .collect();
        for key in keys {
            if let Some(mut state) = self.file_waiters.remove(&key) {
                for mut group in state.groups.drain(..) {
                    group.signal_all(CompletionState::Shutdown);
                }
            }
        }
    }

    /// Signal `Shutdown` to every outstanding waiter group across the
    /// whole DAG. Called from `Scheduler::Drop`.
    pub fn signal_all_shutdown(&mut self) {
        for (_, mut state) in self.file_waiters.drain() {
            for mut group in state.groups.drain(..) {
                group.signal_all(CompletionState::Shutdown);
            }
        }
    }

    /// Highest priority among waiter groups at `(canonical, generation)`.
    /// Used to propagate Critical urgency across stage transitions.
    pub fn highest_priority_for_file(
        &self,
        canonical: &Arc<str>,
        generation: u64,
    ) -> Option<Priority> {
        // Priority tracking on waiter groups is intentionally
        // delegated to the dag nodes. This method returns the highest
        // base_priority across all dag nodes for this `(canonical,
        // generation)` — pending or dispatched alike — so a fresh
        // stage transition inherits the urgency of any outstanding
        // request at this generation.
        let mut best: Option<Priority> = None;
        for node in self.nodes.values() {
            if node.cancelled {
                continue;
            }
            if node_matches_file_gen(node, canonical, generation) {
                let p = node.base_priority;
                best = Some(match best {
                    Some(prev) => std::cmp::min(prev, p),
                    None => p,
                });
            }
        }
        best
    }

    /// Profile hashes and priorities for every pending Artifact waiter
    /// group at `(canonical, generation)`. Used by the driver after
    /// Analysis completes to enqueue Artifact jobs.
    pub fn pending_artifact_profiles(
        &self,
        canonical: &Arc<str>,
        generation: u64,
    ) -> Vec<(u64, Priority)> {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        let mut out = Vec::new();
        if let Some(state) = self.file_waiters.get(&key) {
            for group in state.groups.iter() {
                if let TargetStage::Artifact { profile_hash } = &group.target {
                    // Use the highest priority observed for this
                    // file-generation as the seed; the driver will
                    // submit() and the dag's dedup path will continue
                    // to upgrade.
                    let prio = self
                        .highest_priority_for_file(canonical, generation)
                        .unwrap_or(Priority::Background);
                    out.push((*profile_hash, prio));
                }
            }
        }
        out
    }

    /// Read the winner context (if any) for the `(canonical, generation)`
    /// group. Returns the first non-None winner observed. Used by the
    /// dispatch loop to install TLS on the worker.
    pub fn winner_context_for(
        &self,
        canonical: &Arc<str>,
        generation: u64,
    ) -> Option<crate::request_context::OpaqueRequestContext> {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        self.file_waiters
            .get(&key)
            .and_then(|state| state.groups.iter().find_map(|g| g.winner_context.clone()))
    }

    /// Whether ANY waiter group exists for `(canonical, generation)`.
    pub fn has_waiters(&self, canonical: &Arc<str>, generation: u64) -> bool {
        let key = FileGenKey {
            canonical: Arc::clone(canonical),
            generation,
        };
        self.file_waiters.contains_key(&key)
    }

    // The Artifact blocker-dep registry's typed API lives in the
    // [`blocker_registry`] child module, declared at the top of
    // this file. The underlying storage stays on `SchedulerDag` so
    // every registry mutation still synchronises through the DAG
    // mutex; the child split is structural (file size + ownership)
    // and does not change race semantics.
}

fn node_matches_file_gen(node: &DagNode, canonical: &Arc<str>, generation: u64) -> bool {
    match &node.identity {
        WorkNodeIdentity::FileStage {
            canonical: c,
            generation: g,
            ..
        }
        | WorkNodeIdentity::Artifact {
            canonical: c,
            generation: g,
            ..
        } => c.as_ref() == canonical.as_ref() && *g == generation,
        WorkNodeIdentity::CacheNode { .. } => false,
    }
}

/// Compute the effective priority for `node` at `now` under `aging`.
fn effective_priority(node: &DagNode, now: Instant, aging: &DagAgingConfig) -> Priority {
    let base = node.base_priority;
    let age = now.saturating_duration_since(node.enqueue_time);
    match base {
        Priority::Background if age >= aging.background_to_interactive => Priority::Interactive,
        Priority::Maintenance if age >= aging.maintenance_to_background => Priority::Background,
        other => other,
    }
}

/// Task-key / profile-hash conversion utilities. Re-exported here so
/// existing `crate::dag::dag_keys_for_task` and
/// `crate::dag::profile_hash_*` call sites continue to resolve
/// without callers needing to know about the submodule split.
mod task_keys;
pub use task_keys::{dag_keys_for_task, profile_hash_from_bytes, profile_hash_to_bytes};

#[cfg(test)]
#[path = "dag_tests.rs"]
mod dag_tests;
