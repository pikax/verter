//! Host-owned semantic-query memo table.
//!
//! This module provides the concrete backing store for
//! [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey) →
//! [`SemanticNodeId`](crate::semantic_query::SemanticNodeId) memoization
//! and the stable storage for
//! [`SemanticNodeData`](crate::semantic_query::SemanticNodeData).
//!
//! ## Contract
//!
//! - One shared memo per [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).
//! - Entries are keyed by `SemanticQueryKey`; cold winners compute the
//!   node, store it, and return its id. Joiners on the same key observe
//!   the same id (no duplicated cold work).
//! - [`SemanticNodeId`] is never reused. A node lives while an owner holds
//!   it — a cache candidate, a construction scope, a delivered lease, a
//!   counted edge from a live parent — and is destroyed by its last release
//!   (`arena.rs`, **Lifetime**).
//! - **Same-path recursion** returns `QueryResult::Recursive(self_id)`
//!   so cycles dedup rather than re-entering.
//! - **Distinct top-level waiters** block cooperatively on a per-entry
//!   [`Condvar`] pairing (see [`FlightCell`]).
//! - Cancelled, budget-exceeded, or partial results **never** promote to a
//!   warm memo entry; they surface as [`QueryError`] variants and the
//!   in-flight admission is removed so the next caller starts fresh.
//! - Entries are immutable once stored. Node data never retains borrowed
//!   OXC AST pointers — callers materialize semantic data before calling
//!   [`SemanticGraphStore::intern_node`].
//! - A document CLOSE evicts the cache owners bound to the closed
//!   canonical's content ([`SemanticGraphStore::close_canonical`], see
//!   `release.rs`); its nodes are destroyed once no caller holds them, and
//!   a held result is never revoked. An EDIT
//!   ([`SemanticGraphStore::invalidate_canonical`]) drains only the entries
//!   its carriers name.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(any(test, feature = "test-support"))]
use crate::instant::Instant;

use dashmap::DashMap;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::semantic_query::{
    CacheRead, DepSignature, NodeScopeId, OriginEdge, OriginEdgeKind, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryValue, SemanticQueryValueTag,
};

pub mod arena;
mod binder_dependencies;
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
pub mod charge_witness;
pub mod construction_copy;
pub mod delivery_lease;
pub mod derivation;
pub mod family;
mod family_footprint;
mod family_intern;
mod family_retention;
mod fingerprint_hasher;
mod flow_return_memo;
#[cfg(any(test, feature = "test-support"))]
pub mod graph_fixture;
mod graph_in;
pub use graph_in::GraphIn;
mod hash_cons_memos;
pub(crate) use hash_cons_memos::CompletedDeferredEvaluation;
pub mod inflight;
#[cfg(any(test, feature = "test-support", feature = "soak-support"))]
pub use inflight::{FlightRendezvous, FlightRendezvousMiss, FlightRendezvousObservation};
pub(crate) mod intern_table;
pub mod interner;
pub(crate) mod member_index;
pub mod node_data_lease;
pub mod node_roots;
pub(crate) mod nodes;
mod structural_domains;
pub use nodes::AliasChainEnd;
mod observability;
pub use observability::NodeOwnershipCounts;
mod origin_edges;
pub(crate) use origin_edges::record_origin_edge;
pub mod prepared;
pub mod refusal_summary;
pub(crate) mod relation_memo;
pub(crate) mod release;
mod resolve_call_memo;
mod retention;
mod reverse_index;
#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
pub use reverse_index::ReverseIndexEdgeOps;
pub use reverse_index::{ReceiptReverseIndex, ReverseIndexOccupancy};
pub mod scc_publish;
mod signature_epoch;
pub mod union_views;
pub mod unresolved_reach;

pub use arena::{NodeData, UNALLOCATABLE_ID_FLOOR};
pub(crate) use inflight::InlineMemberFlight;
pub use intern_table::{IdentityIndexSnapshot, InternIndexOccupancy};
pub use producer::{
    Acquired, Claim, ClaimAttempt, Joined, ProducerLease, ReadCapture, Recursion, Subscription,
};
pub use release::{ClosedContent, DrainedStorageReport, SemanticCloseReport};
pub use scc_publish::{
    PendingFlowReturnMember, PendingRelationMember, PendingResolveCallMember, SccRootWitness,
};
pub(crate) use verter_execution::tasks::ExecutionTask;

pub mod producer;
pub(crate) mod stats;
#[cfg(any(
    test,
    feature = "test-support",
    feature = "semantic-observe",
    feature = "soak-support"
))]
pub use stats::GraphWorkSnapshot;
pub(crate) mod tasks;
// `SemanticGraphStore`'s `#[doc(hidden)]` `*_for_tests` publish / probe
// helpers live in a sibling continuation-impl file so the hot-path memo
// logic here stays under the Tier-2 module-size budget.
pub(crate) mod store_test_support;
#[cfg(any(test, feature = "test-support"))]
pub use store_test_support::BatchExpandError;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_gates;
mod trait_impls;
// Test-only observability surface for `SemanticGraphStore` (in-flight
// abort driver, joiner-admission strong-count + condvar-pairing probes,
// per-store cold-abort trigger). Extracted to a sibling so the hot-path
// memo logic here stays under the Tier-2 module-size budget. Gated out of
// release: its only consumers are tests and the `for_tests` shims, both of
// which build with `debug_assertions` (test profile) or `cfg(test)`.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub use test_support::{
    empty_signature_for_tests, test_trigger_inflight_abort, TestForceColdAbortGuard,
};

#[allow(unused_imports)]
pub use interner::DepSignatureInterner;
#[cfg(any(test, feature = "test-support"))]
pub use member_index::MEMBER_ORDINAL_INDEX_LINEAR_SCAN_MAX;

use crate::semantic_query::demand::{
    cached_satisfies, MaterializedPoint, MaterializedSet, ProjectionPath,
};
use arena::NodeArena;
use derivation::DerivationStore;
pub use family::AuditEagerKeyRow;
use family::{
    carrier_facts_reference_canonical, family_and_slot, requested_path_for_key, CandidateSnapshot,
    FamilyKey, FamilySlots, MemoEntry, ModeSlot,
};
// Used only by the `#[cfg(any(test, feature = "test-support"))]` publish
// helpers; the production paths read the prepared token instead.
#[cfg(any(test, feature = "test-support"))]
use family::requested_point_for_key;
use inflight::FlightCell;
use prepared::PreparedKeyHandle;
use stats::{AtomicSemanticGraphStats, EntriesLockGuard};
use tasks::TaskRegistry;

#[cfg(any(test, feature = "test-support"))]
use test_gates::validate_running_probe;
#[cfg(any(test, feature = "test-support"))]
pub use test_gates::{ValidateRunningProbeGuard, VALIDATE_RUNNING_PROBE_TEST_LOCK};

/// Test-only: the stable variant label of the [`FamilyKey`] a
/// [`SemanticQueryKey`] maps to under `family_and_slot`. Lets the g_block
/// guards assert the family-domain mapping (e.g. that `Relate` maps to the
/// dedicated `FamilyKey::Relate`, never aliasing `IndexedAccess`) without
/// exposing the `pub(super)` `FamilyKey` taxonomy outside the crate.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn family_variant_label_for_tests(
    key: &crate::semantic_query::SemanticQueryKey,
) -> &'static str {
    let (family, _slot) = family_and_slot(key);
    family.variant_label()
}

fn narrow_value_result(result: QueryResult<SemanticQueryValue>) -> QueryResult<SemanticNodeId> {
    match result {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => QueryResult::Value(node),
        QueryResult::Value(other) => QueryResult::Error(QueryError::ValueDomainMismatch {
            expected: SemanticQueryValueTag::TypeNode,
            actual: other.tag(),
        }),
        QueryResult::Recursive(node) => QueryResult::Recursive(node),
        QueryResult::Error(error) => QueryResult::Error(error),
    }
}

fn narrow_cache_read(
    read: CacheRead<QueryResult<SemanticQueryValue>>,
) -> CacheRead<QueryResult<SemanticNodeId>> {
    CacheRead {
        lease: read.lease,
        value: narrow_value_result(read.value),
        dep_signature: read.dep_signature,
        walker_diagnostics: read.walker_diagnostics,
        cache_suppress: read.cache_suppress,
        fact_status: read.fact_status,
        receipt: read.receipt,
    }
}

pub(crate) fn cancelled_cache_read() -> CacheRead<QueryResult<SemanticQueryValue>> {
    crate::request_context::mark_request_result_cancelled();
    CacheRead {
        lease: Default::default(),
        value: QueryResult::Error(QueryError::Cancelled),
        dep_signature: empty_signature(),
        walker_diagnostics: empty_diagnostics(),
        cache_suppress: true,
        fact_status: crate::semantic_query::FactStatus::unavailable(
            crate::semantic_query::PartialReasonSet::CANCELLED,
        ),
        receipt: crate::semantic_query::ReadReceipt::Unpriced,
    }
}

/// The terminal read of a query whose key names an operand this graph does
/// not hold live: a destroyed node or a foreign ordinal. It is answered as
/// that typed error and never cached.
pub(crate) fn refused_operand_cache_read(
    error: QueryError,
) -> CacheRead<QueryResult<SemanticQueryValue>> {
    let fact_status = crate::semantic_query::FactStatus::Unavailable(
        crate::semantic_query::surface_resolution::NonEmptyReasons::from_query_error(&error),
    );
    CacheRead {
        lease: Default::default(),
        value: QueryResult::Error(error),
        dep_signature: empty_signature(),
        walker_diagnostics: empty_diagnostics(),
        cache_suppress: true,
        fact_status,
        receipt: crate::semantic_query::ReadReceipt::Unpriced,
    }
}

/// Test-only: `std::mem::size_of::<FamilyKey>()`. Lets the size-discipline
/// guard pin that the hot single-node `FamilyKey → FamilySlots` keyspace is NOT
/// inflated by embedding the ~144B `RelateMemoKey` by value — without exposing
/// the `pub(super)` `FamilyKey` taxonomy outside the crate. The `Relate` payload
/// must stay interned (see [`family::FamilyKey::Relate`]).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn family_key_size_for_tests() -> usize {
    std::mem::size_of::<family::FamilyKey>()
}

/// Test-only: `std::mem::size_of::<MemoEntry>()` for layout reporting.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[must_use]
pub fn memo_entry_size_for_tests() -> usize {
    std::mem::size_of::<family::MemoEntry>()
}

// ──────────────────────────────────────────────────────────────────────────
// (NodeArena moved to `arena.rs` — see that module for the structural-
// interning sharded dedup hot path.)
// ──────────────────────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────────────────────
// Semantic graph store
// ──────────────────────────────────────────────────────────────────────────

/// Host-owned semantic-query memo table + node arena. One instance per
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore).
///
/// This store alone does not execute queries — it is the cache substrate.
/// Concrete resolution happens inside a dispatcher that owns the solver /
/// resolver knowledge.
#[allow(dead_code)]
struct SemanticGraphIdentity(u64);

impl Default for SemanticGraphIdentity {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct SemanticGraphStore {
    /// Process-local identity used only to confine opaque runtime operands to
    /// the store that issued them.
    #[allow(dead_code)]
    operand_identity: SemanticGraphIdentity,
    arena: NodeArena,
    /// Epoch-safe signature records (candidates, descriptors, results).
    signatures: crate::signature_kernel::SignatureStore,
    /// Family-keyed warm memo.
    ///
    /// Each entry's [`FamilyKey`] is mode-erased; the per-mode result lives
    /// in one of the [`FamilySlots`] slots. For non-mode-bearing variants
    /// (`ResolveDecl`, `Instantiate`, `KeyOf`, etc.) the family is the
    /// variant itself and only the `single` slot is ever populated. For
    /// mode-bearing variants (`ProjectMember`, `IndexedAccess`,
    /// `ProjectPath`) the family carries the variant minus its mode field
    /// and the per-`ProjectionMode` slots hold independent results.
    ///
    /// **Materialised-record warm hit (§3.4):** each candidate carries a
    /// recorded `satisfied_projection` — the concrete `(path, point)` set
    /// its compute ACTUALLY produced, NOT its nominal slot mode. A warm hit
    /// requires TWO gates, BOTH passing: `cached_satisfies` (a RECORDED point
    /// dominates the request at the SAME path — EXACT, never prefix) AND
    /// per-candidate `read_set_signature.validate_with_self_roots`.
    ///
    /// **Backfill on completion:** a broader-projection build clones its
    /// entry — recorded set VERBATIM — into a projection-depth-narrower
    /// EMPTY sibling slot ONLY when a recorded point `cached_satisfies` the
    /// target's requested point (directional siblings — see
    /// [`family::slot_domain_siblings`]). Never by enum rank, so the
    /// lattice-unsound `Shallow → Navigate` clone is REJECTED. Narrower
    /// builds NEVER backfill broader slots, and only into an empty slot.
    ///
    /// **Relation judgements live here too**, under the dedicated
    /// [`FamilyKey::Relate`] family (the full-identity
    /// [`RelateMemoKey`](crate::semantic_query::RelateMemoKey) — source /
    /// target / relation kind / policy / source freshness / inference
    /// context / env+substitution+projection-reduction context — boxed, 1:1
    /// with the key the relation engine builds) in the [`ModeSlot::Single`]
    /// slot. The stored candidate's value is the PUBLIC
    /// [`SemanticQueryValue::Relation`] payload — decided binary
    /// `Assignable`/`NotAssignable` outcomes ONLY (an undecided judgement
    /// has no value-domain form and is never admitted; a `BudgetExceeded`
    /// payload is public but ReturnOnly). Warm reads validate the carrier
    /// strictly, so a same-canonical content edit to either the source's
    /// or the target's originating file — or a `ProjectGeneration` bump —
    /// misses the warm relation judgement via the carrier's
    /// `FactVersionRef::ProjectGeneration` fact and forces a recompute
    /// (`validated_at_generation` is recency metadata, not a validity
    /// gate). Retention rides the
    /// family rails (per-family cap, invalid-first / LRU eviction, the
    /// family `memo_budget` global bound, reverse-index drains).
    entries: Mutex<FxHashMap<FamilyKey, FamilySlots>>,
    /// In-flight admission keyed by the prepared query token
    /// ([`PreparedKeyHandle`]) whose equality IS full
    /// [`SemanticQueryKey`] equality (bijection pinned by the
    /// `prepared_identity_bijection` guards). Because mode is part of
    /// the key for mode-bearing variants, this keying gives
    /// per-`(family, mode_slot)` in-flight authority — concurrent
    /// `Navigate` and `Expanded` builds on the same family run as two
    /// independent in-flight entries. The handle additionally carries
    /// the prepared `(family, slot)` projection, so invalidation sweeps
    /// read it instead of re-running `family_and_slot` per entry.
    inflight: Mutex<FxHashMap<PreparedKeyHandle, Arc<FlightCell>>>,
    /// Expression demands share flights only with expression demands. Complete
    /// products publish to the same site-independent family memo.
    expression_inflight: Mutex<FxHashMap<PreparedKeyHandle, Arc<FlightCell>>>,
    /// Generation-qualified execution tasks and the wait-for graph between
    /// them. A task owns the producers it claimed; a subscriber registers a
    /// temporary edge to the producing task before it parks.
    task_registry: TaskRegistry,
    /// Independently-owned binding relation flights. Binding roots carry
    /// transaction-local inference candidates, so concurrent callers must
    /// never join one another's cold work. They still register here while
    /// building so cancellation and invalidation can revoke their publish
    /// right under the same store-owned admission fence as ordinary flights.
    /// The value is a list because multiple independent owners may compute
    /// the same full key concurrently.
    independent_inflight: Mutex<FxHashMap<PreparedKeyHandle, Vec<Arc<FlightCell>>>>,
    /// Sibling derivation/origin layer. Edges are keyed by
    /// `(result_node, kind)`; multiple derivations of the same structural
    /// result store multiple edges per key. Edge dep-signatures are
    /// interned in the store's signature pool so per-builder fence
    /// snapshots share allocations. Origin edges are bounded best-effort
    /// provenance for the audit origin-graph trace, NOT an invalidation
    /// source — see the `derivation` module docs.
    pub(super) derivation: Arc<release::SidecarMutex<DerivationStore>>,
    /// Lock-free telemetry counters. Read via [`Self::stats_snapshot`]
    /// into the public [`SemanticGraphStats`] surface.
    pub(super) stats: AtomicSemanticGraphStats,
    /// Optional contention instrumentation. Mirrors the arena's
    /// `provenance` field: `Some` for stores wired up by the host,
    /// `None` for test-default stores constructed via `Default`.
    /// Used by `execute_cooperative` to bucket owner vs joiner paths
    /// and held time on `EngineProvenance`.
    provenance: Option<Arc<crate::engine_provenance::EngineProvenance>>,
    /// The aggregate retained-byte account every candidate this memo
    /// publishes charges.
    ///
    /// There is no account-less memo store: the field carries a
    /// [`StoreAccount`](verter_session_query::retention::StoreAccount),
    /// whose `Default` is the ONE process-local account. A store built by
    /// `Default` / [`Self::new`] therefore charges the same ceiling a
    /// host-built store does, instead of retaining candidates that consume
    /// no aggregate headroom.
    retention_account: verter_session_query::retention::StoreAccount,
    /// The shared charges of the allocations this store's candidates and
    /// their readers alias — fact carriers, self-root lists, delivered
    /// rails and values — keyed by allocation, so sibling candidates share
    /// one charge per allocation and a reader that outlives its candidate
    /// keeps that allocation charged. Holds one registration per allocation
    /// still held, nothing more.
    alias_charges: verter_session_query::retention::shared::AliasCharges,
    /// Per-store test trigger for the cold-abort sweep path. When a test
    /// sets this (via [`Self::test_force_cold_abort_sweep`]), the
    /// cold-winner re-check in [`Self::warm_publish_one`] marks its own
    /// in-flight entry `aborted = true` just before the TOCTOU
    /// abort-check, simulating a concurrent canonical invalidation sweep
    /// — driving `cold_aborts_swept` deterministically without racing a
    /// real invalidation window.
    ///
    /// **Per-store scope (test hermeticity).** The flag lives on the
    /// store the test drives, not in a process-global. Rust runs a test
    /// binary's tests in parallel; a process-global trigger set by an
    /// abort-forcing test would also abort an unrelated concurrent
    /// test's `execute_cooperative` cold publish on its own store. A
    /// per-store flag affects only the store it is set on, so no test
    /// flipping the trigger can disturb a concurrent unrelated test.
    ///
    /// Default `false`. The production cold-publish path reads it once
    /// per cold publish — a single relaxed atomic load on a path that
    /// already takes the entries lock, so the cost is in the noise and
    /// production cold-abort-sweep behaviour is unchanged.
    force_cold_abort_sweep: std::sync::atomic::AtomicBool,
    /// Memoized `reaches an unresolved carrier` bit, per interned node id.
    ///
    /// The semantic graph is hash-consed and IMMUTABLE: a node's payload
    /// never changes after intern, and its children are interned before
    /// it. "This value reaches a carrier whose own resolution answered
    /// nothing" is therefore a pure INDUCTIVE function of the node id —
    /// computed once, from the children's bits, and true forever after.
    ///
    /// That is what makes the answer budget-free. A bounded walk that
    /// reports "unresolved" on exhaustion is not a conservative
    /// approximation of this function, it is a DIFFERENT function: a
    /// 4,100-arm literal union with no miss in it is fully known, and
    /// stamping it unresolved propagates a factually false permanent warm
    /// refusal into every enclosing result. Memoizing removes the reason
    /// the bound existed (re-walking the same structure per SCC fixpoint
    /// iteration) instead of trading correctness for it. ONE per-node sidecar:
    /// the entry holds every bit memoized for the id, released together.
    unresolved_reach: Arc<release::SidecarMutex<unresolved_reach::StructureBits>>,
    /// The `VerterStableV1` member view of each union built in this store's
    /// arena, keyed by the store's OWN node ids. Ownership, lifetime and the
    /// contract with a payload-retiring holder: `union_views.rs`.
    union_views: Mutex<union_views::UnionViews>,
    /// The stable-key classes of this store's nodes. Ownership, lifetime and
    /// release: [`crate::semantic_query::stable_key::KeyClasses`].
    key_classes: Arc<release::SidecarMutex<crate::semantic_query::stable_key::KeyClasses>>,
    /// Test-only: order union members by DESCENDING stable key. Reversing the
    /// one union order in a fresh store — an isolated cache namespace, its
    /// views and memo entries included — is the §5.8 counterfactual that
    /// admits an order-only difference as `VerterStableV1`-induced.
    #[cfg(any(test, feature = "test-support"))]
    union_order_reversed_for_tests: std::sync::atomic::AtomicBool,
    /// Per-store test-only injection point for the
    /// [`Self::invalidate_all`] post-`entries`-clear tail. When a test
    /// arms it (via [`Self::test_invalidate_all_post_entries_clear_gate`])
    /// `invalidate_all` calls [`std::sync::Barrier::wait`] on the stored
    /// barrier right after releasing the `entries` lock that performed
    /// the in-flight abort + memo clear — letting a test deterministically
    /// race a cold winner's `warm_publish_one` against that tail and
    /// assert no stale memo entry survives.
    ///
    /// **Per-store scope (test hermeticity).** Like `force_cold_abort_sweep`,
    /// the gate lives on the store the test drives, never in a
    /// process-global, so a test arming it cannot park an unrelated
    /// concurrent test's `invalidate_all`.
    ///
    /// `cfg`-gated to `test` / `debug_assertions`: the field and the
    /// `invalidate_all` probe are both absent from release builds, so the
    /// production reset path is unchanged.
    #[cfg(any(test, feature = "test-support"))]
    invalidate_all_post_entries_clear_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside [`Self::invalidate_all`],
    /// fired immediately before the `memo_budget` clear — and, in
    /// final-state code, while the `entries` lock that performed the
    /// `entries.clear()` is STILL held. A race test arms it (via
    /// [`Self::test_invalidate_all_pre_memo_budget_clear_gate`]) and, with
    /// `invalidate_all` parked here, asserts `entries.try_lock()` is
    /// `None`: the `entries` + `memo_budget` clears run in one lock
    /// domain, so a concurrent publisher cannot strand a live `entries`
    /// family with no `memo_budget` ledger record.
    ///
    /// **Per-store scope (test hermeticity).** Like
    /// `invalidate_all_post_entries_clear_gate`, the gate lives on the
    /// store the test drives, never in a process-global.
    ///
    /// `cfg`-gated to `test` / `debug_assertions`; absent from release
    /// builds, so the production reset path is unchanged.
    #[cfg(any(test, feature = "test-support"))]
    invalidate_all_pre_memo_budget_clear_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside the warm-slot publish
    /// path ([`Self::warm_publish_one`] / [`Self::warm_publish_one_if_absent`]
    /// / [`Self::publish_with_carrier_for_tests`]), fired right after the
    /// family `entries` publish and the `memo_budget` admission land — and,
    /// in final-state code, while the `entries` lock is STILL held. A race
    /// test arms it (via [`Self::test_publish_post_memo_budget_record_gate`])
    /// and, with a publisher parked here, asserts `entries.try_lock()` is
    /// `None`: the `entries` publish and the `memo_budget` admission run in
    /// one lock domain, so a concurrent `invalidate_all` cannot erase the
    /// ledger record for the family this publish just landed.
    ///
    /// **Per-store scope (test hermeticity).** Per-store, never a
    /// process-global. `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    publish_post_memo_budget_record_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside
    /// [`Self::execute_cooperative`]'s cold-winner path, fired AFTER
    /// [`Self::warm_publish_one`] published the parent entry
    /// (`published == true`) and BEFORE the prefix-backfill loop runs.
    /// A race test arms it (via [`Self::test_cold_winner_pre_backfill_gate`])
    /// and, with the winner parked here, runs `invalidate_all` so the
    /// winner's in-flight entry is marked `aborted`; when the winner is
    /// released its prefix-backfill loop must skip every backfill (the
    /// abort fence in `warm_publish_one_if_absent`).
    ///
    /// **Per-store scope (test hermeticity).** Per-store, never a
    /// process-global. `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    cold_winner_pre_backfill_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside
    /// [`Self::execute_cooperative`]'s cold-winner path, fired immediately
    /// after [`Self::warm_publish_one`] admits the parent candidate and before
    /// prefix backfill begins. A test can cancel the owning request in this
    /// exact window and verify that the completed admission remains the
    /// operation's linearization point.
    #[cfg(any(test, feature = "test-support"))]
    cold_winner_post_admission_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside cancellation abort, after
    /// joiners are notified and before the cancelled owner's flight is
    /// retired. A test can hold the owner in this exact window and prove a
    /// live follower makes progress without repeatedly rejoining the aborted
    /// flight.
    #[cfg(any(test, feature = "test-support"))]
    cancellation_abort_pre_retire_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside [`Self::invalidate_all`],
    /// fired right BEFORE the `canonical_to_entries` reverse-index clear —
    /// and, in final-state code, with the `entries` lock STILL held. A
    /// race test arms it (via
    /// [`Self::test_invalidate_all_pre_reverse_index_clear_gate`]) and,
    /// with `invalidate_all` parked here, asserts `entries.try_lock()` is
    /// `None`: the reverse-index clear runs in the SAME `entries` lock
    /// domain as the `entries` + `memo_budget` clears, so a concurrent
    /// publisher cannot register into `canonical_to_entries` between the
    /// `entries` clear and the reverse-index clear (which would strand a
    /// live memo entry with no reverse-index registration, or a
    /// registration with no entry).
    ///
    /// **Per-store scope (test hermeticity).** Per-store, never a
    /// process-global. `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    invalidate_all_pre_reverse_index_clear_gate:
        parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside
    /// [`Self::record_family_admission_locked`], fired right AFTER the
    /// FIFO budget-eviction victims' `canonical_to_entries` reverse-index
    /// registrations are pruned — and, in final-state code, with the
    /// `entries` lock STILL held. A race test arms it (via
    /// [`Self::test_publish_post_reverse_index_prune_gate`]) and, with the
    /// publisher parked here, asserts `entries.try_lock()` is `None`: the
    /// FIFO victim's reverse-index pruning runs in the SAME `entries` lock
    /// domain as the `entries` removal + `memo_budget` eviction, so a
    /// concurrent fresh same-`(family, slot)` re-publish cannot register
    /// into `canonical_to_entries` between the victim's `entries` removal
    /// and the victim's reverse-index pruning (which would delete the
    /// fresh registration, leaving the live re-published memo slot
    /// invisible to `invalidate_canonical`).
    ///
    /// **Per-store scope (test hermeticity).** Per-store, never a
    /// process-global. `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    publish_post_reverse_index_prune_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point immediately before an inline
    /// relation member takes the entries lock for publication.
    #[cfg(any(test, feature = "test-support"))]
    relation_member_pre_entries_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point after a relation root publishes
    /// and before its transaction drains completed SCC members.
    #[cfg(any(test, feature = "test-support"))]
    relation_root_pre_member_drain_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside [`Self::invalidate_all`]'s
    /// in-flight abort loop, fired while iterating the COLLECTED entry
    /// handles and locking each entry's `state` — with the `inflight`
    /// table lock NOT held. A race test arms it (via
    /// [`Self::test_invalidate_all_inflight_abort_gate`]) and asserts
    /// `inflight.try_lock()` is `Some`, proving the collect-then-release
    /// lock order that keeps the abort loop within the module's global
    /// rule (`state` is never taken while the `inflight` table lock is
    /// held). Per-store scoped; `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    invalidate_all_inflight_abort_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside
    /// [`Self::invalidate_canonical`]'s in-flight abort loop, fired while
    /// iterating the COLLECTED entry handles and locking each entry's
    /// `state` — with the `inflight` table lock NOT held. A race test arms
    /// it (via [`Self::test_invalidate_canonical_inflight_abort_gate`]) and
    /// asserts `inflight.try_lock()` is `Some`, proving `invalidate_canonical`
    /// honours the same collect-then-release lock order as
    /// [`Self::invalidate_all`]. Per-store scoped; `cfg`-gated to `test` /
    /// `debug_assertions`.
    #[cfg(any(test, feature = "test-support"))]
    invalidate_canonical_inflight_abort_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only counter: incremented by one IMMEDIATELY before
    /// a cooperative joiner blocks on the per-entry `ready` condvar via
    /// `wait_while` in [`Self::execute_cooperative`]. Unlike the in-flight
    /// `Arc` strong count (which rises when a joiner merely *clones* the
    /// entry, one step BEFORE it reaches the condvar), this counter proves
    /// the joiner is genuinely SUSPENDED on the condvar — the precise
    /// invariant a condvar-pairing test asserts. Read via
    /// [`Self::test_joiner_on_condvar_count`].
    ///
    /// **Per-store scope (test hermeticity).** Like every other gate on
    /// this store, the counter is per-store, never a process-global, so a
    /// condvar-pairing test on one store cannot observe an unrelated
    /// concurrent test's joiners. `cfg`-gated to `test` / `debug_assertions`;
    /// the increment and the field are both absent from release builds, so
    /// the production cooperative-wait path is unchanged.
    #[cfg(any(test, feature = "test-support"))]
    joiner_on_condvar_count: std::sync::atomic::AtomicUsize,
    /// Direct-edge reverse index: each canonical names the evidence
    /// (carriers and receipts, cached or not) whose own facts read
    /// it, each consumed receipt names the evidence that consumed it, and
    /// each carrier names the candidates it validates. A candidate also
    /// registers under its dispatch-fence canonicals.
    /// `invalidate_canonical` walks it up from the canonical instead of
    /// scanning the family memo; it drives eviction only, never validity.
    /// A candidate is registered by the publish that makes it visible,
    /// under the same `entries` lock hold.
    ///
    /// **Lock order — `entries → canonical_to_entries`.** The family
    /// `entries` `Mutex` is OUTERMOST; the index's own lock is a leaf. The
    /// publish-side registration, the [`Self::invalidate_all`] clear,
    /// the FIFO budget-eviction cleanup, and the
    /// [`Self::invalidate_canonical`] drain all mutate
    /// `canonical_to_entries` WHILE holding `entries`, which makes the
    /// family memo's three-member consistency cluster (`entries`,
    /// `memo_budget`, `canonical_to_entries`) mutate atomically. The index
    /// never takes another lock while it holds its own.
    canonical_to_entries: CanonicalToEntries,
    /// Store-owned hash-cons pool for dispatch dependency signatures.
    /// Every warm candidate passes through this pool before admission, so
    /// equivalent signatures share one allocation while live candidates keep
    /// the pool's weak entries valid. Content equality, not pointer identity,
    /// remains the semantic authority; candidate admission sequence identifies
    /// individual reverse-index registrations.
    dep_signature_interner: DepSignatureInterner,
    /// The name every project-generation dependency entry of this store
    /// carries, minted with the store and pinned to its account once: each
    /// entry shares the one allocation and its charge rather than copying
    /// the text per signature ([`Self::project_dep_entry`]).
    project_dep_name: verter_type_expr::OwnedStr,
    /// Global insertion-ordered total-size budget for the family memo.
    /// Each `FamilyKey` is built from content-derived `SemanticNodeId`s
    /// / a `DeclIdentity` embedding the file whole-hash, so a content
    /// edit produces fresh families. The reverse-index drain reclaims
    /// only on per-canonical invalidation, which an owner-content edit
    /// no longer triggers — this budget is the routine reclamation:
    /// publishing a newly-keyed family records an admission and the
    /// oldest families past the cap are FIFO-evicted write-side.
    ///
    /// **Consistency-cluster fence.** The family memo's three members —
    /// `entries` (the memo map), this budget, and `canonical_to_entries`
    /// (the reverse index) — are ALL mutated within ONE lock domain, the
    /// `entries` `Mutex`. Every publish records the family admission AND
    /// registers the reverse index while holding the `entries` lock that
    /// landed the slot; every reset ([`Self::invalidate_all`]) and
    /// per-canonical drain ([`Self::invalidate_canonical`]) mutates this
    /// budget AND `canonical_to_entries` while holding that same lock. A
    /// `clear` therefore cannot interleave with a concurrent publish, so
    /// the budget never strands a live `entries` family with no admission
    /// record (which would make the family invisible to FIFO eviction and
    /// break the cap) and the reverse index never strands a live entry
    /// with no registration. The `entries → canonical_to_entries` lock
    /// order permits taking the index's leaf lock while `entries` is held.
    memo_budget: crate::bounded_query_retention::GlobalRetentionBudget<FamilyKey>,
    /// Monotonic per-candidate admission-sequence allocator. Each
    /// `MemoEntry` carries a unique seq so the
    /// `(FamilyKey, ModeSlot, seq)` reverse-index registration is
    /// per-candidate — a cross-canonical cleanup for one evicted
    /// candidate strips only that candidate's seq, leaving siblings'
    /// registrations intact.
    candidate_admission_seq: std::sync::atomic::AtomicU64,
    // Hash-cons substitution + evaluation result memos — accessors
    // and invalidation contract live in `hash_cons_memos.rs`. Both
    // dedupe identical structural keys reaching
    // `substitute_semantic_type_param` and
    // `evaluate_deferred_semantic_node_with_context` so the mapped-
    // type per-K materialiser does not recompute K-independent
    // subtrees on every iteration. Both are bounded by
    // `HASH_CONS_MEMO_RETENTION_CAP` (FIFO eviction via the
    // sidecar deque) so a long-running workspace cannot grow either
    // memo without bound; see `hash_cons_memos.rs` for the
    // eviction contract. Their keys are arena-allocated node ids and
    // closed internal enums, never authored text, so they hash with
    // `FxHash`: no input can choose colliding keys.
    pub(super) structural_domains: DashMap<
        SemanticNodeId,
        structural_domains::StructuralDomainEntry,
        rustc_hash::FxBuildHasher,
    >,
    pub(super) structural_domains_fifo:
        parking_lot::Mutex<std::collections::VecDeque<SemanticNodeId>>,
    pub(super) binder_dependencies: DashMap<
        SemanticNodeId,
        binder_dependencies::BinderDependencyEntry,
        rustc_hash::FxBuildHasher,
    >,
    pub(super) binder_dependencies_fifo:
        parking_lot::Mutex<std::collections::VecDeque<SemanticNodeId>>,
    pub(super) substitute_memo: DashMap<
        (SemanticNodeId, SemanticNodeId, SemanticNodeId),
        hash_cons_memos::SubstituteMemoEntry,
        rustc_hash::FxBuildHasher,
    >,
    pub(super) substitute_memo_fifo: parking_lot::Mutex<
        std::collections::VecDeque<(SemanticNodeId, SemanticNodeId, SemanticNodeId)>,
    >,
    pub(super) evaluate_deferred_memo: DashMap<
        (
            SemanticNodeId,
            crate::semantic_query::ProjectionReductionContext,
            crate::project_semantic_dispatch::evaluate::DeferredEvaluationDemand,
        ),
        Arc<hash_cons_memos::DeferredEvaluationMemoEntry>,
        rustc_hash::FxBuildHasher,
    >,
    pub(super) evaluate_deferred_memo_fifo: parking_lot::Mutex<
        std::collections::VecDeque<(
            SemanticNodeId,
            crate::semantic_query::ProjectionReductionContext,
            crate::project_semantic_dispatch::evaluate::DeferredEvaluationDemand,
        )>,
    >,
    // Member-ordinal index sidecar for large interned `Object` surfaces —
    // accessor, retention contract, and hashing rationale live in
    // `member_index.rs`. IDENTITY-EXCLUDED: keys on the append-only
    // `SemanticNodeId` (payloads immutable, ids never reused), so entries
    // never go stale and no invalidation hook exists. The outer `DashMap`
    // and the inner name map both use the collision-safe std default
    // hasher (member names are authored strings — never FxHash).
    pub(super) member_ordinal_index_memo:
        Arc<DashMap<SemanticNodeId, Arc<member_index::MemberOrdinalIndex>>>,
    pub(super) member_ordinal_index_fifo:
        Arc<release::SidecarMutex<std::collections::VecDeque<SemanticNodeId>>>,
    /// Sealed refusals of isolated root demands — see `refusal_summary`.
    pub(super) refusal_summaries: refusal_summary::RefusalSummaryTable,
    /// Test-only: run by a dispatch's synchronous entry once it holds a
    /// key's producer claim, before the producer builds — see
    /// [`Self::set_produce_hook_for_tests`].
    #[cfg(any(test, feature = "test-support"))]
    pub(super) produce_hook: parking_lot::Mutex<Option<test_support::ProduceHookForTests>>,
    /// Per-store, scoped forcing of the exhausted subscriber delivery boundary.
    #[cfg(any(test, feature = "test-support"))]
    exhausted_join_abort:
        parking_lot::Mutex<Option<(SemanticQueryKey, test_support::ExhaustedJoinAbortForTests)>>,
    /// The rendezvous scopes armed on this store, outermost first — see
    /// [`Self::arm_flight_rendezvous`]. A guard's drop removes exactly its
    /// own registration, so overlapping scopes may end in any order.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    flight_rendezvous: parking_lot::Mutex<Vec<Arc<inflight::FlightRendezvousShared>>>,
    /// How many rendezvous guards are live on this store right now — the
    /// lock-free miss path of [`Self::armed_flight_rendezvous`], so a store
    /// with nothing armed never takes the table's mutex from a cold build
    /// or a joined wait.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    flight_rendezvous_armed: std::sync::atomic::AtomicUsize,
    /// The arena's teardown. Declared last: it ends after every cache above
    /// has released what it held ([`arena::ArenaTeardown`]).
    arena_teardown: arena::ArenaTeardown,
}

/// The teardown begins before any field drops, so the caches' root
/// releases defer to [`SemanticGraphStore::arena_teardown`].
impl Drop for SemanticGraphStore {
    fn drop(&mut self) {
        self.arena_teardown.begin();
    }
}

/// The family memo's retention bound, chosen when a store is constructed
/// ([`SemanticGraphStore::with_memo_family_bound`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoFamilyBound {
    /// The production bound,
    /// [`crate::bounded_query_retention::DEFAULT_BUDGET_CAP`] families.
    #[default]
    Production,
    /// Test support: retain at most this many families, through the same
    /// FIFO eviction as the production bound, so a test overruns the memo
    /// without first admitting the production bound's worth of families.
    #[cfg(any(test, feature = "test-support"))]
    ForTest(usize),
}

impl Default for SemanticGraphStore {
    fn default() -> Self {
        Self::with_arena(Default::default(), Default::default())
    }
}

/// Optional telemetry follows the selected execution mode. Test harnesses
/// keep work accounting independently of request capture.
#[inline(always)]
pub(super) const fn records_telemetry<M: verter_audit::observe::ObserveExec>() -> bool {
    cfg!(any(test, feature = "test-support")) || M::CAPTURED
}

/// Only the selected work counters count independently of capture in tooling.
#[inline(always)]
pub(super) const fn records_work<M: verter_audit::observe::ObserveExec>() -> bool {
    cfg!(feature = "soak-support") || records_telemetry::<M>()
}

impl AsRef<SemanticGraphStore> for SemanticGraphStore {
    fn as_ref(&self) -> &SemanticGraphStore {
        self
    }
}

impl SemanticGraphStore {
    /// A project-generation dependency entry at `version`: its name is this
    /// store's one `"<project>"` allocation, shared by every such entry, so
    /// a signature that records only the project generation copies no text.
    ///
    /// The store mints the name with itself and pins its cell to the
    /// store's account, so the allocation is charged once, as the store's
    /// own storage, until the store and the last entry naming it drop. A
    /// cache retaining an entry holds the same cell, as it would any owned
    /// name.
    pub(crate) fn project_dep_entry(
        &self,
        version: crate::semantic_query::DepVersion,
    ) -> crate::semantic_query::DepEntry {
        crate::semantic_query::DepEntry(self.project_dep_name.clone(), version)
    }

    /// Bytes the store's project dependency name is pinned for while the
    /// store alone charges it; a cache retaining an entry holds that charge
    /// instead while it does.
    pub(super) fn project_dep_name_pinned_bytes(&self) -> usize {
        // Bytes and class read together: a cache hold or release between two
        // reads would count the name in neither class or in both.
        match self.project_dep_name.charge() {
            Some((bytes, verter_session_query::retention::ChargeClass::Pinned)) => bytes,
            _ => 0,
        }
    }

    /// The one store constructor: the store and its graph storage charge
    /// the same account, bound before the first node is interned. Binds
    /// signature-token ownership to the final arena before publishing the
    /// store.
    pub(super) fn with_arena(
        storage: NodeArena,
        retention_account: verter_session_query::retention::StoreAccount,
    ) -> Self {
        let arena = storage;
        arena.core.bind_retention_account(retention_account.get());
        let signatures = crate::signature_kernel::SignatureStore::default();
        signatures.bind_graph(&arena.core);
        signatures.bind_retention_account(retention_account.get());
        let canonical_to_entries = CanonicalToEntries::charged(retention_account.get());
        let refusal_summaries =
            refusal_summary::RefusalSummaryTable::charged(retention_account.get());
        let dep_signature_interner = DepSignatureInterner::charged(retention_account.get());
        let project_dep_name = verter_type_expr::OwnedStr::from("<project>");
        project_dep_name.pin(verter_session_query::retention::owned::ChargeCtx::new(
            retention_account.get(),
        ));
        let unresolved_reach = Arc::new(release::SidecarMutex::new(
            unresolved_reach::StructureBits::new(retention_account.get()),
        ));
        let arena_teardown = arena::ArenaTeardown::of(&arena);
        let store = Self {
            operand_identity: Default::default(),
            arena,
            signatures,
            entries: Default::default(),
            inflight: Default::default(),
            expression_inflight: Default::default(),
            task_registry: Default::default(),
            independent_inflight: Default::default(),
            derivation: Arc::new(release::SidecarMutex::new(DerivationStore::charged(
                retention_account.get(),
            ))),
            stats: Default::default(),
            provenance: Default::default(),
            retention_account,
            alias_charges: Default::default(),
            force_cold_abort_sweep: Default::default(),
            unresolved_reach,
            union_views: Default::default(),
            key_classes: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            union_order_reversed_for_tests: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            invalidate_all_post_entries_clear_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            invalidate_all_pre_memo_budget_clear_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            publish_post_memo_budget_record_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            cold_winner_pre_backfill_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            cold_winner_post_admission_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            cancellation_abort_pre_retire_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            invalidate_all_pre_reverse_index_clear_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            publish_post_reverse_index_prune_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            relation_member_pre_entries_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            relation_root_pre_member_drain_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            invalidate_all_inflight_abort_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            invalidate_canonical_inflight_abort_gate: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            joiner_on_condvar_count: Default::default(),
            canonical_to_entries,
            dep_signature_interner,
            project_dep_name,
            memo_budget: Default::default(),
            candidate_admission_seq: Default::default(),
            structural_domains: Default::default(),
            structural_domains_fifo: Default::default(),
            binder_dependencies: Default::default(),
            binder_dependencies_fifo: Default::default(),
            substitute_memo: Default::default(),
            substitute_memo_fifo: Default::default(),
            evaluate_deferred_memo: Default::default(),
            evaluate_deferred_memo_fifo: Default::default(),
            member_ordinal_index_memo: Default::default(),
            member_ordinal_index_fifo: Default::default(),
            refusal_summaries,
            #[cfg(any(test, feature = "test-support"))]
            produce_hook: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            exhausted_join_abort: Default::default(),
            #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
            flight_rendezvous: Default::default(),
            #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
            flight_rendezvous_armed: Default::default(),
            arena_teardown,
        };
        store.bind_destroyed_sidecar_sink();
        store
    }
}

/// Exact memo candidate admitted by one cold-winner publication.
///
/// The `admission_seq` is the store-owned ABA token for this candidate; the
/// remaining fields are the fact carrier that downstream atomic admissions
/// must inherit from this exact publication.
#[derive(Debug, Clone)]
pub struct PublishedMemoCandidate {
    pub read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature,
    pub self_root_canonicals: Arc<[Arc<str>]>,
    pub validated_at_generation: u64,
    pub admission_seq: u64,
    /// The receipt of the build that published the candidate: an SCC
    /// component's members share it.
    pub cost_receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
}

/// Non-identity runtime evidence retained with a store-local node handle.
///
/// The store owns the fields: `pub(in crate::semantic_query_memo)` makes
/// this module tree the ONE struct-literal producer (the capture path
/// below, reached solely through a capture minted by the token-gated
/// [`ReadCapture::operand_evidence`]).
/// The forcing authority combines evidence only through the token-gated
/// [`Self::seal`]; everywhere else the fields are unreadable private state
/// and the value can be carried but never fabricated.
#[derive(Debug, Clone)]
pub struct SemanticOperandEvidence {
    pub(in crate::semantic_query_memo) read_set:
        verter_session_query::facts::fact_cache::ReadSetSignature,
    pub(in crate::semantic_query_memo) self_roots: Arc<[ObservedGraphSelfRoot]>,
    pub(in crate::semantic_query_memo) dep_signatures: Arc<[DepSignature]>,
}

impl SemanticOperandEvidence {
    /// Forcing-authority construction: the seal's substitution fold and the
    /// force's evidence union. Token-gated on the unforgeable
    /// [`SemanticOperandAuthority`](crate::project_semantic_dispatch::SemanticOperandAuthority)
    /// so no consumer outside the forcing boundary can fabricate an
    /// evidence set and hand it to a mint.
    pub(crate) fn seal(
        read_set: verter_session_query::facts::fact_cache::ReadSetSignature,
        self_roots: Arc<[ObservedGraphSelfRoot]>,
        dep_signatures: Arc<[DepSignature]>,
        _authority: &crate::project_semantic_dispatch::SemanticOperandAuthority,
    ) -> Self {
        Self {
            read_set,
            self_roots,
            dep_signatures,
        }
    }

    pub fn read_set(&self) -> &verter_session_query::facts::fact_cache::ReadSetSignature {
        &self.read_set
    }

    pub fn self_roots(&self) -> &[ObservedGraphSelfRoot] {
        &self.self_roots
    }

    pub(crate) fn dep_signatures(&self) -> &[DepSignature] {
        &self.dep_signatures
    }
}

/// Reconstruct the operand evidence for one warm/cold read from the
/// candidate's fact carrier and its LISTED self-root canonicals.
///
/// Fail-closed on root reconstruction: a listed self-root canonical with
/// no matching `FileWholeHash` fact on the carrier yields `None` — the
/// force's typed incomplete refusal — never a silently SHRUNK root set.
/// A shrunk set would validate at mint/force and let a
/// `mint -> force -> mint` chain drop a producer root, serving
/// stale-complete after a dead-operand edit. Every fact of the carrier is
/// read, so a wide carrier finds its roots like a narrow one.
pub fn semantic_operand_evidence(
    read_set: &verter_session_query::facts::fact_cache::ReadSetSignature,
    self_root_canonicals: &[Arc<str>],
    dep_signature: &DepSignature,
) -> Option<crate::semantic_query::operand::SemanticOperandEvidence> {
    let mut self_roots = Vec::with_capacity(self_root_canonicals.len());
    for canonical in self_root_canonicals {
        let found = read_set.entries().find_map(|fact| match fact {
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                canonical_id,
                hash,
            } if **canonical_id == **canonical => {
                Some((verter_type_expr::OwnedStr::from(canonical), *hash))
            }
            _ => None,
        });
        let root = found?;
        self_roots.push(root);
    }
    Some(crate::semantic_query::operand::SemanticOperandEvidence {
        read_set: read_set.clone(),
        self_roots: Arc::from(self_roots.into_boxed_slice()),
        dep_signatures: Arc::from([dep_signature.clone()]),
    })
}

pub enum WarmPublishOutcome {
    Published(PublishedMemoCandidate),
    Skipped,
    Aborted,
}

/// The family memo's direct-edge reverse index. See
/// [`SemanticGraphStore::canonical_to_entries`] for the contract.
use reverse_index::CanonicalToEntries;

impl std::fmt::Debug for SemanticGraphStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticGraphStore")
            .field("nodes", &self.arena.live_len())
            .field("node_ids", &self.arena.len())
            .field("node_storage_slots", &self.arena.storage_slots())
            .field("memo_entries", &self.memo_entry_count())
            .finish_non_exhaustive()
    }
}

impl SemanticGraphStore {
    /// The store's epoch-safe signature records.
    #[must_use]
    pub fn signature_store(&self) -> &crate::signature_kernel::SignatureStore {
        &self.signatures
    }

    #[allow(dead_code)]
    pub(crate) fn operand_store_identity(&self) -> u64 {
        self.operand_identity.0
    }

    /// Run a test body with `key`'s producer open on the current task, the
    /// same authority a claim consults for same-path recursion.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_same_path_inflight_for_test<T>(
        &self,
        key: SemanticQueryKey,
        body: impl FnOnce() -> T,
    ) -> T {
        use tasks::SemanticProducers as _;
        let execution = self.enter_execution();
        let _open = execution.task().open_producer(
            self.prepare_rooted_key(key),
            crate::semantic_query::checker_policy::TupleSplicePosition::Type,
        );
        body()
    }

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint an explicit fixture-only infer identity. Production lowering has
    /// no graph allocator: authored identities come from the shared exact
    /// typed-root [`crate::semantic_query::InferBinderFactory`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub(crate) fn synthetic_infer_binder_id_for_tests(
        &self,
        ordinal: u64,
    ) -> crate::semantic_query::InferBinderId {
        crate::semantic_query::InferBinderFactory::synthetic(ordinal)
    }

    /// Compatibility mint for older in-crate fixtures. This is compiled out
    /// of production and delegates exclusively to the private synthetic
    /// identity variant; authored lowering never reaches this counter.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn alloc_infer_binder_id(&self) -> crate::semantic_query::InferBinderId {
        static NEXT_SYNTHETIC_FIXTURE_ID: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let ordinal = NEXT_SYNTHETIC_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        self.synthetic_infer_binder_id_for_tests(ordinal)
    }

    /// Test-only constructor pinning a small `memo_budget` cap. Lets a
    /// reverse-index test drive FIFO family eviction without admitting
    /// the production [`crate::bounded_query_retention::DEFAULT_BUDGET_CAP`]
    /// (4096) families. Every other field is the `Default` value.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn new_with_memo_budget_for_test(memo_budget_cap: usize) -> Self {
        Self::default().with_memo_family_bound(MemoFamilyBound::ForTest(memo_budget_cap))
    }

    /// Bound this store's family memo at construction. Consumes the
    /// store before any caller can share it, so the bound is fixed before
    /// the first admission and never changes afterwards.
    #[must_use]
    pub fn with_memo_family_bound(
        #[cfg_attr(not(any(test, feature = "test-support")), allow(unused_mut))] mut self,
        bound: MemoFamilyBound,
    ) -> Self {
        match bound {
            MemoFamilyBound::Production => {}
            #[cfg(any(test, feature = "test-support"))]
            MemoFamilyBound::ForTest(cap) => {
                self.memo_budget = crate::bounded_query_retention::GlobalRetentionBudget::new(cap);
            }
        }
        self
    }

    /// Test-only — number of canonicals currently resident in the
    /// `canonical_to_entries` reverse index: those some registered
    /// receipt or candidate reads directly. A canonical leaves with the
    /// last node reading it, so this count tracks the surviving
    /// canonicals — not the lifetime canonical count.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn canonical_to_entries_shard_count_for_test(&self) -> usize {
        self.canonical_to_entries.occupancy().canonicals
    }

    /// Diagnosis accessor: number of distinct interned `DepSignature`
    /// payloads still reachable from a live origin edge in the
    /// derivation-signature pool. The pool stores `Weak` values whose
    /// lifetime is tied to the edges that reference them, so this counts
    /// only the entries that still upgrade — a dead `Weak` left behind by
    /// an evicted edge bucket is not counted. Used by the diagnosis
    /// benchmark to record the pool's growth across scenarios —
    /// `record_signature_pool_size` on the active capture token reads
    /// this value at end-of-capture.
    #[must_use]
    pub fn derivation_signature_pool_size(&self) -> usize {
        self.derivation
            .lock()
            .signature_pool
            .values()
            .flatten()
            .filter(|weak| weak.strong_count() > 0)
            .count()
    }

    /// Diagnosis-instrumented entries-mutex acquisition.
    ///
    /// Returns a [`parking_lot::MutexGuard`] for `self.entries` while
    /// timing both the wait (lock-acquisition latency) and the hold
    /// (lifetime of the returned guard) under the active capture
    /// token, if any. The hooks are no-ops when no token is bound,
    /// and the timing reads themselves are constant-time.
    ///
    /// Production callers acquired this lock via `self.entries.lock()`
    /// directly; this helper preserves the same contract while
    /// surfacing per-acquisition cost to the diagnosis benchmark.
    fn entries_lock_diagnosed<'a>(
        &'a self,
    ) -> EntriesLockGuard<'a, FxHashMap<FamilyKey, FamilySlots>> {
        // Wait-time measurement feeds the capture-token entries-mutex
        // hook only; gated to match the instrumentation module (absent
        // in release).
        #[cfg(any(test, feature = "test-support"))]
        let wait_start = Instant::now();
        let guard = self.entries.lock();
        #[cfg(any(test, feature = "test-support"))]
        let wait_ns = wait_start.elapsed().as_nanos();
        EntriesLockGuard {
            guard: Some(guard),
            #[cfg(any(test, feature = "test-support"))]
            hold_start: Instant::now(),
            #[cfg(any(test, feature = "test-support"))]
            wait_ns,
        }
    }

    /// Allocate the next per-candidate admission sequence — the
    /// store-assigned identity each new [`MemoEntry`] carries. Used
    /// to key `canonical_to_entries` per-candidate so a cross-canonical
    /// cleanup for one evicted candidate doesn't strip a sibling's
    /// registrations.
    fn alloc_candidate_admission_seq(&self) -> u64 {
        self.candidate_admission_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Public read accessor for the shared
    /// [`crate::engine_provenance::EngineProvenance`] handle the store was constructed
    /// with. Returns `None` for `Default`-built stores (test-default
    /// path); host-built stores always return `Some`.
    /// `meta_resolve::slot_binding_graph` reaches the
    /// `slot_binding_graph_*` counters through this accessor without
    /// threading a `&VerterHost` reference through every helper
    /// signature.
    #[must_use]
    pub fn provenance(&self) -> Option<&Arc<crate::engine_provenance::EngineProvenance>> {
        self.provenance.as_ref()
    }

    /// Run `f` over this store's stable-key class table.
    pub(crate) fn with_key_classes<R>(
        &self,
        f: impl FnOnce(&mut crate::semantic_query::stable_key::KeyClasses) -> R,
    ) -> R {
        let (result, fresh) = {
            let mut classes = self.key_classes.lock();
            let result = f(&mut classes);
            (result, std::mem::take(&mut classes.fresh))
        };
        let stale: Vec<SemanticNodeId> = fresh
            .into_iter()
            .filter(|node| !self.track_sidecar(*node))
            .collect();
        if !stale.is_empty() {
            self.key_classes
                .lock()
                .release_nodes(|node| stale.contains(&node));
        }
        result
    }

    /// Test-only — number of distinct `FamilyKey`s currently resident in
    /// the warm `entries` memo. The `memo_budget` retention ledger is
    /// keyed by family, so the map/budget lifecycle-fence tests compare
    /// this against [`Self::memo_budget_tracked_len_for_test`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn memo_family_count_for_test(&self) -> usize {
        self.entries.lock().len()
    }

    /// Test-only — number of admission records currently in the
    /// `memo_budget` retention ledger. With the map/budget lifecycle
    /// fence in place this stays consistent with
    /// [`Self::memo_family_count_for_test`]: every live `entries` family
    /// has exactly one ledger record, so a desync (a live family with no
    /// record, invisible to FIFO eviction) is observable as a mismatch.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn memo_budget_tracked_len_for_test(&self) -> usize {
        self.memo_budget.tracked_len()
    }

    /// Test-only — the family memo's retention bound: the number of
    /// families it retains before FIFO eviction.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn memo_family_cap_for_test(&self) -> usize {
        self.memo_budget.cap()
    }

    /// Audit-only dump of the warm memo entries, keyed by the
    /// debug-formatted [`FamilyKey`]. Returns one row per populated slot
    /// (a single family with N slot variants populated yields N rows).
    /// Each row carries the slot label, a stable hash of the cached
    /// `SemanticNodeId` payload, and a debug-formatted snapshot of the
    /// `dep_signature` recorded at admission.
    ///
    /// Only used by the Tier 0 Step 0.2 corpus-snapshot test; not on
    /// any hot path. The returned `Vec` is sorted by key-debug-string for
    /// determinism so two runs over the same corpus produce identical
    /// JSON.
    #[doc(hidden)]
    #[must_use]
    pub fn audit_eager_key_dump(&self) -> Vec<AuditEagerKeyRow> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let entries = self.entries.lock();
        let mut rows: Vec<AuditEagerKeyRow> = Vec::with_capacity(entries.len());
        for (family, slots) in entries.iter() {
            let key_repr = format!("{family:?}");
            for (slot_label, slot) in slots.iter_populated_slots() {
                // QueryResult does not derive Hash, so hash a stable
                // debug-formatted projection instead. The hash is opaque
                // to callers — they only need it to be deterministic
                // across two runs over the same fixture corpus.
                let mut hasher = DefaultHasher::new();
                let result_repr = format!("{:?}", slot.result);
                result_repr.hash(&mut hasher);
                let result_hash = format!("{:016x}", hasher.finish());
                let dep_signature = format!("{:?}", slot.read_set_signature.facts);
                rows.push(AuditEagerKeyRow {
                    key_repr: format!("{key_repr}/{slot_label}"),
                    result_hash,
                    dep_signature,
                });
            }
        }
        drop(entries);
        rows.sort_by(|a, b| a.key_repr.cmp(&b.key_repr));
        rows
    }

    /// Number of `(family, slot, admission_seq)` registrations whose
    /// receipts reach `canonical_id` in the `canonical_to_entries` reverse
    /// index: what [`Self::invalidate_canonical`] would evict now. Returns
    /// 0 when no registered receipt reads the canonical.
    #[must_use]
    pub fn canonical_to_entries_count(&self, canonical_id: &str) -> usize {
        self.canonical_to_entries
            .reached_by(canonical_id)
            .iter()
            .map(|(_, registration)| registration.slots().len())
            .sum()
    }

    /// What the family memo's reverse index holds now: its canonicals,
    /// evidence nodes, candidates and physical edges. A required occupancy
    /// count.
    #[must_use]
    pub fn reverse_index_occupancy(&self) -> ReverseIndexOccupancy {
        self.canonical_to_entries.occupancy()
    }

    /// The edge operations the family memo's reverse index has performed.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn reverse_index_edge_ops(&self) -> ReverseIndexEdgeOps {
        self.canonical_to_entries.edge_ops()
    }

    /// Invalidate every warm memo slot whose stored `DepSignature`
    /// references `canonical_id` (a dep-signature sweep, narrower than a
    /// conservative whole-family `family_references_canonical` match).
    ///
    /// Walks every `(FamilyKey, FamilySlots)` entry and, for each
    /// populated slot, drops the slot whose dep-signature names the
    /// changed canonical. Families that end up with no populated slot are
    /// also removed from the entries map.
    ///
    /// In-flight entries whose `(family, slot)` pair matches an evicted
    /// warm slot drop their in-flight handle — `aborted = true` is set on
    /// their shared state, a sentinel is planted in `completed` if not
    /// already set, joiners are woken via `Condvar::notify_all`, and the
    /// entry is removed from the in-flight table so fresh callers start
    /// cold. Joiners currently waiting on the condvar observe the abort
    /// flag on wake and re-enter dispatch from step 1 of
    /// [`Self::execute_cooperative`] (up to `MAX_INFLIGHT_RETRIES`).
    ///
    /// Over-invalidation trade-off: backfilled narrower
    /// slots inherit the broader compute's full dep-signature, so this
    /// sweep may evict a narrower slot whose independent recomputation
    /// would not have read the changed canonical. Correct — never misses
    /// — but potentially spurious. Tightening narrower-slot dep-sigs is
    /// permitted follow-up work.
    ///
    /// Semantic node ids remain stable (the arena is append-only); only
    /// memo slots are cleared. Returns the number of warm slots evicted;
    /// in-flight drops are not included in the count (they are not warm
    /// entries).
    ///
    /// An invalidation is its own execution unit: it adopts the capture mode
    /// the installed request context carries once, here, and its lock
    /// acquisitions record only into a captured request.
    pub fn invalidate_canonical(&self, canonical_id: &str) -> usize {
        verter_audit::specialise!(crate::request_context::installed_observe_mode() => M, {
            self.invalidate_canonical_in::<M>(canonical_id)
        })
    }

    fn invalidate_canonical_in<M: verter_audit::observe::ObserveExec>(
        &self,
        canonical_id: &str,
    ) -> usize {
        use rustc_hash::FxHashSet;
        let observation = crate::request_observers::CaptureTargets::installed::<M>();

        // The family memo is a three-member consistency cluster —
        // `entries` (the memo map), `memo_budget` (its FIFO ledger), and
        // `canonical_to_entries` (the reverse index). Every mutation that
        // touches more than one of them runs under ONE lock domain, the
        // `entries` `Mutex`; the reverse index's own lock is a leaf taken
        // inside it. This drain — the walk from `canonical_id` to every
        // candidate whose receipts reach it, each candidate's eviction and
        // `memo_budget` forget, and each candidate's unregistration — runs
        // under a SINGLE `entries`-lock hold, so a concurrent publish
        // (which registers under the same lock) can neither land between
        // the walk and the eviction nor leave a registration whose entry
        // this drain removed.
        //
        // `affected_pairs` is collected from the reached set so the
        // post-lock in-flight abort can drop matching in-flight entries.
        let mut affected_pairs: FxHashSet<(FamilyKey, ModeSlot)> = FxHashSet::default();
        let mut evicted = 0usize;
        {
            let mut entries = self.entries_lock_diagnosed();
            // The index's lock acquisition, as request audit contention.
            #[cfg(feature = "semantic-observe")]
            crate::request_observers::record_family_map_lock_acquisition::<M>(
                &observation,
                std::time::Duration::ZERO,
            );
            let reached = self.canonical_to_entries.reached_by(canonical_id);
            for (seq, registration) in &reached {
                let family = registration.family();
                for slot in registration.slots() {
                    affected_pairs.insert((family.clone(), *slot));
                }
                // Evict exactly the reached candidate from every slot it
                // populated; sibling candidates in those slots keep their
                // own registrations (R20 overlay isolation).
                if let Some(slots) = entries.get_mut(family) {
                    for slot in registration.slots() {
                        slots.retain_candidates_in_slot_mut(*slot, |entry| {
                            if entry.admission_seq == *seq {
                                evicted += 1;
                                false
                            } else {
                                true
                            }
                        });
                    }
                }
                self.canonical_to_entries.unregister(seq);
            }
            // A family that loses its last slot is removed outright;
            // drop its retention-budget ledger record so the budget
            // does not later return an already-removed family. The
            // key-wide `forget` is sound HERE because this runs inside
            // the `entries`-lock hold — the exact lock domain
            // `record_family_admission_locked` records every
            // `memo_budget` admission under and `invalidate_all` clears
            // it under. No concurrent publisher can record a fresh
            // admission for `family` while this drain holds that lock, so
            // the key-wide removal cannot clobber a concurrent
            // re-admission. `forget_key_under_exclusive_lock`'s contract
            // documents this serialization precondition.
            entries.retain(|family, slots| {
                if slots.populated_count() > 0 {
                    true
                } else {
                    self.memo_budget.forget_key_under_exclusive_lock(family);
                    false
                }
            });

            // Revoke matching ordinary flights and every independently-owned
            // binding flight before releasing `entries`. Independent binding
            // builds are rare and can discover transitive dependencies while
            // running, so a per-canonical edit conservatively aborts the whole
            // independent set. Both registries are collect-then-release:
            // no per-entry state lock is acquired while a registry lock is
            // held. Keeping the `entries` lock through the state update closes
            // the publish-after-invalidation window because both store-owned
            // publish paths re-check `aborted` under this same lock.
            let aborted_entries: Vec<Arc<FlightCell>> = {
                let mut collected: Vec<Arc<FlightCell>> = Vec::new();
                for registry in [&self.inflight, &self.expression_inflight] {
                    let mut table = registry.lock();
                    table.retain(|handle, inflight| {
                        // The prepared handle carries its `(family, slot)`
                        // projection — no per-entry `family_and_slot`
                        // rebuild, no owned-`FamilyKey` allocation. The
                        // slot compares first (a cheap discriminant) so
                        // most non-matching pairs reject before the family
                        // comparison.
                        let affected = affected_pairs.iter().any(|(family, slot)| {
                            *slot == handle.slot() && family == handle.family()
                        });
                        if affected {
                            collected.push(Arc::clone(inflight));
                            false // remove — this entry's slot was swept
                        } else {
                            true // keep — unrelated in-flight build
                        }
                    });
                }
                collected
            };
            let independent_aborted_entries: Vec<Arc<FlightCell>> = {
                let mut table = self.independent_inflight.lock();
                let collected = table
                    .values()
                    .flat_map(|entries| entries.iter().map(Arc::clone))
                    .collect();
                table.clear();
                collected
            };
            for inflight in aborted_entries
                .iter()
                .chain(independent_aborted_entries.iter())
            {
                {
                    let mut state = inflight.state.lock();
                    state.aborted = true;
                    if state.completed.is_none() {
                        state.completed = Some(QueryResult::Error(QueryError::Other(
                            verter_type_expr::OwnedStr::from("aborted by canonical invalidation"),
                        )));
                        state.dep_signature = Some(empty_signature());
                    }
                }
                inflight.wake_subscribers();
                // Test-only injection point: parked while iterating the
                // collected entries and locking per-entry `state` — with
                // the `inflight` table lock NOT held. A race test arms it
                // (via `test_invalidate_canonical_inflight_abort_gate`)
                // and asserts `inflight.try_lock()` succeeds, proving the
                // collect-then-release lock order. `None` (production
                // default) is a no-op.
                #[cfg(any(test, feature = "test-support"))]
                {
                    let gate = self.invalidate_canonical_inflight_abort_gate.lock().clone();
                    if let Some(barrier) = gate {
                        barrier.wait();
                        barrier.wait();
                    }
                }
            }
        }

        // Drop
        // NodeArena shard-dedup entries keyed at
        // `File { canonical_id: c, .. }`. Preserves Global entries
        // and entries for any other canonical. The
        // arena Vec is append-only — this only clears the "next
        // intern returns existing id" path; valid SemanticNodeIds for
        // nodes already published into the arena are unaffected.
        self.arena
            .invalidate_for_canonical::<M>(&observation, canonical_id);

        // Hash-cons memos (substitute, evaluate-deferred) — drop in
        // full on any per-canonical edit. The memo VALUE may be a
        // SemanticNodeId whose structural meaning depended on the
        // edited canonical's content (TypeOf walks through
        // ValueRootKey, generic instantiations referencing imported
        // type-decl bodies, etc.). Even though the cache KEY is a
        // node-id tuple, the cached (key → result_id) mapping is
        // computed by a walk that may transitively reach the
        // invalidated canonical. The sledgehammer clear is correct
        // by construction: no stale derivative survives a content
        // edit. The structural plumbing for a reverse-indexed
        // per-canonical clear is documented as future work in
        // hash_cons_memos.rs.
        self.clear_hash_cons_memos();
        // Sealed refusals whose failed evaluation reached the canonical
        // leave with the memo's candidates.
        self.invalidate_refusal_summaries(canonical_id);
        // A per-canonical edit is a reclamation point for the kernel store.
        let _ = self.compact_signature_store_if_over_cap();
        evicted
    }

    /// Clear every warm semantic cache entry. Used on project-generation
    /// bumps (`tsconfig` changes, active-TS-SDK swaps, workspace-folder
    /// changes). Returns the number of memo slots cleared (summed across
    /// every family).
    ///
    /// ## What this clears
    ///
    /// A project-generation bump invalidates the whole semantic graph at
    /// once. This method drops every warm cache structure on the store:
    ///
    /// - `entries` — the family memo. Every populated slot is counted
    ///   into the return value, then the map is cleared.
    /// - `inflight` — in-flight admission table. Each entry is aborted
    ///   (so any cooperative joiner blocked on the condvar wakes and
    ///   re-enters dispatch against the fresh project generation rather
    ///   than joining a stale build) and the table is drained. This
    ///   mirrors the per-canonical abort in [`Self::invalidate_canonical`].
    /// - `memo_budget`, `canonical_to_entries` — the family memo's FIFO
    ///   retention ledger and reverse index. Both are cleared under the
    ///   same `entries`-lock hold as `entries.clear()` (see below).
    /// - `derivation` — the other
    ///   `SemanticNodeId`-keyed semantic cache. Clearing it on a
    ///   project-generation bump drops the stale judgements it holds.
    ///   (The relation memo needs no separate clear: its entries live in
    ///   the family memo's `Relate` family and are dropped by the
    ///   `entries` / `memo_budget` / `canonical_to_entries` clear above.)
    ///
    /// Each bounded cache has a retention ledger; the ledger is cleared
    /// in lockstep with the map it bounds so no budget retains a key
    /// whose entry is gone. The family memo is a three-member consistency
    /// cluster — `entries` (the memo map), `memo_budget` (its FIFO
    /// ledger), and `canonical_to_entries` (the reverse index
    /// [`Self::invalidate_canonical`] walks) — and all three live in one
    /// lock domain, the `entries` `Mutex`: this method clears `memo_budget`
    /// AND `canonical_to_entries` while still holding the lock that
    /// performed `entries.clear()`, and the publish path records each
    /// `memo_budget` admission and registers each `canonical_to_entries`
    /// entry while holding the `entries` lock that landed the slot, so a
    /// publish cannot strand a live family with no ledger record nor a
    /// live memo entry with no reverse-index registration. The relation
    /// memo rides the SAME cluster (its entries are `Relate` families),
    /// so its map and ledger are cleared in the same lock domain too.
    ///
    /// ## The node arena is append-only
    ///
    /// `invalidate_all` does NOT reset the node arena. `SemanticNodeId`
    /// is a raw `u64` arena index with no generation tag, so the arena's
    /// dense node storage stays append-only — every `SemanticNodeId`
    /// handed out before this call remains valid and resolves to the
    /// same payload afterwards. Reclaiming the id space would require a
    /// generational `SemanticNodeId` redesign.
    ///
    /// ## Locking — no winner may publish into a cleared `entries`
    ///
    /// The in-flight abort and the `entries` clear are performed under a
    /// SINGLE `entries`-lock hold (the abort loop's lock discipline is
    /// detailed in the inline comment below). This is load-bearing: a
    /// cold winner publishes through [`Self::warm_publish_one`], which
    /// acquires `entries` and re-checks `inflight.state.aborted` under it
    /// before writing a memo slot. Holding `entries` across BOTH the
    /// abort and the clear leaves a winner with strictly two cases — it
    /// published BEFORE the clear (its entry is dropped by the clear), or
    /// it acquires `entries` AFTER this method releases it, by which
    /// point `aborted` is set so its `warm_publish_one` re-check skips
    /// the publish. No window lets a winner publish a slot the
    /// abort/clear then fails to remove.
    ///
    /// The acquisition order `entries → inflight` matches
    /// [`Self::invalidate_canonical`] and every other multi-lock path, so
    /// there is no AB-BA cycle. The abort loop additionally never holds
    /// the `inflight` table lock while taking a per-entry `state` lock
    /// (collect-then-release — see the inline comment), keeping it within
    /// the module-global lock rule. [`InflightPanicGuard`]'s `drop` is not
    /// a counter-acquirer of these two locks at all: it locks `state`,
    /// *releases* it, and only then acquires the `inflight` table lock —
    /// two sequential acquisitions, never nested — so it can neither
    /// deadlock nor establish a competing order. The relation memo rides
    /// the same `entries` lock domain (its entries are `Relate` families
    /// in the family memo) — no separate relation gate exists.
    pub fn invalidate_all(&self) -> usize {
        let _ = self.signatures.replace_epoch();
        self.clear_refusal_summaries();
        let removed: usize = {
            let mut entries = self.entries_lock_diagnosed();
            let count = entries.values().map(FamilySlots::populated_count).sum();
            // Abort every in-flight admission, then drain the table and
            // clear `entries` — all under the SAME `entries`-lock hold.
            // `SemanticQueryKey` keys embed `SemanticNodeId`s, and a
            // cooperative joiner blocked on the condvar must wake and
            // re-enter dispatch rather than join a stale, soon-to-be-
            // invalid entry. Setting `aborted` before the `entries` lock
            // that performs the clear is released closes the torn-publish
            // window: a cold winner that acquires `entries` afterwards
            // re-checks `aborted` in `warm_publish_one` and skips. Mirrors
            // the per-canonical abort in `invalidate_canonical`.
            //
            // **Lock order — collect-then-release.** This loop must NOT
            // hold the `inflight` table lock while it takes each entry's
            // `state` lock. A loop holding `table` across the per-entry
            // `state` acquisition would establish a `table → state`
            // nesting — a latent lock-order inconsistency against the
            // module-global rule below. (It is not a live deadlock today:
            // the only other path touching both locks,
            // `InflightPanicGuard::drop`, acquires `state`, *releases* it,
            // and only then acquires the `inflight` table lock — two
            // sequential, non-nested acquisitions — so it cannot AB-BA
            // against either order. Collect-then-release keeps the rule
            // uniform regardless.) Instead: snapshot the
            // `Arc<FlightCell>` handles AND drain the table under the
            // table lock, RELEASE the table lock, THEN lock each entry's
            // `state`. Global rule: `state` is never taken while the
            // `inflight` table lock is held.
            let aborted_entries: Vec<Arc<FlightCell>> = {
                let mut collected = Vec::new();
                for registry in [&self.inflight, &self.expression_inflight] {
                    let mut table = registry.lock();
                    collected.extend(table.values().map(Arc::clone));
                    table.clear();
                }
                collected
            };
            let independent_aborted_entries: Vec<Arc<FlightCell>> = {
                let mut table = self.independent_inflight.lock();
                let collected = table
                    .values()
                    .flat_map(|entries| entries.iter().map(Arc::clone))
                    .collect();
                table.clear();
                collected
            };
            // Table lock released — now mark each collected entry
            // `aborted` and wake its waiters with no table lock held.
            for inflight in aborted_entries
                .iter()
                .chain(independent_aborted_entries.iter())
            {
                {
                    let mut state = inflight.state.lock();
                    state.aborted = true;
                    if state.completed.is_none() {
                        state.completed = Some(QueryResult::Error(QueryError::Other(
                            verter_type_expr::OwnedStr::from("aborted by project-generation reset"),
                        )));
                        state.dep_signature = Some(empty_signature());
                    }
                }
                inflight.wake_subscribers();
                // Test-only injection point: parked while iterating the
                // collected entries and locking per-entry `state` — with
                // the `inflight` table lock NOT held. A race test arms it
                // (via `test_invalidate_all_inflight_abort_gate`) and
                // asserts `inflight.try_lock()` succeeds, proving the
                // collect-then-release lock order. `None` (production
                // default) is a no-op.
                #[cfg(any(test, feature = "test-support"))]
                {
                    let gate = self.invalidate_all_inflight_abort_gate.lock().clone();
                    if let Some(barrier) = gate {
                        barrier.wait();
                        barrier.wait();
                    }
                }
            }
            entries.clear();
            // Drop the family memo's retention ledger UNDER THE SAME
            // `entries`-lock hold as the `entries.clear()` above. The
            // publish path records each `memo_budget` admission while
            // holding the `entries` lock that landed the slot, so the
            // map and its budget are mutated within one lock domain. A
            // concurrent publisher therefore cannot land an `entries`
            // family + `memo_budget` admission straddling these two
            // clears — which would otherwise strand a live family with
            // no ledger record, making it invisible to FIFO eviction.
            // Test-only injection point: a barrier armed by
            // `test_invalidate_all_pre_memo_budget_clear_gate` parks
            // here, before the `memo_budget` clear and with the `entries`
            // lock still held, so a race test can assert the clear runs
            // in the `entries` lock domain. `None` (the production
            // default) is a no-op.
            #[cfg(any(test, feature = "test-support"))]
            {
                let gate = self
                    .invalidate_all_pre_memo_budget_clear_gate
                    .lock()
                    .clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
            self.memo_budget.clear();
            // Drop the family memo's `canonical_to_entries` reverse index
            // UNDER THE SAME `entries`-lock hold as the `entries.clear()`
            // and `memo_budget.clear()` above. The three are one
            // consistency cluster — `entries` is the memo map,
            // `memo_budget` its FIFO ledger, `canonical_to_entries` the
            // reverse index `invalidate_canonical` walks. The publish path
            // registers each `canonical_to_entries` entry while holding
            // the `entries` lock that landed the slot (see
            // `register_reverse_index`'s call sites), so all three members
            // are mutated within one lock domain — the `entries` `Mutex`.
            // A concurrent publisher therefore cannot register into
            // `canonical_to_entries` between this `entries.clear()` and
            // this reverse-index clear — which would otherwise leave a
            // stranded registration with no entry, or (if the publish
            // landed its `entries` slot before the clear) a live memo
            // entry with no reverse-index registration, invisible to a
            // later `invalidate_canonical`. The `entries →
            // canonical_to_entries` lock order PERMITS taking the index's
            // leaf lock while `entries` is held (`entries` is outermost),
            // and the index never takes another lock, so this nesting is
            // sound.
            // Test-only injection point: a barrier armed by
            // `test_invalidate_all_pre_reverse_index_clear_gate` parks
            // here, before the reverse-index clear and with the `entries`
            // lock still held, so a race test can assert the clear runs in
            // the `entries` lock domain. `None` (the production default)
            // is a no-op.
            #[cfg(any(test, feature = "test-support"))]
            {
                let gate = self
                    .invalidate_all_pre_reverse_index_clear_gate
                    .lock()
                    .clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
            self.canonical_to_entries.clear();
            count
        };
        // Test-only injection point: a barrier armed by
        // `test_invalidate_all_post_entries_clear_gate` parks here, after
        // the `entries` lock that performed the abort + clear has been
        // released. It lets a test deterministically race a cold winner's
        // `warm_publish_one` against this method's post-clear tail — the
        // winner re-checks `aborted` (already set above) and must skip.
        // A single relaxed lock probe; `None` (the production default) is
        // a no-op.
        #[cfg(any(test, feature = "test-support"))]
        {
            let gate = self.invalidate_all_post_entries_clear_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
            }
        }
        // Drop every other `SemanticNodeId`-keyed semantic cache so no
        // stale id-keyed judgement survives the project-generation bump.
        // Relation judgements live in the family memo's `Relate` family,
        // so they were already dropped by the family memo's three-member
        // cluster clear (`entries`, `memo_budget`, `canonical_to_entries`)
        // above under the `entries` lock.
        self.derivation.lock().clear();
        // Hash-cons memos (substitute, evaluate-deferred) — see
        // `hash_cons_memos.rs` for the invalidation contract.
        self.clear_hash_cons_memos();
        removed
    }

    // ──────────────────────────────────────────────────────────────────
    // Telemetry — public stats snapshot
    // ──────────────────────────────────────────────────────────────────

    /// Builder-side counter helpers. Builders increment these as they emit
    /// reusable work; the per-builder semantics are documented in plan
    /// §3 (where the real builders land).
    pub fn record_instantiate(&self) {
        self.stats.instantiate_count.fetch_add(1, Ordering::Relaxed);
    }
    /// Bumped by `build_awaited_normalize` on a COLD build only. A warm
    /// family hit returns before the builder runs, so this counter is the
    /// evidence that the awaited relation memoizes.
    pub fn record_awaited_normalize(&self) {
        self.stats
            .awaited_normalize_count
            .fetch_add(1, Ordering::Relaxed);
    }
    /// Bumped by `build_async_return_payload` on a COLD build only.
    pub fn record_async_return_payload(&self) {
        self.stats
            .async_return_payload_count
            .fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_conditional_decided(&self) {
        self.stats
            .conditional_decided_count
            .fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_conditional_deferred(&self) {
        self.stats
            .conditional_deferred_count
            .fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_branch_selection_true(&self) {
        self.stats
            .branch_selections_true
            .fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_branch_selection_false(&self) {
        self.stats
            .branch_selections_false
            .fetch_add(1, Ordering::Relaxed);
    }
    /// Record one path-length sample into the bounded reservoir.
    /// Builders call this once per `ProjectPath` invocation. A no-op
    /// without the observation gate.
    #[inline(always)]
    pub fn record_path_length(&self, length: u32) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        self.stats.path_length_samples.lock().push(length);
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = length;
    }
    pub fn record_decl_subexpression_lowering(&self) {
        self.stats
            .decl_subexpression_lowering_count
            .fetch_add(1, Ordering::Relaxed);
    }
    pub fn record_relation_check(&self) {
        self.stats
            .relation_check_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Bump the per-K mapped-type materialiser counter. Used by the
    /// key-independent-value hoist discriminator to distinguish the
    /// short-circuit fast path (hoist-eligible mapped types never
    /// reach the per-K materialiser and never bump this counter)
    /// from the fully-instantiated per-K loop.
    pub fn record_mapped_per_k_materialization(&self) {
        self.stats
            .mapped_per_k_materializations
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Warm-lookup a key **without `ReadSetSignature` validation**.
    /// Returns the memoized result + its recorded dependency signature
    /// when the requested `(family, mode_slot)` is populated.
    ///
    /// **Unchecked-read contract — TEST / DEBUG ONLY.** This entry
    /// point bubbles the entry's path-precise fact signature
    /// unconditionally; neither the AND-gate validation nor the strict
    /// self-root validation against the live store view runs here. A
    /// stale entry returns its cached value AND pollutes the outer fact
    /// tracer with observations that no longer reflect current state.
    ///
    /// **No production warm-read caller may use this.** Every
    /// production warm read of the semantic graph — the
    /// cooperative-admission fast path / slow-path step-1 re-check, the
    /// non-admission batch probe, the build-side prefix-probe at
    /// `build_project_path` — routes through the strict
    /// [`Self::get_validated`] (or the carrier-validating
    /// [`Self::execute_cooperative`] fast path). `get_validated`
    /// validates the entry's carrier — every self-root canonical's
    /// `FileWholeHash` strictly — BEFORE bubbling, so a stale entry
    /// neither returns nor pollutes the outer tracer.
    ///
    /// The only sanctioned callers are test and debug probes that
    /// explicitly want the unchecked read for cache-state inspection.
    /// The architecture guard `semantic_graph_production_reads_validated`
    /// (`tests/cases/g_misc0/semantic_graph_production_reads_validated.rs`) enforces
    /// that no seal-scope production file calls `get_unvalidated`.
    ///
    /// The name carries the contract: `get_unvalidated` returns an entry
    /// WITHOUT validating its carrier, so the unvalidated nature is
    /// explicit at every call site.
    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_unvalidated(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        // Pick the first candidate as the unvalidated representative —
        // diagnostic / cache-state callers that do not run the strict
        // self-root validator. The candidate list is multi-element
        // under multi-view publication; selecting the first preserves
        // the prior "is there an entry here at all" semantics.
        let result = entries.get(&family).and_then(|slots| {
            slots.slot_peek_any(slot).cloned().map(|entry| {
                // R3/R26/R28 - bubble the entry path-precise fact
                // observation set into any outer cold-compute scope
                // so transitive memo hits do not lose contributing
                // fact identities. AND-gate validation happens at
                // the outer fence revalidation point.
                entry.read_set_signature.bubble_via_tls();
                let dep_signature = entry.dispatch_dep_signature.clone();
                CacheRead {
                    lease: entry.lease,
                    value: entry.result,
                    dep_signature,
                    walker_diagnostics: entry.walker_diagnostics,
                    cache_suppress: false,
                    fact_status: crate::semantic_query::FactStatus::Complete,
                    receipt: crate::semantic_query::ReadReceipt::Priced(entry.cost_receipt),
                }
            })
        });
        crate::observe_request!(|rctx| {
            if result.is_some() {
                rctx.cache_counters
                    .semantic_graph
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .semantic_graph
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
        result.map(narrow_cache_read)
    }

    construction_ordinal_seam! {
        /// Carrier-aware warm read that validates BEFORE bubbling. Returns
        /// the cached value only when the entry's
        /// [`ReadSetSignature`](verter_session_query::facts::fact_cache::ReadSetSignature)
        /// validates against the live store view; otherwise `None`. On
        /// validation failure the bubble channel is NOT exercised — a
        /// stale entry must not pollute an outer tracer with observations
        /// that no longer reflect the current state.
        ///
        /// This is the production warm-read entry point used by callers
        /// that consult the warm map outside the cooperative-admission
        /// flow (e.g. the prefix-probe in `build_project_path`). The
        /// cold-build helper inside `execute_cooperative` continues to use
        /// `get` because its own coordination flow performs the validate /
        /// remove / cold-recompute dance one level up.
        ///
        /// Counter semantics: a miss-by-staleness records the same
        /// `cache_counters.semantic_graph.misses` bump as a true cold
        /// miss; from the caller's perspective the entry is unavailable
        /// either way.
        #[must_use]
        fn get_validated<C: crate::resolver_core::ResolverCapabilities>(
            &self,
            key: &SemanticQueryKey,
            ctx: &dyn crate::resolver_core::ResolverContext<C>,
        ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
            let (family, slot) = family_and_slot(key);
            // Same formula as `requested_point_for_key`, reusing the
            // `family_and_slot` projection above instead of re-running it.
            let requested = crate::semantic_query::demand::MaterializedPoint::new(
                family::point_for_slot(slot, &requested_path_for_key(key)),
            );
            self.get_validated_value_impl(&family, slot, &requested, ctx, None, None, true)
                .map(narrow_cache_read)
        }
    }

    /// Prepared-token variant of [`Self::get_validated`] — reads the
    /// family / slot / requested point off the token instead of
    /// re-projecting the key. Used by the cooperative slow path's
    /// step-1 warm re-read.
    #[must_use]
    fn get_validated_value_prepared<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        prepared: &PreparedKeyHandle,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        capture: &mut producer::ReadCapture<'_>,
    ) -> Option<CacheRead<QueryResult<SemanticQueryValue>>> {
        let producer::CaptureParts {
            evidence: operand_evidence,
            carrier: deferred_carrier,
        } = capture.parts();
        self.get_validated_value_impl(
            prepared.family(),
            prepared.slot(),
            prepared.requested_point(),
            ctx,
            operand_evidence,
            deferred_carrier,
            true,
        )
    }

    /// Shared snapshot → two-gate validate → bubble → LRU-promote warm
    /// read. `record_miss` selects who owns the per-request miss count:
    /// pass `true` when this probe is the owning read for the logical
    /// query (the [`Self::get_validated`] / cooperative rails). The
    /// modeless payload accessors (`get_relation_payload`,
    /// `get_flow_return_result`, `get_resolve_call_result`) pass `false`:
    /// a modeless miss never short-circuits — every production caller
    /// falls through to the owning cooperative path, which records the
    /// single miss — while a modeless hit short-circuits the dispatch,
    /// so the probe owns the hit count. Counting the probe miss too
    /// would record two misses per logical cold query, breaking the
    /// one-miss contract.
    fn get_validated_value_impl<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        family: &FamilyKey,
        slot: ModeSlot,
        requested: &crate::semantic_query::demand::MaterializedPoint,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        operand_evidence: Option<
            &mut Option<crate::semantic_query::operand::SemanticOperandEvidence>,
        >,
        deferred_carrier: Option<
            &mut Option<verter_session_query::facts::fact_cache::ReadSetSignature>,
        >,
        record_miss: bool,
    ) -> Option<CacheRead<QueryResult<SemanticQueryValue>>> {
        // Snapshot the candidate list under the lock, then validate
        // OUTSIDE the lock. Holding `entries` across `MemoEntry::validate`
        // — which walks the path-precise fact rail against the resolver
        // store view — would serialise every unrelated warm read and
        // cold publish on the single global memo mutex.
        let snapshot: Option<CandidateSnapshot> = {
            let entries = self.entries_lock_diagnosed();
            entries.get(family).map(|slots| slots.snapshot_slot(slot))
        };
        // §3.4 warm hit — `cached_satisfies` (recorded-point dominance,
        // pure), a live kernel epoch, AND `validate_with_self_roots` (fact
        // rail). All must pass; see `try_warm_hit_fast_path` for the rationale.
        // `validated_at_generation` is recency metadata only, never a
        // validity oracle: project-shape invalidation rides
        // `FactVersionRef::ProjectGeneration` on the carrier.
        let validated = snapshot.and_then(|list| {
            let back = list.len().checked_sub(1); // the freshest in LRU order
            let mut candidates = list.into_iter().enumerate();
            let (index, hit) =
                candidates.find(|(_, entry)| self.warm_candidate_serves(entry, requested, ctx))?;
            Some((Some(index) != back, hit))
        });
        if let Some((true, entry)) = &validated {
            // Brief LRU bookkeeping for a hit that is not the snapshot's
            // freshest: reacquire to move it to the LRU back. Matched by
            // discriminant identity; a candidate drained meanwhile is a no-op.
            let mut entries = self.entries_lock_diagnosed();
            if let Some(slots) = entries.get_mut(family) {
                slots.mark_validated_freshest(slot, entry);
            }
        }
        let result = validated.map(|(_, entry)| {
            if let Some(capture) = operand_evidence {
                *capture = semantic_operand_evidence(
                    &entry.read_set_signature,
                    &entry.self_root_canonicals,
                    &entry.dispatch_dep_signature,
                );
            }
            match deferred_carrier {
                Some(slot) => *slot = Some(entry.read_set_signature.clone()),
                None => entry.read_set_signature.bubble(ctx),
            }
            let dep_signature = entry.dispatch_dep_signature.clone();
            CacheRead {
                lease: entry.lease,
                value: entry.result,
                dep_signature,
                walker_diagnostics: entry.walker_diagnostics,
                cache_suppress: false,
                fact_status: crate::semantic_query::FactStatus::Complete,
                receipt: crate::semantic_query::ReadReceipt::Priced(entry.cost_receipt),
            }
        });
        crate::observe_request!(C::Observe => on crate::resolver_core::resolver_context::capture_targets(ctx).request.as_deref() => |rctx| {
            if result.is_some() {
                rctx.cache_counters
                    .semantic_graph
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else if record_miss {
                rctx.cache_counters
                    .semantic_graph
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
        result
    }

    /// Presence probe — true iff the warm map currently holds a
    /// `(family, slot)` entry for `key`. Does NOT validate, does NOT
    /// bubble, does NOT bump any counter. The production prefix-backfill
    /// presence probe runs inline on the prepared token inside
    /// `warm_publish_one_if_absent`; this by-key variant serves the
    /// in-crate dispatch test suite (`#[cfg(test)]`), the only caller —
    /// `pub(crate)` keeps it unreachable from external test crates, so
    /// the tighter `cfg(test)` gate compiles it exactly when its caller
    /// exists (a `test-support`-without-`test` build would otherwise
    /// carry it as dead code).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn contains_key(&self, key: &SemanticQueryKey) -> bool {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .is_some_and(|slots| slots.slot_peek_any(slot).is_some())
    }

    /// Warm lookup for [`Self::begin_query_claim`]. Returns
    /// `Some(CacheRead)` when the slot for `key` is already populated
    /// in the family memo, `None` otherwise.
    ///
    /// On hit, bumps:
    /// - `cache_counters.semantic_graph.hits` (per-request,
    ///   audit-truthful, single bump per warm call)
    /// - `record_cache_event(Hit)` on the active `RequestContext` (if
    ///   any) so audit accounting matches the slow path's warm
    ///   semantics
    /// - `self.stats.hits` (lock-free atomic — same as slow path)
    /// - `WARM_HIT_FAST_PATH_HITS` (instrumentation, attribution to
    ///   the fast branch)
    /// - `EXECUTE_COOPERATIVE_WARM_HITS` and `FAMILY_MEMO_HITS`
    ///   (instrumentation; consistent with slow-path semantics)
    /// - `record_dispatch_warm` (cfg-test) and
    ///   `with_active_capture(record_dispatch)` (production
    ///   capture-token TLS hook)
    ///
    /// On miss returns `None` without touching any counter. The
    /// caller's slow path observes the miss exactly once when its
    /// step 1 `self.get_unvalidated(&key)` returns `None`, preserving slow-path
    /// counter discipline.
    ///
    /// **Lock discipline.** Acquires `self.entries` directly (no
    /// `entries_lock_diagnosed` Instant::now/capture-token wrapping).
    /// Holds the lock ONLY for the slot SNAPSHOT — a clone of the
    /// candidate list out of the slot — then releases it. Carrier
    /// validation (`entry.validate` — fact-rail validation against the
    /// resolver store view with strict self-root checks), the TLS
    /// fact-rail bubble, and instrumentation all run AFTER the lock is
    /// dropped, so an unrelated warm read or cold publish does not
    /// serialise on the single global memo mutex for the duration of
    /// validation. LRU bookkeeping briefly reacquires the lock to move
    /// the matching candidate to the back of the slot's LRU order — a
    /// constant-time `Vec` reorder, no fact-rail work. Mirrors the
    /// relation memo's `get_relation`.
    #[inline]
    fn try_warm_value_hit_fast_path<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        prepared: &PreparedKeyHandle,
        capture: &mut producer::ReadCapture<'_>,
    ) -> Option<CacheRead<QueryResult<SemanticQueryValue>>> {
        let producer::CaptureParts {
            evidence: operand_evidence,
            carrier: deferred_carrier,
        } = capture.parts();
        let key = prepared.key();
        let family = prepared.family();
        let slot = prepared.slot();

        // Snapshot the candidate list under the lock; validate OUTSIDE
        // the lock. With the multi-candidate substrate the
        // validation walk over the path-precise fact rail is bounded
        // but still non-trivial, and holding the single global memo
        // mutex across it would serialise every unrelated warm read
        // and cold publish during validation. Stale candidates are
        // skipped without bubbling.
        let snapshot: Option<CandidateSnapshot> = {
            let entries = self.entries.lock();
            entries.get(family).map(|slots| slots.snapshot_slot(slot))
        };
        // §3.4 warm hit. Gate 1: `cached_satisfies` — the candidate's
        // RECORDED materialised set must dominate the requested point
        // (pure, no store view; cheap, so first). Gate 2: no retired
        // kernel epoch. Gate 3: `validate_with_self_roots` — the fact rail
        // must validate against the live view. ALL must pass; a candidate
        // failing any is skipped without bubbling. `validated_at_generation`
        // is recency metadata only.
        let requested = prepared.requested_point();
        let snapshot = snapshot?;
        let candidates = snapshot.len();
        let (position, entry): (usize, MemoEntry) = snapshot
            .into_iter()
            .enumerate()
            .find(|(_, e)| self.warm_candidate_serves(e, requested, ctx))?;
        // Brief LRU bookkeeping — reacquire ONLY to move the matching
        // candidate to the back of the slot's LRU order so subsequent
        // lookups treat it as freshest. The match is by discriminant
        // identity; if a concurrent invalidation drained it between
        // snapshot and here, the update is a no-op (the caller still
        // gets the cloned entry from the snapshot). A candidate the
        // snapshot already held at the back is the freshest already.
        if position + 1 < candidates {
            let mut entries = self.entries.lock();
            if let Some(slots) = entries.get_mut(family) {
                slots.mark_validated_freshest(slot, &entry);
            }
        }

        if let Some(capture) = operand_evidence {
            *capture = semantic_operand_evidence(
                &entry.read_set_signature,
                &entry.self_root_canonicals,
                &entry.dispatch_dep_signature,
            );
        }

        // R3/R26/R28 - bubble the entry path-precise fact observation
        // set into any outer cold-compute scope so transitive memo
        // hits do not lose the contributing fact identities — or hand it
        // to the consumer that replays it when it resumes.
        match deferred_carrier {
            Some(slot) => *slot = Some(entry.read_set_signature.clone()),
            None => entry.read_set_signature.bubble_via_tls(),
        }
        let hit = CacheRead {
            lease: entry.lease,
            value: entry.result,
            dep_signature: entry.dispatch_dep_signature.clone(),
            walker_diagnostics: entry.walker_diagnostics,
            cache_suppress: false,
            fact_status: crate::semantic_query::FactStatus::Complete,
            receipt: crate::semantic_query::ReadReceipt::Priced(entry.cost_receipt),
        };
        self.note_warm_hit::<C::Observe>(
            crate::resolver_core::resolver_context::capture_targets(ctx),
            key,
        );
        Some(hit)
    }

    /// The observations of one family-memo lookup entry, warm or cold.
    pub(crate) fn note_lookup_entry<M: verter_audit::observe::ObserveExec>(&self) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        if records_telemetry::<M>() {
            crate::loop5_instrumentation::EXECUTE_COOPERATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The observations a family-memo lookup that missed `key` makes.
    pub(crate) fn note_lookup_miss<M: verter_audit::observe::ObserveExec>(
        &self,
        key: &SemanticQueryKey,
    ) {
        let _ = std::marker::PhantomData::<M>;
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = key;
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        if records_telemetry::<M>() {
            crate::loop5_instrumentation::FAMILY_MEMO_MISSES.fetch_add(1, Ordering::Relaxed);
        }
        #[cfg(any(test, feature = "test-support"))]
        crate::project_semantic_dispatch::raise::record_dispatch_cold(key);
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| t.record_dispatch(key, /* hit */ false));
        // A chronological operation trace: optional observation.
        #[cfg(feature = "semantic-observe")]
        if M::CAPTURED {
            tracing::debug!(target: "verter::memo::miss", ?key, "memo_miss");
        }
    }

    /// The observations a family-memo lookup answered warm for `key` makes.
    fn note_warm_hit<M: verter_audit::observe::ObserveExec>(
        &self,
        observation: &crate::request_observers::CaptureTargets,
        key: &SemanticQueryKey,
    ) {
        let _ = observation;
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = key;
        // Instrumentation — fast-path attribution.
        if records_telemetry::<M>() {
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            crate::loop5_instrumentation::WARM_HIT_FAST_PATH_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            crate::loop5_instrumentation::EXECUTE_COOPERATIVE_WARM_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            crate::loop5_instrumentation::FAMILY_MEMO_HITS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Production stats (lock-free atomic — same as slow path's
            // warm-hit branch at step 1).
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
        }

        // Per-request audit hit attribution. Bumped exactly once per
        // warm call (the slow path's old `let initial_hit = self.get_unvalidated(&key)`
        // observation plus the loop's step-1 `self.get_unvalidated(&key)` call
        // bumped this counter twice; the fast path bumps it once).
        crate::observe_request!(M => on observation.request.as_deref() => |rctx| {
            rctx.cache_counters
                .semantic_graph
                .hits
                .fetch_add(1, Ordering::Relaxed);
        });

        // Per-context cache-event attribution (Hit). Same as the slow
        // path's step-1 warm branch, single TLS lookup.
        crate::request_observers::record_cache_event::<M>(
            observation,
            verter_execution::request_context::CacheEventKind::Hit,
        );

        // cfg-test dispatch recording (warm). Same as the slow path's
        // pre-loop `initial_hit` observation.
        #[cfg(any(test, feature = "test-support"))]
        crate::project_semantic_dispatch::raise::record_dispatch_warm(key);

        // Capture-token dispatch recording (warm). Same as the slow
        // path's pre-loop observation. Gated to match the instrumentation
        // module (absent in release).
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| t.record_dispatch(key, /* hit */ true));

        // A chronological operation trace: optional observation.
        #[cfg(feature = "semantic-observe")]
        if M::CAPTURED {
            tracing::debug!(target: "verter::memo::hit", ?key, "memo_hit");
        }
    }

    /// Abort and retire one canceled cold owner without publishing a result.
    /// Joiners observe `aborted`, wake, and re-enter admission with their own
    /// still-unconsumed build closure. Removal is pointer-guarded so a follower
    /// that already installed a replacement flight cannot be evicted here.
    fn abort_inflight_for_cancellation(
        &self,
        prepared: &PreparedKeyHandle,
        inflight: &Arc<FlightCell>,
        independent_owner: bool,
    ) {
        {
            let mut state = inflight.state.lock();
            state.aborted = true;
            state.completed = None;
            state.dep_signature = None;
            state.graph_carrier = None;
            state.cost_receipt = None;
            state.walker_diagnostics = None;
            state.cache_suppress = true;
            state.fact_status = crate::semantic_query::FactStatus::unavailable(
                crate::semantic_query::PartialReasonSet::CANCELLED,
            );
            // A cancellation abort is a partial with its own precise class.
        }
        inflight.wake_subscribers();
        #[cfg(any(test, feature = "test-support"))]
        {
            let gate = self.cancellation_abort_pre_retire_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.retire_inflight(prepared, inflight, independent_owner);
    }

    fn site_inflight(
        &self,
        site: crate::semantic_query::checker_policy::TupleSplicePosition,
    ) -> &Mutex<FxHashMap<PreparedKeyHandle, Arc<FlightCell>>> {
        match site {
            crate::semantic_query::checker_policy::TupleSplicePosition::Type => &self.inflight,
            crate::semantic_query::checker_policy::TupleSplicePosition::Expression => {
                &self.expression_inflight
            }
        }
    }

    fn retire_inflight(
        &self,
        prepared: &PreparedKeyHandle,
        inflight: &Arc<FlightCell>,
        independent_owner: bool,
    ) {
        if independent_owner {
            let mut table = self.independent_inflight.lock();
            if let Some(entries) = table.get_mut(prepared) {
                entries.retain(|entry| !Arc::ptr_eq(entry, inflight));
                if entries.is_empty() {
                    table.remove(prepared);
                }
            }
        } else {
            let mut table = self.site_inflight(inflight.site).lock();
            if table
                .get(prepared)
                .is_some_and(|entry| Arc::ptr_eq(entry, inflight))
            {
                table.remove(prepared);
            }
        }
    }

    /// Cold-winner publish path. Extracted from
    /// [`Self::execute_cooperative`] step 5 (refactor — pure
    /// extraction, no behaviour change). Skips publish when the result is
    /// not a [`QueryResult::Value`] (errors / recursion sentinels never
    /// promote to warm cache entries — cache population).
    ///
    /// **TOCTOU contract.** Acquires `entries` lock first, then
    /// re-checks `inflight.state.aborted` under the entries lock. If
    /// invalidation's acquired `entries` first and aborted this
    /// in-flight via step 2, the re-check sees `aborted = true` and
    /// skips publish. If this caller got `entries` first, publishes and
    /// releases; invalidation then evicts the fresh publish in its phase
    /// 1. Either interleaving leaves the slot empty post-invalidation.
    ///
    /// Per-store test trigger [`Self::force_cold_abort_sweep`]
    /// simulates a concurrent sweep without racing a real
    /// invalidation window.
    ///
    /// **Return value.** Returns [`WarmPublishOutcome::Aborted`] IFF the
    /// TOCTOU re-check observed `aborted == true`, `Skipped` for a
    /// non-value, and `Published` with the exact admitted candidate token
    /// after the entry and reverse index have landed under the entries lock.
    /// The caller's prefix-backfill loop is gated on the abort arm: an aborted winner
    /// was raced by a project-generation reset, so its build interned
    /// against a stale id epoch and its narrower backfills must be
    /// skipped too — see [`Self::invalidate_all`]'s serialization docs.
    fn warm_publish_one<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        prepared: &PreparedKeyHandle,
        result: &QueryResult<SemanticQueryValue>,
        walker_diagnostics: &crate::project_semantic_dispatch::walk::WalkerDiagnostics,
        read_set_signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
        dispatch_dep_signature: &DepSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        satisfied_projection: &MaterializedSet,
        cost_receipt: &Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
        inflight: &Arc<FlightCell>,
    ) -> WarmPublishOutcome {
        let publishable = matches!(result, QueryResult::Value(_));
        if !publishable {
            // Not an abort — an error / recursion sentinel that never
            // promotes to a warm entry. The winner's build epoch is
            // still consistent, so backfills (if any) stay valid.
            return WarmPublishOutcome::Skipped;
        }
        let family = prepared.family();
        let slot = prepared.slot();
        let requested_path = prepared.requested_path();
        // §3.4 soundness invariant (production publish ONLY): the recorded
        // terminal must be at-least the slot's mode — see
        // `family::slot_domain_siblings`. Test-only publishes bypass this.
        verter_debug_assert!(
            cached_satisfies(satisfied_projection, prepared.requested_point()),
            "warm_publish_one: {:?} records no terminal satisfying the slot's mode (§3.4)",
            prepared.key()
        );
        // The carrier is the COMPLETED self-version-rooted carrier the
        // shared cold-build helper produced via
        // `semantic_graph_read_set_signature` — it already leads with a
        // self-root `FileWholeHash` per observed self-root and merges
        // the traced cross-file fact set. `warm_publish_one` records it
        // verbatim; it never reconstructs facts from the legacy fence.
        let read_set_signature = read_set_signature.clone();
        let dispatch_dep_signature = self.dep_signature_interner.intern_in::<C::Observe>(
            crate::resolver_core::resolver_context::capture_targets(ctx),
            dispatch_dep_signature,
        );
        let direct_edges =
            reverse_index::direct_edge_count(&read_set_signature, &dispatch_dep_signature);
        let validated_at_generation = flags.current_project_generation();
        let admission_seq = self.alloc_candidate_admission_seq();
        let mut entry = MemoEntry {
            lease: match self.lease_candidate(
                family,
                delivery_lease::DeliveredAllocations::read(
                    result,
                    walker_diagnostics,
                    &dispatch_dep_signature,
                ),
            ) {
                Ok(lease) => lease,
                Err(_) => return WarmPublishOutcome::Skipped,
            },
            result: result.clone(),
            read_set_signature: read_set_signature.clone(),
            dispatch_dep_signature: dispatch_dep_signature.clone(),
            self_root_canonicals: Arc::clone(self_root_canonicals),
            walker_diagnostics: walker_diagnostics.clone(),
            satisfied_projection: satisfied_projection.clone(),
            retention: None,
            validated_at_generation,
            admission_seq,
            cost_receipt: Arc::clone(cost_receipt),
        };
        // Aggregate retention admission, BEFORE the per-family cap plan:
        // a refusal must not displace a resident candidate to make room
        // for an entry the process then declines to keep. The refusal is
        // `Skipped`, which returns this winner's COMPLETE value to its
        // caller uncached — never a stale candidate, never a partial.
        let populated_slots =
            family::publication_slot_bound(slot, satisfied_projection, requested_path);
        let retention =
            match self.reserve_memo_candidate(&entry, family, populated_slots, direct_edges) {
                Ok(retention) => retention,
                Err(_) => return WarmPublishOutcome::Skipped,
            };
        entry.retention = Some(retention);
        // Per-family bounded retention: plan the cap eviction against the
        // publishing caller's stable view BEFORE the publish lock (the
        // plan releases `entries` before validating fact rails; the
        // TOCTOU abort re-check below still runs under the publish lock,
        // so an invalidation racing the plan is caught by the fence).
        let cap = family.candidate_cap();
        let eviction =
            family::plan_family_slot_eviction(&self.entries, family, slot, &entry, cap, ctx);
        let mut entries = self.entries_lock_diagnosed();
        // Test forcing: simulate a concurrent sweep that aborted
        // this in-flight entry just before the TOCTOU re-check —
        // per-store flag, default `false`, single relaxed load on
        // the production cold path.
        if self.force_cold_abort_sweep.load(Ordering::Relaxed) {
            inflight.state.lock().aborted = true;
        }
        // Atomic re-check under the entries lock — `state` is briefly
        // locked nested inside `entries`; no AB-BA deadlock risk because
        // no path holds `state` then acquires `entries`.
        let cancelled = flags.is_cancelled();
        if cancelled {
            inflight.state.lock().aborted = true;
        }
        let aborted = inflight.state.lock().aborted;
        if aborted {
            drop(entries);
            // Canonical invalidation or a project-generation reset swept
            // this slot during the cold build; skip warm publish and
            // record the sweep. Returning `false` makes the caller skip
            // the prefix-backfill loop too — the build's ids were
            // interned against a now-stale epoch.
            record_cold_abort_swept::<C::Observe>(
                &self.stats,
                crate::resolver_core::resolver_context::capture_targets(ctx),
            );
            return WarmPublishOutcome::Aborted;
        }
        // Record whether this family is newly entering the memo so the
        // retention budget tracks one ledger record per family. A family the
        // account refuses to key is not published.
        let Some((slots, family_was_new)) = self.family_slots_locked(&mut entries, family) else {
            drop(entries);
            return WarmPublishOutcome::Skipped;
        };
        let outcome = slots.publish(slot, entry, requested_path, cap, eviction);
        let populated_slots = outcome.populated;
        // Register the newcomer in the reverse index under this same
        // `entries` hold, before the candidates it displaced are
        // unregistered below: its visibility and its registration change
        // in one step, and evidence it shares with a displaced sibling
        // stays indexed across the replacement.
        reverse_index::register_published(
            C::Observe::default(),
            crate::resolver_core::resolver_context::capture_targets(ctx),
            &self.canonical_to_entries,
            family,
            &populated_slots,
            admission_seq,
            &read_set_signature,
            &dispatch_dep_signature,
        );
        // Per-request memo-insertion attribution. Each populated slot
        // (primary plus any backfilled narrower slots) counts as one
        // insertion under the active request's audit. The host-global
        // memo size remains the canonical signal for warm-cache state;
        // the per-request mirror lets attribution tests assert
        // "no synthesis-attributable insertions during this request"
        // without false positives from peer dispatches in
        // workspace-parallel runs.
        if !populated_slots.is_empty() {
            crate::observe_request!(C::Observe => on crate::resolver_core::resolver_context::capture_targets(ctx).request.as_deref() => |ctx| {
                ctx.memo_insertions
                    .fetch_add(populated_slots.len() as u64, Ordering::Relaxed);
            });
        }
        // Drain per-candidate reverse-index registrations for every
        // candidate this publish DISPLACED (same-discriminant
        // replacements + per-family bounded-retention cap-eviction
        // victims, at whatever slot position the victim occupied). Each
        // displaced candidate's `admission_seq` keys its own
        // registrations, so siblings in the same slot keep theirs
        // (R20 overlay isolation). Runs UNDER the held `entries`
        // lock.
        for (displaced_slot, displaced_entry) in &outcome.displaced {
            reverse_index::drain_candidate_reverse_index_registrations(
                &self.canonical_to_entries,
                *displaced_slot,
                displaced_entry,
            );
        }
        // Family-memo consistency-cluster fence: the three cluster
        // members — `entries`, `memo_budget`, `canonical_to_entries` —
        // are ALL mutated WHILE the `entries` lock is still held, so the
        // publish's `(entries slot, memo_budget record, reverse-index
        // register)` triple is one atomic step against a concurrent
        // `invalidate_all` (which clears all three under the same lock).
        // `record_family_admission_locked` records the `memo_budget`
        // admission for a newly-keyed family and prunes the reverse index
        // of any FIFO victim it evicts.
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, family);
        }
        let published = PublishedMemoCandidate {
            read_set_signature,
            self_root_canonicals: Arc::clone(self_root_canonicals),
            validated_at_generation,
            admission_seq,
            cost_receipt: Arc::clone(cost_receipt),
        };
        drop(entries);
        // Published cleanly under a non-aborted in-flight entry — the
        // winner's id epoch is consistent, so the caller may proceed to
        // publish its narrower prefix-backfills.
        WarmPublishOutcome::Published(published)
    }

    /// Variant of [`Self::warm_publish_one`]: publish
    /// `(key, result, dep_signature)` into the warm map only when no
    /// entry already exists AND no concurrent in-flight build owns the
    /// key. Used by the prefix-backfill path in
    /// [`crate::project_semantic_dispatch`]'s `build_project_path` so
    /// intermediate `(base, path[..k], Navigate)` hops land in the same
    /// warm map and reverse index as cooperative-admission publishes,
    /// without racing past a concurrent cold winner that might publish
    /// a different value for the same key.
    ///
    /// Skip rules (any of which short-circuits without publishing):
    /// 1. `result` is not [`QueryResult::Value`].
    /// 2. `self.get_unvalidated(&key).is_some()` — slot is already warm.
    /// 3. The in-flight table contains `key` — a cold winner is
    ///    currently building this exact key; let it publish.
    /// 4. The parent winner's in-flight entry is `aborted` (re-checked
    ///    under the `entries` lock — see the abort fence below).
    ///
    /// **Abort fence.** The caller owns an in-flight entry (the parent
    /// cold winner whose prefix backfills these are) and passes it as
    /// `parent_inflight`. This helper re-checks `parent_inflight`'s
    /// `aborted` flag UNDER the `entries` lock, symmetric with
    /// [`Self::warm_publish_one`]'s TOCTOU re-check: if a project-
    /// generation reset ([`Self::invalidate_all`]) or a canonical
    /// invalidation aborted the parent build, every backfill it
    /// accumulated was interned against a now-stale id epoch and MUST
    /// NOT enter the memo. Making the re-check structural here — rather
    /// than only gating the caller's loop — guarantees a future caller
    /// of this helper cannot forget it: an aborted winner skips ALL its
    /// backfills.
    ///
    /// **Carrier contract.** The published `MemoEntry` stores the
    /// caller-supplied [`ReadSetSignature`] verbatim. Prefix-backfill
    /// callers must pass the parent's authoritative carrier — the
    /// path-precise traced facts captured under `install_fact_tracer`
    /// — so the backfilled entry's facts rail contains the parent's
    /// `Parse(...)` / `ResolveImports(...)` / `RouteSurface(...)`
    /// observations — never a fence-only reconstruction, which drops
    /// path-precise facts.
    fn warm_publish_one_if_absent<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature,
        dispatch_dep_signature: DepSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        satisfied_projection: MaterializedSet,
        cost_receipt: Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
        parent_inflight: &Arc<FlightCell>,
        admission_already_linearized: bool,
    ) -> bool {
        if !matches!(result, QueryResult::Value(_)) {
            return false;
        }
        // Prepare the backfill key's token once — the same
        // family/slot/path/point projection the by-key helpers ran
        // separately, plus the hash the in-flight probe needs.
        let prepared = self.prepare_rooted_key(key);
        let family = prepared.family();
        let slot = prepared.slot();
        let requested_path = prepared.requested_path();
        // §3.4 soundness invariant — same as `warm_publish_one` (a
        // prefix-backfill's `Navigate@prefix` hop is self-satisfying).
        verter_debug_assert!(
            cached_satisfies(&satisfied_projection, prepared.requested_point()),
            "warm_publish_one_if_absent: {:?} records no terminal satisfying the slot's \
             mode (§3.4)",
            prepared.key()
        );
        // Skip if already warm OR currently in flight. Both checks
        // happen BEFORE acquiring the entries lock; a concurrent cold
        // winner publish that lands between this check and the publish
        // is benign (FamilySlots::publish overrides; both are computing
        // the same canonical prefix node so values agree).
        //
        // A bare presence probe (never `get`) so the check does NOT
        // bubble a stale entry's facts into the outer tracer before
        // declining to publish.
        {
            let entries = self.entries_lock_diagnosed();
            if entries
                .get(family)
                .is_some_and(|slots| slots.slot_peek_any(slot).is_some())
            {
                return false;
            }
        }
        if self.inflight.lock().contains_key(&prepared)
            || self.expression_inflight.lock().contains_key(&prepared)
        {
            return false;
        }
        // Past both early-outs: this backfill genuinely clones the broader
        // result into an empty narrower sibling slot.
        verter_audit::attribute!(CacheCandidateCopy);
        let dispatch_dep_signature = self.dep_signature_interner.intern_in::<C::Observe>(
            crate::resolver_core::resolver_context::capture_targets(ctx),
            &dispatch_dep_signature,
        );
        let direct_edges =
            reverse_index::direct_edge_count(&read_set_signature, &dispatch_dep_signature);
        let validated_at_generation = flags.current_project_generation();
        let admission_seq = self.alloc_candidate_admission_seq();
        let mut entry = MemoEntry {
            lease: Default::default(),
            result: match result {
                QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                QueryResult::Recursive(node) => QueryResult::Recursive(node),
                QueryResult::Error(error) => QueryResult::Error(error),
            },
            read_set_signature: read_set_signature.clone(),
            dispatch_dep_signature: dispatch_dep_signature.clone(),
            self_root_canonicals,
            walker_diagnostics: empty_diagnostics(),
            satisfied_projection,
            retention: None,
            validated_at_generation,
            admission_seq,
            cost_receipt,
        };
        entry.lease = match self.lease_candidate(
            family,
            delivery_lease::DeliveredAllocations::read(
                &entry.result,
                &entry.walker_diagnostics,
                &entry.dispatch_dep_signature,
            ),
        ) {
            Ok(lease) => lease,
            Err(_) => return false,
        };
        // A narrower sibling slot is a fresh candidate: its own bookkeeping
        // is charged like any other publish, while the carrier it shares
        // with the broader entry is held through the same shared charge.
        // Under pressure the backfill is simply declined — the broader
        // entry it would have been cloned from stays warm and a later
        // narrow request recomputes.
        let populated_slots =
            family::publication_slot_bound(slot, &entry.satisfied_projection, requested_path);
        match self.reserve_memo_candidate(&entry, family, populated_slots, direct_edges) {
            Ok(retention) => entry.retention = Some(retention),
            Err(_) => return false,
        }
        // Per-family bounded retention: plan the cap eviction against the
        // publishing caller's stable view BEFORE the publish lock —
        // symmetric with `warm_publish_one` (a concurrent cold winner may
        // have filled the slot to its family cap between the presence
        // probe above and this publish). The plan releases `entries`
        // before validating fact rails; the parent abort fence below
        // still runs under the publish lock.
        let cap = family.candidate_cap();
        let eviction =
            family::plan_family_slot_eviction(&self.entries, family, slot, &entry, cap, ctx);
        let mut entries = self.entries_lock_diagnosed();
        // Abort fence — re-check the parent winner's in-flight `aborted`
        // flag UNDER the `entries` lock, symmetric with
        // `warm_publish_one`. `invalidate_all` sets `aborted` and clears
        // `entries` under the SAME `entries` lock; acquiring it here
        // serialises this backfill publish against that reset. If the
        // parent was aborted (a project-generation reset or canonical
        // invalidation raced the parent cold build), the parent's — and
        // therefore this backfill's — `SemanticNodeId`s were interned
        // against a now-stale id epoch: skip the publish so no stale
        // warm slot survives the reset.
        if !admission_already_linearized && flags.is_cancelled() {
            parent_inflight.state.lock().aborted = true;
        }
        if parent_inflight.state.lock().aborted {
            drop(entries);
            record_cold_abort_swept::<C::Observe>(
                &self.stats,
                crate::resolver_core::resolver_context::capture_targets(ctx),
            );
            return false;
        }
        let Some((slots, family_was_new)) = self.family_slots_locked(&mut entries, family) else {
            drop(entries);
            return false;
        };
        let outcome = slots.publish(slot, entry, requested_path, cap, eviction);
        let populated_slots = outcome.populated;
        // Register the newcomer in the reverse index under this same
        // `entries` hold, before the candidates it displaced are
        // unregistered below: its visibility and its registration change
        // in one step, and evidence it shares with a displaced sibling
        // stays indexed across the replacement.
        reverse_index::register_published(
            C::Observe::default(),
            crate::resolver_core::resolver_context::capture_targets(ctx),
            &self.canonical_to_entries,
            family,
            &populated_slots,
            admission_seq,
            &read_set_signature,
            &dispatch_dep_signature,
        );
        // Per-request memo-insertion attribution — see
        // `warm_publish_one` for the full rationale; the prefix-backfill
        // path bumps the same per-request counter so attribution
        // tests cover both publish sites.
        if !populated_slots.is_empty() {
            crate::observe_request!(C::Observe => on crate::resolver_core::resolver_context::capture_targets(ctx).request.as_deref() => |ctx| {
                ctx.memo_insertions
                    .fetch_add(populated_slots.len() as u64, Ordering::Relaxed);
            });
        }
        // Drain displaced candidates' per-candidate registrations —
        // see `warm_publish_one` for the rationale.
        for (displaced_slot, displaced_entry) in &outcome.displaced {
            reverse_index::drain_candidate_reverse_index_registrations(
                &self.canonical_to_entries,
                *displaced_slot,
                displaced_entry,
            );
        }
        // Family-memo consistency-cluster fence — record the
        // `memo_budget` admission under the held `entries` lock; see
        // `warm_publish_one`.
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, family);
        }
        let admitted = !populated_slots.is_empty();
        drop(entries);
        admitted
    }
}

// `publish_warm_if_absent` was an immediate-publish path-prefix
// backfill API used by `backfill_prefixes` in `build_project_path`.
// Carrier-aware deferral retired it: backfills now accumulate onto
// `QueryBuildOutput.pending_prefix_backfills` and the cooperative
// admission flow publishes them via `warm_publish_one_if_absent`
// AFTER the parent's `install_fact_tracer` finalises. This guarantees
// each backfilled memo entry's carrier holds the parent's
// authoritative path-precise fact signature instead of a fence-only
// derivation that drops Parse/ResolveImports/RouteSurface facts.

/// One observed self-root: a query-identity memo entry's keyed (or
/// file-derived input) canonical paired with the content version the
/// builder actually computed the value against.
///
/// The hash is the file's whole-hash **as observed at value-compute
/// time** — never re-read at signature-build time. Builders capture it
/// once at the value source (a [`crate::resolver_core::MaterializeScopeObservation`],
/// a [`crate::semantic_query::DeclIdentity`]'s `whole_hash`, or a
/// file-derived input node's [`crate::semantic_query::NodeScopeId::File`])
/// and thread it here. Rooting the entry on the observed version is what
/// makes a same-canonical content edit miss the warm read: a re-read of
/// current content inside the signature builder would reopen the publish
/// race a concurrent `upsert` between value-compute and signature-build
/// otherwise creates.
pub type ObservedGraphSelfRoot = (
    verter_type_expr::OwnedStr,
    verter_session_query::analysis::types::Hash16,
);

/// Build the [`ReadSetSignature`](verter_session_query::facts::fact_cache::ReadSetSignature)
/// carrier for a [`SemanticGraphStore`] query-identity memo entry —
/// **provenance-pure**.
///
/// A semantic-graph memo entry caches a resolved [`SemanticNodeId`] for
/// one [`SemanticQueryKey`]. The entry is self-version-rooted: its
/// signature leads with a `FileWholeHash` fact for every canonical the
/// builder's value depends on for its own identity (its keyed canonical,
/// or — for node kinds keyed by already-interned input nodes — the
/// file-derived origin of each input node). A warm read validates those
/// self-roots strictly through
/// [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`],
/// so a same-canonical content edit — or a keyed canonical the live
/// store view no longer tracks — rejects the entry and forces a
/// recompute.
///
/// The builder NEVER re-reads current content. `observed_self_roots`
/// carries each `(canonical, observed_hash)` pair the cold build
/// captured at the value source; `traced_facts` is the path-precise
/// fact set the `install_fact_tracer` scope collected. Re-reading a
/// canonical's current hash here would reopen the publish race the
/// central fact-signature helpers close.
///
/// Returns `None` — refusing shared-cache admission — when the entry
/// cannot be soundly rooted. The cooperative-admission caller still
/// returns the freshly-computed value; it only forgoes the shared memo.
/// `None` is returned when:
///
/// - two `observed_self_roots` name the same canonical with conflicting
///   observed hashes (a torn observation), or
/// - a `traced` `FileWholeHash` fact names a self-root canonical with a
///   hash that disagrees with the observed self-root (the traced
///   dependency rail and the observed self-root disagree on the keyed
///   file's version).
///
/// The traced fact set is merged after the self-roots: a self-root
/// `FileWholeHash` already emitted is not duplicated, but every other
/// traced fact (cross-file `Parse` / `ResolveImports` / `RouteSurface`
/// dependency facts, `DerivedFactHash`, `ProjectGeneration`) is kept so
/// transitive invalidation still works.
//
// arch-guard:graph-signature-builder-provenance-pure — this function
// (and any helper it calls to build facts) must stay provenance-pure.
// The guard `semantic_graph_signature_builder_is_provenance_pure`
// (`tests/cases/g_misc1/semantic_graph_signature_builder_provenance.rs`) bans
// `authoritative_current_content_hash`, `current_file_facts`,
// `parse_fact_ref(`, `self_root_fact`, and `shallow_file_state` inside
// this body.
pub fn semantic_graph_read_set_signature(
    view: &dyn verter_session_query::facts::store_view::StoreView,
    observed_self_roots: &[ObservedGraphSelfRoot],
    traced_facts: &[verter_session_query::facts::fact_cache::FactVersionRef],
) -> Result<
    crate::fact_signature_helpers::StructuralCarrierReadSet,
    crate::cache_runtime::NonAdmissionReason,
> {
    semantic_graph_carrier(
        view,
        observed_self_roots,
        &verter_session_query::facts::fact_set::FactSet::from_facts(traced_facts.to_vec()),
        Vec::new(),
    )
}

/// [`semantic_graph_read_set_signature`] over traced facts the caller
/// hands over: the carrier is built from them in place.
pub fn semantic_graph_carrier(
    view: &dyn verter_session_query::facts::store_view::StoreView,
    observed_self_roots: &[ObservedGraphSelfRoot],
    traced: &verter_session_query::facts::fact_set::FactSet,
    extra: Vec<verter_session_query::facts::fact_cache::FactVersionRef>,
) -> Result<
    crate::fact_signature_helpers::StructuralCarrierReadSet,
    crate::cache_runtime::NonAdmissionReason,
> {
    use verter_session_query::facts::fact_cache::FactVersionRef;

    // Collapse the observed self-roots per canonical; a conflicting hash for
    // the same canonical is a torn observation. A build observes a handful
    // of roots, so they are compared linearly, in plain loops: this runs
    // once per cold build.
    let mut self_root_hashes: smallvec::SmallVec<
        [(
            &verter_type_expr::OwnedStr,
            verter_session_query::analysis::types::Hash16,
        ); 4],
    > = smallvec::SmallVec::new();
    for (canonical, observed_hash) in observed_self_roots {
        match root_position(&self_root_hashes, canonical) {
            Some(index) if self_root_hashes[index].1 != *observed_hash => {
                return Err(crate::cache_runtime::NonAdmissionReason::SelfRootConflict)
            }
            Some(_) => {}
            None => self_root_hashes.push((canonical, *observed_hash)),
        }
    }

    // Every traced `FileWholeHash` for a self-root canonical MUST agree
    // with the observed hash (else the dependency rail and the observed
    // self-root disagree on the keyed file's version — a torn read). A
    // wide traced set is checked like a narrow one: every fact is part of
    // the rail, so a torn root has nowhere to hide.
    // The traced set is read in place: one file's whole-file facts sit side
    // by side in canonical order, so each root reads only its own.
    if !self_root_hashes.is_empty() {
        for (canonical, observed) in &self_root_hashes {
            let from = FactVersionRef::FileWholeHash {
                canonical_id: Arc::from(canonical.as_str()),
                hash: [0; 16],
            };
            let traced_root = traced.iter_from(&from).take_while(|fact| {
                matches!(fact, FactVersionRef::FileWholeHash { canonical_id, .. }
                    if **canonical_id == *canonical.as_str())
            });
            for fact in traced_root.chain(extra.iter()) {
                if let FactVersionRef::FileWholeHash { canonical_id, hash } = fact {
                    if **canonical_id == *canonical.as_str() && hash != observed {
                        return Err(crate::cache_runtime::NonAdmissionReason::SelfRootConflict);
                    }
                }
            }
        }
    }

    // One self-root `FileWholeHash` per observed self-root, pinned to the
    // OBSERVED content version, beside the traced fact set kept verbatim
    // (shared, never copied) so transitive cross-file invalidation is
    // preserved. A traced `FileWholeHash` for a self-root canonical already
    // is that fact — the check above proved its hash the observed one — so
    // only the roots the traced set does not name are added.
    let mut named = smallvec::SmallVec::<[bool; 4]>::from_elem(false, self_root_hashes.len());
    for (index, (canonical, observed)) in self_root_hashes.iter().enumerate() {
        let fact = FactVersionRef::FileWholeHash {
            canonical_id: Arc::from(canonical.as_str()),
            hash: *observed,
        };
        named[index] = traced.contains(&fact) || extra.contains(&fact);
    }
    let mut facts = extra;
    let mut self_root_canonicals: Vec<Arc<str>> = Vec::with_capacity(self_root_hashes.len());
    for (index, (canonical, observed_hash)) in self_root_hashes.iter().enumerate() {
        let canonical = Arc::<str>::from(*canonical);
        if !named[index] {
            facts.push(FactVersionRef::FileWholeHash {
                canonical_id: Arc::clone(&canonical),
                hash: *observed_hash,
            });
        }
        self_root_canonicals.push(canonical);
    }
    crate::fact_signature_helpers::bound_completed_structural_carrier(
        view,
        traced,
        facts,
        self_root_canonicals,
    )
}

// ──────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────

/// The position of `canonical` among collapsed self-roots.
fn root_position(
    roots: &[(
        &verter_type_expr::OwnedStr,
        verter_session_query::analysis::types::Hash16,
    )],
    canonical: &str,
) -> Option<usize> {
    for (index, (root, _)) in roots.iter().enumerate() {
        if root.as_ref() == canonical {
            return Some(index);
        }
    }
    None
}

/// The empty dispatch dependency signature: one shared allocation, so a
/// result that depends on no dispatch fence costs no allocation.
pub fn empty_signature() -> DepSignature {
    static EMPTY: std::sync::OnceLock<DepSignature> = std::sync::OnceLock::new();
    EMPTY.get_or_init(DepSignature::static_empty).clone()
}

/// The one process-wide empty walker-diagnostic list. Like
/// [`empty_signature`], it is process-static storage, never charged: a read
/// with no diagnostics shares it instead of allocating its own.
pub fn empty_diagnostics() -> crate::project_semantic_dispatch::walk::WalkerDiagnostics {
    static EMPTY: std::sync::OnceLock<crate::project_semantic_dispatch::walk::WalkerDiagnostics> =
        std::sync::OnceLock::new();
    EMPTY
        .get_or_init(crate::project_semantic_dispatch::walk::WalkerDiagnostics::static_empty)
        .clone()
}

// ──────────────────────────────────────────────────────────────────────────
// Counter helpers (Decision #5: single helper, dual-target write)
// ──────────────────────────────────────────────────────────────────────────
//
// `inflight_aborted_retries` and `cold_aborts_swept` write both the global
// aggregate AND the per-request mirror through one helper so the two halves
// cannot diverge. Architecture guard `audit_counter_single_helper` rejects
// any direct `self.stats.<counter>.fetch_add` for these two counters
// outside the helper bodies in this module.

/// Bump `inflight_aborted_retries` on both the global
/// `SemanticGraphStats` AND, when a `RequestContext` is installed in
/// TLS on the calling thread, the per-request mirror.
///
/// The global write is unconditional — non-audited callers continue
/// to observe the counter via [`SemanticGraphStore::stats_snapshot`].
/// The per-request write surfaces in the audit miner's
/// `CacheOutcomeTally` (see
/// `component_meta_audit/footprint_miner.rs`).
///
/// `RequestContext` also implements [`verter_audit::AuditObserver`],
/// so `verter_audit::current_observer()` reaches the same per-request
/// counters via the substrate-side TLS slot. Producers outside
/// `verter_session` use the substrate accessor; this in-crate helper
/// keeps the typed direct path for the per-request mirror to avoid an
/// unnecessary vtable hop on the hot loop.
#[inline]
fn record_inflight_aborted_retry<M: verter_audit::observe::ObserveExec>(
    stats: &AtomicSemanticGraphStats,
    observation: &crate::request_observers::CaptureTargets,
) {
    let _ = observation;
    if records_telemetry::<M>() {
        stats
            .inflight_aborted_retries
            .fetch_add(1, Ordering::Relaxed);
    }
    crate::observe_request!(M => on observation.request.as_deref() => |ctx| {
        ctx.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
    });
}

/// Bump `cold_aborts_swept` on both the global `SemanticGraphStats`
/// AND, when a `RequestContext` is installed in TLS on the calling
/// thread, the per-request mirror.
///
/// Mirrors [`record_inflight_aborted_retry`] for the cold-abort sweep
/// counter that the TOCTOU re-check fires on a swept publish.
#[inline]
fn record_cold_abort_swept<M: verter_audit::observe::ObserveExec>(
    stats: &AtomicSemanticGraphStats,
    observation: &crate::request_observers::CaptureTargets,
) {
    let _ = observation;
    if records_telemetry::<M>() {
        stats.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
    }
    crate::observe_request!(M => on observation.request.as_deref() => |ctx| {
        ctx.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
    });
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod scc_publish_tests;
#[cfg(test)]
mod storage_charge_tests;
