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
//! - [`SemanticNodeId`] is stable for the lifetime of the memo. Node data
//!   is stored in an append-only arena so readers can hold a long-lived
//!   id without worrying about resizing.
//! - **Same-path recursion** returns `QueryResult::Recursive(self_id)`
//!   so cycles dedup rather than re-entering.
//! - **Distinct top-level waiters** block cooperatively on a per-entry
//!   [`Condvar`] pairing (see [`InflightEntry`]).
//! - Cancelled, budget-exceeded, or partial results **never** promote to a
//!   warm memo entry; they surface as [`QueryError`] variants and the
//!   in-flight admission is removed so the next caller starts fresh.
//! - Entries are immutable once stored. Node data never retains borrowed
//!   OXC AST pointers — callers materialize semantic data before calling
//!   [`SemanticGraphStore::intern_node`].

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::instant::Instant;

use dashmap::DashMap;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::semantic_query::{
    CacheRead, DepSignature, HostResolvedNamedTypeKey, NodeScopeId, OriginEdge, OriginEdgeKind,
    QueryError, QueryResult, SemanticGraphRead, SemanticGraphStats, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};
#[cfg(test)]
use crate::semantic_query::{PathSegment, ProjectionMode};

mod arena;
mod budgeted_caches;
mod derivation;
mod family;
mod inflight;
mod interner;
mod stats;
#[cfg(any(test, debug_assertions))]
mod test_gates;

#[allow(unused_imports)]
pub use interner::DepSignatureInterner;
#[cfg(test)]
use interner::SWEEP_INTERVAL;

use arena::NodeArena;
#[cfg(test)]
use arena::{shard_index_for, NUM_SHARDS};
use budgeted_caches::{BudgetedNamedTypeIndex, BudgetedRelationMemo};
use derivation::{sorted_percentile, DerivationStore};
pub use family::AuditEagerKeyRow;
use family::{
    carrier_facts_reference_canonical, dep_signature_references_canonical, family_and_slot,
    FamilyKey, FamilySlots, MemoEntry, ModeSlot,
};
use inflight::{
    InflightEntry, InflightPanicGuard, RecursionStackGuard, IN_FLIGHT_ON_THIS_THREAD,
    MAX_INFLIGHT_RETRIES,
};
use stats::{AtomicSemanticGraphStats, EntriesLockGuard, InFlightStatsGuard};

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
///
/// ## Vue macro resolution identity map
///
/// The [`named_type_index`](Self::named_type_index) `DashMap` is a secondary
/// identity table that lets the parser's
/// [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
/// adapter hit the shared graph in refcount-only time. Reads go
/// `key → SemanticNodeId → SemanticNodeData::VueMacroElements(arc) →
/// arc.clone()`: the hot path pays one `DashMap::get` + one arena read +
/// one `Arc::clone`, matching the retired `ResolvedNamedTypesDb`'s
/// cost profile.
///
/// Entries are whole-hash-scoped (the key carries `whole_hash`) so reads
/// are self-validating within one workspace content generation. The
/// formal `execute_cooperative` path is not in the read hot path — writes
/// enter through [`SemanticGraphStore::insert_resolved_named_type`] from
/// the adapter side.
#[derive(Default)]
pub struct SemanticGraphStore {
    arena: NodeArena,
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
    /// **Backfill on completion:** when a broader-mode build publishes its
    /// result, it also writes that result into every empty narrower-mode
    /// slot in the same family — `Expanded` backfills `Shallow` /
    /// `Navigate` / `Identity`, `Shallow` backfills `Navigate` /
    /// `Identity`, `Navigate` backfills `Identity`. Narrower builds NEVER
    /// backfill broader slots. Backfill writes only into empty slots, so a
    /// concurrent narrower build that already populated its slot is never
    /// pre-empted.
    entries: Mutex<FxHashMap<FamilyKey, FamilySlots>>,
    /// In-flight admission keyed by the full [`SemanticQueryKey`]. Because
    /// mode is part of the key for mode-bearing variants, this keying
    /// gives per-`(family, mode_slot)` in-flight authority
    /// concurrent `Navigate` and `Expanded` builds on the same family run
    /// as two independent in-flight entries.
    inflight: Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
    /// Identity map for Vue macro resolution artifacts keyed by
    /// [`HostResolvedNamedTypeKey`]. See the struct-level docs for the
    /// read-path shape. `SemanticQueryKey::ResolvedNamedType` bypasses
    /// the family memo entirely — this map is the cache, and
    /// `execute_cooperative` short-circuits straight to the build
    /// closure for that variant.
    ///
    /// The map and its retention budget are mutated within one lock
    /// domain ([`BudgetedNamedTypeIndex`]) — every insert / per-canonical
    /// drain runs under the wrapper's `retention_gate` read guard,
    /// `clear` under its write guard — so a concurrent insert can never
    /// strand a live map entry whose budget admission a project-
    /// generation `clear` then erases.
    /// The monotonic resolved-named-type reset epoch is owned INSIDE
    /// [`BudgetedNamedTypeIndex`]: [`Self::invalidate_all`] bumps it under
    /// the same `retention_gate.write()` that clears the map+budget, and
    /// the [`Self::insert_resolved_named_type`] fence checks it under the
    /// same `retention_gate.read()` that performs the insert — so a
    /// straggler insert is fully ordered against a project-generation
    /// reset with no window (see `BudgetedNamedTypeIndex`'s docs).
    named_type_index: BudgetedNamedTypeIndex,
    /// Relation-engine memo. Maps `(source, target)` semantic-node pairs
    /// to the tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) plus the
    /// self-version-rooted carrier + the self-root canonical set used
    /// for strict warm-hit validation. Separate from the family memo
    /// because relation identity is pairwise, not single-node.
    ///
    /// The stored `RelationMemoEntry` is validated on every warm read
    /// (`get_relation`) — every self-root canonical's `FileWholeHash` is
    /// validated strictly, so a same-canonical content edit to either
    /// the source's or the target's originating file misses the warm
    /// relation judgement and forces a recompute.
    ///
    /// The map and its retention budget are mutated within one lock
    /// domain ([`BudgetedRelationMemo`]) — `insert` runs under the
    /// wrapper's `retention_gate` read guard, `clear` under its write
    /// guard.
    relation_memo: BudgetedRelationMemo,
    /// Sibling derivation/origin layer. Edges are keyed by
    /// `(result_node, kind)`; multiple derivations of the same structural
    /// result store multiple edges per key. Edge dep-signatures are
    /// interned in the store's signature pool so per-builder fence
    /// snapshots share allocations. Origin edges are bounded best-effort
    /// provenance for the audit origin-graph trace, NOT an invalidation
    /// source — see the `derivation` module docs.
    derivation: Mutex<DerivationStore>,
    /// Lock-free telemetry counters. Read via [`Self::stats_snapshot`]
    /// into the public [`SemanticGraphStats`] surface.
    stats: AtomicSemanticGraphStats,
    /// Path C C1 contention instrumentation. Mirrors the arena's
    /// `provenance` field: `Some` for stores wired up by the host, `None`
    /// for the test-default stores constructed via `Default`. Used by
    /// `execute_cooperative` to bucket owner vs joiner paths and held
    /// time on `MetaProvenance`.
    provenance: Option<Arc<crate::types::MetaProvenance>>,
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
    #[cfg(any(test, debug_assertions))]
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
    #[cfg(any(test, debug_assertions))]
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
    #[cfg(any(test, debug_assertions))]
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
    #[cfg(any(test, debug_assertions))]
    cold_winner_pre_backfill_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
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
    #[cfg(any(test, debug_assertions))]
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
    #[cfg(any(test, debug_assertions))]
    publish_post_reverse_index_prune_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Per-store test-only injection point inside [`Self::invalidate_all`]'s
    /// in-flight abort loop, fired while iterating the COLLECTED entry
    /// handles and locking each entry's `state` — with the `inflight`
    /// table lock NOT held. A race test arms it (via
    /// [`Self::test_invalidate_all_inflight_abort_gate`]) and asserts
    /// `inflight.try_lock()` is `Some`, proving the collect-then-release
    /// lock order that rules out an AB-BA with `InflightPanicGuard::drop`.
    /// Per-store scoped; `cfg`-gated to `test` / `debug_assertions`.
    #[cfg(any(test, debug_assertions))]
    invalidate_all_inflight_abort_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Γ.B reverse index. For each canonical id,
    /// holds the set of `(family, slot)` pairs whose published
    /// dep_signature references it, paired with the dep_signature
    /// `Arc` that was registered. `invalidate_canonical` consults
    /// this map instead of linearly scanning the family memo.
    ///
    /// **`Arc` discrimination.** When evicting an entry the registered
    /// `dep_signature` Arc is `ptr_eq`-compared against the current
    /// entry's dep_signature. Under Γ.C interning this Arc is
    /// shared across equivalent dep_signatures so ptr_eq matches a
    /// concurrent fresh write only when its content really is the
    /// same; pre-Γ.C the registered Arc is the exact one the publish
    /// path stored, so ptr_eq distinguishes our entry from any later
    /// fresh build's distinct Arc.
    ///
    /// **Lock order — `entries → canonical_to_entries shards`.** The
    /// family `entries` `Mutex` is OUTERMOST; a `canonical_to_entries`
    /// shard mutex (the `DashMap` per-canonical `Mutex`) is INNER. The
    /// publish-side registration, the [`Self::invalidate_all`] clear,
    /// the FIFO budget-eviction cleanup, and the
    /// [`Self::invalidate_canonical`] drain all mutate
    /// `canonical_to_entries` WHILE holding `entries` — that is exactly
    /// what this order permits, and it makes the family memo's
    /// three-member consistency cluster (`entries`, `memo_budget`,
    /// `canonical_to_entries`) mutate atomically. The order is
    /// load-bearing in one direction only: NO path may acquire a
    /// `canonical_to_entries` shard mutex and THEN the `entries` `Mutex`
    /// — that would be an AB-BA deadlock. Every reverse-index helper
    /// (`register_reverse_index`, `prune_reverse_index_registration`)
    /// takes only shard mutexes and never re-enters `entries`, so the
    /// order holds.
    canonical_to_entries: CanonicalToEntries,
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
    /// with no registration. The `entries → canonical_to_entries shards`
    /// lock order permits taking a shard mutex while `entries` is held.
    memo_budget: crate::bounded_query_retention::GlobalRetentionBudget<FamilyKey>,
}

/// Γ.B reverse-index type alias. See
/// [`SemanticGraphStore::canonical_to_entries`] for the contract.
type CanonicalToEntries = DashMap<Arc<str>, Mutex<FxHashMap<(FamilyKey, ModeSlot), DepSignature>>>;

impl std::fmt::Debug for SemanticGraphStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticGraphStore")
            .field("nodes", &self.arena.len())
            .field("memo_entries", &self.memo_entry_count())
            .field("named_type_entries", &self.named_type_index.len())
            .finish_non_exhaustive()
    }
}

impl SemanticGraphStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only constructor pinning a small `memo_budget` cap. Lets a
    /// reverse-index test drive FIFO family eviction without admitting
    /// the production [`crate::bounded_query_retention::DEFAULT_BUDGET_CAP`]
    /// (4096) families. Every other field is the `Default` value.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_memo_budget_for_test(memo_budget_cap: usize) -> Self {
        Self {
            memo_budget: crate::bounded_query_retention::GlobalRetentionBudget::new(
                memo_budget_cap,
            ),
            ..Self::default()
        }
    }

    /// Test-only — number of distinct outer shards currently resident
    /// in the Γ.B `canonical_to_entries` reverse index (one shard per
    /// canonical that has, or has had, a registration). A budget
    /// eviction that empties a shard's inner map must drop the outer
    /// shard, so this count tracks the surviving canonicals — not the
    /// lifetime canonical count.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn canonical_to_entries_shard_count_for_test(&self) -> usize {
        self.canonical_to_entries.len()
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
        let wait_start = Instant::now();
        let guard = self.entries.lock();
        let wait_ns = wait_start.elapsed().as_nanos();
        EntriesLockGuard {
            guard: Some(guard),
            hold_start: Instant::now(),
            wait_ns,
        }
    }

    /// Construct a store wired to the host's
    /// [`MetaProvenance`](crate::types::MetaProvenance) so the underlying
    /// [`NodeArena`] and `execute_cooperative` path record Path C C1
    /// instrumentation. Test-only direct constructions keep using
    /// [`Self::new`] / [`Self::default`] (provenance stays `None`).
    ///
    /// The constructor installs provenance via field mutation on a
    /// `Default`-built store so it stays compatible with the dispatch
    /// invariant tests that require single-owner cardinality for
    /// `arena: NodeArena` and `relation_memo: BudgetedRelationMemo` in
    /// production code.
    #[must_use]
    pub fn with_provenance(provenance: Arc<crate::types::MetaProvenance>) -> Self {
        let mut store = Self::default();
        store.arena.provenance = Some(Arc::clone(&provenance));
        store.provenance = Some(provenance);
        store
    }

    /// Public read accessor for the shared
    /// [`crate::types::MetaProvenance`] handle the store was constructed
    /// with. Returns `None` for `Default`-built stores (test-default
    /// path); host-built stores always return `Some`. Block 1.C uses
    /// this to bump `slot_binding_graph_*` counters from the
    /// `meta_resolve::slot_binding_graph` module without threading a
    /// `&VerterHost` reference through every helper signature.
    #[must_use]
    pub fn provenance(&self) -> Option<&Arc<crate::types::MetaProvenance>> {
        self.provenance.as_ref()
    }

    /// Intern a new immutable [`SemanticNodeData`] and return its stable id.
    ///
    /// The interned node records [`NodeScopeId::Global`] in the origin
    /// sidecar (see [`Self::node_scope`]) — use
    /// [`Self::intern_node_with_scope`] when the node's origin scope is
    /// known (declaration anchors, instantiated shells, surface members
    /// whose value carries a declaration identity, etc.).
    ///
    /// [`SemanticNodeData::VueMacroElements`] nodes are sidecar-exempt per
    /// their sidecar slot is forced to `None` structurally,
    /// regardless of which intern entry point is used.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.arena.push(data)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Dispatch
    /// builders that know the node's declaration origin (e.g.
    /// `build_resolve_decl` / `build_typeof` / `build_instantiate`) use
    /// this entry point so per-base-scope routing via [`Self::node_scope`]
    /// returns the originating scope later.
    ///
    /// [`SemanticNodeData::VueMacroElements`] nodes are sidecar-exempt per
    ///; passing a non-`Global` scope has no effect for that
    /// variant — the sidecar slot is forced to `None` structurally.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.arena.push_with_scope(data, scope)
    }

    /// Intern a rebuilt shell `data` while preserving the scope of an
    /// `origin` shell (Path C C6a items 4-5).
    ///
    /// **Invariant** (per Claude Code R2): when a rebuilt shell `X'`
    /// is derived from `X` with substituted sub-expressions,
    /// `node_scope(X') == node_scope(X)`. Used by
    /// [`crate::project_semantic_dispatch::ProjectSemanticDispatch::substitute_semantic_type_param`]
    /// and any other shell-rebuild site that previously called the
    /// scope-less `intern_node` and would otherwise drop the origin
    /// scope under C7's compound `(payload, scope)` interning.
    ///
    /// Falls back to [`NodeScopeId::Global`] when `origin`'s sidecar
    /// is empty (e.g., the origin is a `VueMacroElements` exempt
    /// slot, or `origin` is out of bounds). The fallback preserves
    /// pre-C6a behaviour for these cases — they were already
    /// scope-less.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_preserving_scope(
        &self,
        origin: SemanticNodeId,
        data: SemanticNodeData,
    ) -> SemanticNodeId {
        self.stats
            .intern_preserving_scope_calls
            .fetch_add(1, Ordering::Relaxed);
        let scope = self.node_scope(origin).unwrap_or(NodeScopeId::Global);
        self.arena.push_with_scope(data, scope)
    }

    /// Test/diagnostic — read the cumulative count of
    /// `intern_preserving_scope` calls. /
    /// discriminating signal for the substitute change-tracking
    /// optimization: a no-op substitution must increment this
    /// counter by zero post-Fix-D.
    #[must_use]
    pub fn intern_preserving_scope_call_count(&self) -> u64 {
        self.stats
            .intern_preserving_scope_calls
            .load(Ordering::Relaxed)
    }

    /// Return the recorded origin scope for `id`.
    ///
    /// Returns:
    /// - `None` — `id` is an exempt [`SemanticNodeData::VueMacroElements`]
    ///   node, or the id is out of bounds for the arena.
    /// - `Some(NodeScopeId::Global)` — scope-less structural node
    ///   (primitive, shared literal-union, helper intermediate).
    /// - `Some(NodeScopeId::File { .. })` — declaration-bound node whose
    ///   origin scope is the recorded `(canonical_id, whole_hash,
    ///   local_scope)` triple.
    ///
    /// The sidecar records the scope at the moment of **first intern**; a
    /// reader that calls `node_scope(id)` from a different scope observes
    /// the origin scope, not their own.
    #[must_use]
    pub fn node_scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        self.arena.scope(id)
    }

    /// Read the resolved payload for a semantic node id. Returns `None` if
    /// the id has not been interned.
    #[must_use]
    pub fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.arena.get(id)
    }

    /// Number of interned semantic nodes. Useful for tests and counters.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    /// Number of warm memo entries — sums populated slots across every
    /// family. Useful for tests and counters. Two distinct mode slots in
    /// the same family count as two entries.
    #[must_use]
    pub fn memo_entry_count(&self) -> usize {
        self.entries
            .lock()
            .values()
            .map(FamilySlots::populated_count)
            .sum()
    }

    /// Test-only accessor returning the memo's populated-slot count
    /// for a given host. Used by the slot-binding regression
    /// `cache_suppress_true_skips_memo_insertion` to inspect memo
    /// growth before and after a synthesis call. Equivalent to
    /// `host.project_type_store().semantic_graph().memo_entry_count()`.
    #[cfg(test)]
    pub fn memo_size_in_test(host: &crate::VerterHost) -> usize {
        host.project_type_store()
            .semantic_graph()
            .memo_entry_count()
    }

    /// Test-only — number of distinct `FamilyKey`s currently resident in
    /// the warm `entries` memo. The `memo_budget` retention ledger is
    /// keyed by family, so the map/budget lifecycle-fence tests compare
    /// this against [`Self::memo_budget_tracked_len_for_test`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn memo_family_count_for_test(&self) -> usize {
        self.entries.lock().len()
    }

    /// Test-only — number of admission records currently in the
    /// `memo_budget` retention ledger. With the map/budget lifecycle
    /// fence in place this stays consistent with
    /// [`Self::memo_family_count_for_test`]: every live `entries` family
    /// has exactly one ledger record, so a desync (a live family with no
    /// record, invisible to FIFO eviction) is observable as a mismatch.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn memo_budget_tracked_len_for_test(&self) -> usize {
        self.memo_budget.tracked_len()
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
                let dep_signature = format!("{:?}", slot.read_set_signature.legacy);
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

    /// Number of `(family, slot)` registrations under `canonical_id`
    /// in the Γ.B `canonical_to_entries` reverse index. Returns 0 when
    /// the canonical is not present. Test/diagnostic accessor — plan
    /// §6 / §13.2.
    #[must_use]
    pub fn canonical_to_entries_count(&self, canonical_id: &str) -> usize {
        self.canonical_to_entries
            .get(canonical_id)
            .map(|shard| shard.value().lock().len())
            .unwrap_or(0)
    }

    /// Invalidate every warm memo slot whose stored `DepSignature`
    /// references `canonical_id` (plan B3 dep-signature sweep, replacing
    /// the pre-B3 conservative `family_references_canonical` helper).
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
    pub fn invalidate_canonical(&self, canonical_id: &str) -> usize {
        use rustc_hash::FxHashSet;

        // The family memo is a three-member consistency cluster —
        // `entries` (the memo map), `memo_budget` (its FIFO ledger), and
        // `canonical_to_entries` (this reverse index). Every mutation
        // that touches more than one of them runs under ONE lock domain,
        // the `entries` `Mutex`. This per-canonical drain mutates all
        // three, so the whole drain — the `canonical_to_entries` shard
        // drain for `canonical_id`, the `entries` slot eviction +
        // `memo_budget` forget, AND the cross-canonical
        // `canonical_to_entries` cleanup — runs under a SINGLE
        // `entries`-lock hold. The `entries → canonical_to_entries
        // shards` lock order permits taking a shard mutex while `entries`
        // is held (`entries` is outermost), and no path takes a
        // `canonical_to_entries` shard mutex then `entries`, so this
        // nesting is sound. Holding `entries` across the whole drain
        // means a concurrent publish (which registers into
        // `canonical_to_entries` under the same `entries` lock) cannot
        // interleave — so the drain cannot strand a registration whose
        // entry it removed, nor miss an entry whose registration a fresh
        // publish landed mid-drain.
        //
        // `affected_pairs` is collected from the drained set so the
        // post-lock in-flight abort can drop matching in-flight entries
        // even when the `Arc::ptr_eq` check rejects an entry (e.g., a
        // fresh post-publish write replaced the registered dep_signature).
        let mut affected_pairs: FxHashSet<(FamilyKey, ModeSlot)> = FxHashSet::default();
        let mut evicted = 0usize;
        {
            let timing_on = verter_scheduler::request_context::current_timing_enabled();
            let mut entries = self.entries_lock_diagnosed();

            // Drain the per-canonical `(family, slot) →
            // registered_dep_signature` map for `canonical_id`, UNDER the
            // held `entries` lock.
            let drained: Vec<((FamilyKey, ModeSlot), DepSignature)> =
                match self.canonical_to_entries.remove(canonical_id) {
                    Some((_, mutex)) => {
                        let lock_start = if timing_on {
                            Some(Instant::now())
                        } else {
                            None
                        };
                        let mut map = mutex.lock();
                        let lock_wait = lock_start
                            .map(|t| t.elapsed())
                            .unwrap_or(std::time::Duration::ZERO);
                        crate::host_manage::record_family_map_lock_acquisition(lock_wait);
                        map.drain().collect()
                    }
                    None => {
                        // Still account for the canonical-shard removal
                        // itself as one observed acquisition; the DashMap
                        // shard read is implicit in `remove`. When the
                        // entry was absent there is no inner mutex to
                        // time, so the wait is zero.
                        crate::host_manage::record_family_map_lock_acquisition(
                            std::time::Duration::ZERO,
                        );
                        Vec::new()
                    }
                };
            for ((family, slot), _) in &drained {
                affected_pairs.insert((family.clone(), *slot));
            }

            // Walk the drained set. Drop each slot whose current
            // dep_signature `Arc::ptr_eq`-matches the registered
            // dep_signature. ptr_eq distinguishes "our entry" from "a
            // fresh post-publish write that beat us". A fallback dep-sig
            // / fact-rail walk catches any slot that did not ptr_eq (the
            // registered dep_sig was replaced by a fresh build whose
            // dep_sig also references the canonical).
            //
            // Track each evicted entry's `(family, slot)` key together
            // with its full carrier so the cross-canonical drain can
            // remove that exact entry's registrations from every
            // canonical it referenced — both the legacy `DepSignature`
            // rail AND the path-precise `facts` rail (the union returned
            // by `ReadSetSignature::canonical_ids()`).
            //
            // Keying the drain by entry identity `(FamilyKey, ModeSlot)`
            // rather than by `Arc::ptr_eq` on the stored legacy
            // `DepSignature` prevents the "shared legacy Arc" hazard:
            // when two memo entries share the same legacy fence Arc and
            // only one is evicted, a `map.retain` over the other
            // canonical shards with `Arc::ptr_eq(registered, sig)` would
            // remove BOTH entries' registrations because they point to
            // the same Arc. The surviving entry would then have no
            // reverse-index registration for its legacy canonicals and a
            // later `invalidate_canonical` of those canonicals would miss
            // it, leaving stale warm data. Keying the drain by entry
            // identity is what makes that drain precise.
            let mut evicted_entries: Vec<(
                (FamilyKey, ModeSlot),
                crate::fact_signature_helpers::ReadSetSignature,
            )> = Vec::new();
            for ((family, slot), registered_sig) in &drained {
                let Some(slots) = entries.get_mut(family) else {
                    continue;
                };
                let Some(current_entry) = slots.slot(*slot) else {
                    continue;
                };
                let entry_legacy = &current_entry.read_set_signature.legacy;
                let drop = Arc::ptr_eq(entry_legacy, registered_sig)
                    || dep_signature_references_canonical(entry_legacy, canonical_id)
                    || carrier_facts_reference_canonical(
                        &current_entry.read_set_signature.facts,
                        canonical_id,
                    );
                if drop {
                    let entry_carrier = current_entry.read_set_signature.clone();
                    *slots.slot_mut(*slot) = None;
                    evicted += 1;
                    evicted_entries.push(((family.clone(), *slot), entry_carrier));
                }
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

            // For each evicted entry, walk every canonical its carrier
            // referenced (union of `legacy` + path-precise `facts` via
            // `canonical_ids()`) and remove THAT entry's `(family, slot)`
            // registration from the canonical's shard. Removal is by
            // entry identity, not by `Arc::ptr_eq` on the stored legacy
            // signature — see the comment on `evicted_entries` above for
            // the shared-Arc hazard this avoids. Walking the full
            // `canonical_ids()` set (not only `legacy.iter()`) also
            // drains the path-precise fact rail's registrations, matching
            // what `register_reverse_index` populated. Runs UNDER the
            // held `entries` lock (the `entries → canonical_to_entries`
            // order permits it), so the cross-canonical cleanup is atomic
            // with the `entries` eviction above.
            for (evicted_key, evicted_carrier) in &evicted_entries {
                for other_canonical in evicted_carrier.canonical_ids() {
                    if other_canonical.as_ref() == canonical_id {
                        continue;
                    }
                    Self::prune_reverse_index_registration(
                        &self.canonical_to_entries,
                        &other_canonical,
                        evicted_key,
                    );
                }
            }
        }

        // Drop
        // in-flight entries for any (family, slot) whose warm slot
        // was just evicted. Joiners waiting on the condvar observe
        // `aborted = true` on wake and re-enter dispatch from step 1
        // of `execute_cooperative`. The completed sentinel wakes any
        // joiner whose wait predicate only checks `completed`.
        //
        // `affected_pairs` is populated from the Γ.B drained set —
        // even slots that the ptr_eq step rejected (because a fresh
        // post-publish write replaced the registered Arc) are included
        // so any in-flight entry under that pair still aborts correctly.
        //
        // The `affected_pairs.is_empty()` guard short-circuits the
        // whole phase when no canonical-keyed entries existed,
        // avoiding an unnecessary `self.inflight.lock()` acquisition.
        if !affected_pairs.is_empty() {
            let mut table = self.inflight.lock();
            table.retain(|key, inflight| {
                let (family, slot) = family_and_slot(key);
                if !affected_pairs.contains(&(family, slot)) {
                    return true; // keep
                }
                {
                    let mut state = inflight.state.lock();
                    state.aborted = true;
                    if state.completed.is_none() {
                        state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                            "aborted by canonical invalidation",
                        ))));
                        state.dep_signature = Some(empty_signature());
                    }
                }
                inflight.ready.notify_all();
                false // remove
            });
        }

        // Drop
        // NodeArena shard-dedup entries keyed at
        // `File { canonical_id: c, .. }`. Preserves Global entries
        // and entries for any other canonical. The
        // arena Vec is append-only — this only clears the "next
        // intern returns existing id" path; valid SemanticNodeIds for
        // nodes already published into the arena are unaffected.
        self.arena.invalidate_for_canonical(canonical_id);

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
    /// - `named_type_index`, `relation_memo`, `derivation` — the other
    ///   `SemanticNodeId`-keyed semantic caches. Clearing them on a
    ///   project-generation bump drops the stale judgements those caches
    ///   hold.
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
    /// live memo entry with no reverse-index registration. For the
    /// relation memo and the resolved-named-type index the map and its
    /// ledger live in one lock domain too — the wrapper's `clear` holds a
    /// `retention_gate` write guard across both clears, exclusive against
    /// concurrent inserts.
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
    /// (collect-then-release — see the inline comment), so it cannot
    /// AB-BA against [`InflightPanicGuard`]'s `drop`. The relation memo
    /// and resolved-named-type index take their own `retention_gate`
    /// write guard independently of `entries` — no path holds `entries`
    /// across a `retention_gate` acquisition.
    pub fn invalidate_all(&self) -> usize {
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
            // `state` lock: `InflightPanicGuard::drop` acquires `state`
            // then the `inflight` table lock, so a loop holding `table`
            // across the per-entry `state` acquisition would establish
            // the opposite `table → state` order. Instead: snapshot the
            // `Arc<InflightEntry>` handles AND drain the table under the
            // table lock, RELEASE the table lock, THEN lock each entry's
            // `state`. Global rule: `state` is never taken while the
            // `inflight` table lock is held — no AB-BA cycle.
            let aborted_entries: Vec<Arc<InflightEntry>> = {
                let mut table = self.inflight.lock();
                let collected: Vec<Arc<InflightEntry>> = table.values().map(Arc::clone).collect();
                table.clear();
                collected
            };
            // Table lock released — now mark each collected entry
            // `aborted` and wake its waiters with no table lock held.
            for inflight in &aborted_entries {
                {
                    let mut state = inflight.state.lock();
                    state.aborted = true;
                    if state.completed.is_none() {
                        state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                            "aborted by project-generation reset",
                        ))));
                        state.dep_signature = Some(empty_signature());
                    }
                }
                inflight.ready.notify_all();
                // Test-only injection point: parked while iterating the
                // collected entries and locking per-entry `state` — with
                // the `inflight` table lock NOT held. A race test arms it
                // (via `test_invalidate_all_inflight_abort_gate`) and
                // asserts `inflight.try_lock()` succeeds, proving the
                // collect-then-release lock order. `None` (production
                // default) is a no-op.
                #[cfg(any(test, debug_assertions))]
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
            #[cfg(any(test, debug_assertions))]
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
            // canonical_to_entries shards` lock order PERMITS taking a
            // shard mutex while `entries` is held (`entries` is
            // outermost), and no path takes a `canonical_to_entries` shard
            // mutex then `entries`, so this nesting is sound.
            // Test-only injection point: a barrier armed by
            // `test_invalidate_all_pre_reverse_index_clear_gate` parks
            // here, before the reverse-index clear and with the `entries`
            // lock still held, so a race test can assert the clear runs in
            // the `entries` lock domain. `None` (the production default)
            // is a no-op.
            #[cfg(any(test, debug_assertions))]
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
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.invalidate_all_post_entries_clear_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
            }
        }
        // Drop every other `SemanticNodeId`-keyed semantic cache so no
        // stale id-keyed judgement survives the project-generation bump.
        // The relation memo and resolved-named-type index each clear
        // their map and retention budget under their own `retention_gate`
        // write guard — a concurrent insert is excluded across the whole
        // map+budget clear. `clear_and_bump_generation` additionally
        // advances the resolved-named-type reset epoch inside that same
        // `retention_gate.write()` section, so the epoch bump and the map
        // clear are one atomic step — a straggler `insert_resolved_named_type`
        // is fully ordered against it with no window (see that method's
        // docs). The family memo's three-member cluster (`entries`,
        // `memo_budget`, `canonical_to_entries`) was cleared above under
        // the `entries` lock.
        self.named_type_index.clear_and_bump_generation();
        self.relation_memo.clear();
        self.derivation.lock().clear();
        removed
    }

    /// Current Vue macro resolved-named-type reset epoch.
    ///
    /// A macro-resolution build snapshots this value when its
    /// [`HostNamedTypeCacheAdapter`](crate::host_manage::HostNamedTypeCacheAdapter)
    /// is constructed and threads the snapshot into
    /// [`Self::insert_resolved_named_type`] so a stale build's insert is
    /// fenced out. The epoch counter is owned inside
    /// [`BudgetedNamedTypeIndex`] so the bump-under-write-guard /
    /// check-under-read-guard invariant is structural.
    #[must_use]
    pub fn named_type_generation(&self) -> u64 {
        self.named_type_index.generation()
    }

    /// Insert a Vue macro resolution artifact under `key`, fenced by the
    /// resolved-named-type reset epoch.
    ///
    /// `observed_named_type_generation` is the epoch the calling build
    /// snapshotted when its
    /// [`HostNamedTypeCacheAdapter`](crate::host_manage::HostNamedTypeCacheAdapter)
    /// was constructed. The insert is **rejected** (returns `None`,
    /// nothing is recorded in the identity map) when that snapshot no
    /// longer equals the live epoch. On an accepted insert the payload is
    /// interned as a [`SemanticNodeData::VueMacroElements`] node and the
    /// identity mapping is recorded in
    /// [`named_type_index`](Self::named_type_index); reads via
    /// [`Self::get_resolved_named_type`] are then refcount-only.
    ///
    /// ## The fence has no timing window
    ///
    /// A potentially-stale insert comes from a macro-resolution build
    /// aborted by
    /// [`ProjectTypeStore::bump_project_generation_and_evict`](crate::project_type_store::ProjectTypeStore::bump_project_generation_and_evict);
    /// that bump's [`Self::invalidate_all`] advances the reset epoch via
    /// [`BudgetedNamedTypeIndex::clear_and_bump_generation`]. The decisive
    /// epoch check is performed by
    /// [`BudgetedNamedTypeIndex::insert_if_generation_matches`] UNDER the
    /// same `retention_gate.read()` guard that performs the map insert,
    /// while `clear_and_bump_generation` bumps the epoch + clears the map
    /// under `retention_gate.write()`. `RwLock` read/write mutual
    /// exclusion fully orders a straggler against the clear+bump: it runs
    /// wholly before it (inserts; the clear then drops the entry) or
    /// wholly after it (its in-guard epoch read sees the bumped epoch —
    /// rejected). There is no interleaving in which a stale entry
    /// survives — see `insert_if_generation_matches`'s no-window proof.
    /// The snapshot is frozen at adapter construction, so a build aborted
    /// by the bump carries the pre-bump epoch however long it straggles.
    ///
    /// The cheap pre-filter below is an optimization only; the in-guard
    /// check in `insert_if_generation_matches` is the airtight authority.
    pub fn insert_resolved_named_type(
        &self,
        key: HostResolvedNamedTypeKey,
        elements: Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        observed_named_type_generation: u64,
    ) -> Option<SemanticNodeId> {
        // Cheap pre-filter — reject the common straggler without interning
        // a node the in-gate fence would only discard. Optimization only,
        // NOT the airtight authority (this load is unsynchronised against
        // a concurrent `clear_and_bump_generation`).
        if observed_named_type_generation != self.named_type_generation() {
            return None;
        }
        let node_id = self.intern_node(SemanticNodeData::VueMacroElements(elements));
        // Airtight fence: `insert_if_generation_matches` re-reads + compares
        // the epoch UNDER the same `retention_gate.read()` guard that
        // performs the map insert, so a concurrent reset's bump+clear
        // (write guard) is fully ordered against the whole check+insert.
        // On a rejected straggler the interned `node_id` is left
        // unreferenced — harmless: the node arena is append-only.
        if self.named_type_index.insert_if_generation_matches(
            key,
            node_id,
            observed_named_type_generation,
        ) {
            Some(node_id)
        } else {
            None
        }
    }

    /// Fast-path read of a Vue macro resolution artifact. Walks
    /// `key → SemanticNodeId → SemanticNodeData::VueMacroElements(arc) →
    /// arc.clone()`. No dep-signature construction, no cooperative
    /// admission — entries are whole-hash-scoped by construction and
    /// reads are self-validating within one project generation.
    #[must_use]
    pub fn get_resolved_named_type(
        &self,
        key: &HostResolvedNamedTypeKey,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>> {
        let node_id = self.named_type_index.get(key)?;
        match &*self.arena.get(node_id)? {
            SemanticNodeData::VueMacroElements(arc) => Some(Arc::clone(arc)),
            _ => None,
        }
    }

    /// Identity-only lookup: return the [`SemanticNodeId`] associated with
    /// `key` without resolving the payload. Used by
    /// [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
    /// so the formal `execute` entry point can hand back a node id when
    /// the entry is present, without paying for an `Arc::clone` of the
    /// `ResolvedElements` payload on the dispatch hot path.
    #[must_use]
    pub fn resolved_named_type_node_id(
        &self,
        key: &HostResolvedNamedTypeKey,
    ) -> Option<SemanticNodeId> {
        self.named_type_index.get(key)
    }

    /// Drop every entry in the Vue macro resolution identity map. Invoked
    /// on project-generation bumps / per-canonical evictions — the
    /// append-only node arena keeps the interned
    /// [`SemanticNodeData::VueMacroElements`] payloads alive only as long
    /// as something else references their ids, which is fine because the
    /// identity map was the only external reachability path to them.
    ///
    /// The map and its retention budget are cleared in one lock domain
    /// under the `BudgetedNamedTypeIndex`'s `retention_gate` write
    /// guard, exclusive against concurrent inserts.
    pub fn clear_resolved_named_types(&self) {
        self.named_type_index.clear();
    }

    /// Remove every entry in the Vue macro resolution identity map whose
    /// key's `canonical_id` matches `canonical_id`. Called from
    /// [`ProjectTypeStore::evict_canonical`](crate::project_type_store::ProjectTypeStore::evict_canonical)
    /// so stale artifacts do not keep a retired file's spans alive.
    /// Returns the number of entries evicted. The map retention + the
    /// per-entry budget removal run under the `BudgetedNamedTypeIndex`'s
    /// `retention_gate` read guard; each removal is scoped to the dropped
    /// entry's own `admission_seq` (`forget_seq`), so a concurrent
    /// `insert` re-admitting the same key keeps its fresh admission.
    pub fn invalidate_resolved_named_types_for_canonical(&self, canonical_id: &str) -> usize {
        self.named_type_index.retain_for_canonical(canonical_id)
    }

    /// Number of Vue macro resolution entries. Useful for tests and
    /// debug/telemetry counters.
    #[must_use]
    pub fn resolved_named_type_count(&self) -> usize {
        self.named_type_index.len()
    }

    // ──────────────────────────────────────────────────────────────────
    // Relation memo
    // ──────────────────────────────────────────────────────────────────

    /// Strict warm-hit read of a cached relation judgement for
    /// `(source, target)`.
    ///
    /// Returns the tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) plus the
    /// recorded legacy `DepSignature` **only when** the stored entry's
    /// self-version-rooted carrier validates against the live store view
    /// — every self-root canonical's `FileWholeHash` is validated
    /// strictly. A stale entry (a same-canonical content edit to the
    /// source's or the target's originating file, or a self-root the
    /// live store view no longer tracks) returns `None`, so the caller
    /// recomputes the relation judgement instead of serving it stale.
    /// Validation failure does NOT bubble the carrier — a stale entry
    /// must not pollute an outer tracer.
    #[must_use]
    pub(crate) fn get_relation(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(DepSignature, crate::semantic_query::RelationResult)> {
        // Clone the entry OUT of the `DashMap` shard guard before
        // validating: `validate_with_self_roots` / `bubble` consult the
        // resolver store view and fan into TLS tracers, which may
        // re-enter the relation memo — holding the shard guard across
        // that re-entry would deadlock. `BudgetedRelationMemo::get_cloned`
        // performs exactly that clone-out-of-guard.
        let entry = self.relation_memo.get_cloned(source, target)?;
        if !entry
            .carrier
            .validate_with_self_roots(ctx, &entry.self_root_canonicals)
        {
            return None;
        }
        entry.carrier.bubble(ctx);
        Some((Arc::clone(&entry.carrier.legacy), entry.result))
    }

    /// Publish a relation judgement for `(source, target)`. Writes to the
    /// dedicated relation memo DashMap, separate from the family memo so
    /// pairwise identity does not inflate the single-node keyspace.
    ///
    /// The entry is self-version-rooted: `carrier` is built by
    /// [`semantic_graph_read_set_signature`] from the relation build's
    /// observed self-roots (the file-derived origins of `source` and
    /// `target`), so a content edit to either originating file misses
    /// the warm relation read. `self_root_canonicals` is the strict
    /// self-root canonical set the warm validator checks.
    pub fn insert_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        result: crate::semantic_query::RelationResult,
    ) {
        // The map insert + retention-budget admission run under the
        // `BudgetedRelationMemo`'s `retention_gate` read guard, and
        // new-key detection is atomic (via the `DashMap::entry` API) —
        // a `contains_key`-then-`insert` pair would let two writers
        // racing the same key both observe "absent" and double-record
        // the admission. `insert` stamps the entry's retention-ledger
        // `admission_seq` so it stays paired with its FIFO ledger
        // record.
        self.relation_memo
            .insert(source, target, carrier, self_root_canonicals, result);
    }

    /// Count of relation memo entries. Useful for tests and counters.
    #[must_use]
    pub fn relation_memo_count(&self) -> usize {
        self.relation_memo.len()
    }

    /// Drop every entry in the relation memo. Invoked on
    /// project-generation bumps so warm relation judgements cannot leak
    /// across a version boundary. The map and its retention budget are
    /// cleared in one lock domain under the `BudgetedRelationMemo`'s
    /// `retention_gate` write guard, exclusive against concurrent
    /// inserts.
    pub fn clear_relation_memo(&self) {
        self.relation_memo.clear();
    }

    // ──────────────────────────────────────────────────────────────────
    // Derivation / origin layer
    // ──────────────────────────────────────────────────────────────────

    /// Record a derivation/origin edge for `result`. Builders call this
    /// whenever they produce a reusable result — the edge captures the
    /// source-set, per-edge metadata, and a snapshot of the publishing
    /// builder's active fence (`builder_fence`). The fence snapshot is
    /// interned in the store's signature pool so identical fences share
    /// one allocation.
    ///
    /// Origin edges are bounded best-effort provenance for the audit
    /// origin-graph trace, NOT an invalidation source — the stored fence
    /// snapshot is never reconstructed into a `CompletionFence`. See the
    /// `derivation` module docs.
    ///
    /// Multiple derivations of the same structural `result` produce
    /// multiple edges with the same `(result, kind)` — the layer supports
    /// this; the walker walks all edges.
    ///
    /// Edges are deduplicated by identity at the call site: before
    /// recording into [`DerivationStore::edges`], an edge with the exact
    /// same `(result, kind, sources, meta, fence)` identity tuple already
    /// present is skipped, so repeated walks through the same
    /// intermediate hop do not inflate the bucket or the per-request
    /// audit cost. The audit-mining contract is preserved: the
    /// [`request_context::current_accumulator`] push remains
    /// unconditional so the footprint miner observes every derivation hop
    /// the production hot path would have emitted.
    pub fn record_origin_edge(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        sources: Arc<[SemanticNodeId]>,
        meta: crate::semantic_query::OriginMeta,
        builder_fence: DepSignature,
    ) {
        // Diagnosis instrumentation: bracket the entire
        // `record_origin_edge` call with `Instant::now()` deltas so the
        // capture token can attribute per-call wall-clock cost. The
        // timing measurement itself is two RDTSC reads (Linux) /
        // QueryPerformanceCounter (Windows) — no allocation, no lock —
        // so it does not perturb the production hot path beyond the
        // `with_active_capture` thread-local lookup that is already
        // present below. The deltas are only consumed when a token is
        // bound; the producer always pays the two timestamp reads, but
        // they are constant-time and on the critical path of every
        // origin-edge emission anyway (`stats.origin_edges_emitted` is
        // already atomically bumped). The diagnosis benchmark is the
        // only consumer; production-path behaviour is unchanged when no
        // token is bound.
        let start = Instant::now();
        // Build the edge under the derivation lock, then release the
        // lock before pushing into the accumulator — the accumulator
        // acquires its own mutex and we must not hold the graph lock
        // across that boundary.
        //
        // The edge identity tuple is checked
        // under the derivation lock for an existing match. When found,
        // the ledger write is skipped (no `store.record` call) and the
        // `already_recorded` flag flows through the rest of the
        // function so the capture-token edge ledger and the
        // `origin_edges_emitted` stats counter mirror the dedup. The
        // audit-accumulator push and the `record_origin_edge_total_ns`
        // wall-clock attribution are intentionally NOT gated by this
        // flag — see the audit-mining contract preservation note above.
        let (edge, already_recorded) = {
            let mut store = self.derivation.lock();
            let edge_dep_signature = store.intern_signature(builder_fence);
            let edge = OriginEdge {
                sources,
                meta,
                edge_dep_signature,
            };
            // Identity check: scan the existing `(result, kind)` bucket
            // for an entry that matches this edge's full identity tuple
            // (sources content, meta value, and interned dep_signature
            // pointer). The interner guarantees identical signatures
            // share a single Arc, so `Arc::ptr_eq` is a sound identity
            // probe; `OriginMeta` derives `PartialEq` and `sources` is
            // a content-comparable slice.
            let already_recorded = store
                .bucket_for(result, kind)
                .map(|existing| {
                    existing.iter().any(|e| {
                        Arc::ptr_eq(&e.edge_dep_signature, &edge.edge_dep_signature)
                            && e.sources.as_ref() == edge.sources.as_ref()
                            && e.meta == edge.meta
                    })
                })
                .unwrap_or(false);
            if !already_recorded {
                store.record(result, kind, edge.clone());
            }
            (edge, already_recorded)
        };
        if !already_recorded {
            self.stats
                .origin_edges_emitted
                .fetch_add(1, Ordering::Relaxed);
        }
        // Feed the accumulator of the active audited
        // request so the footprint miner sees every derivation hop.
        // No-op when no request context is installed.
        //
        // Audit-mining contract preservation: this push is
        // intentionally unconditional — it runs even on the dedup path
        // so dropped ledger writes still surface in the audit trace.
        if let Some(acc) = crate::request_context::current_accumulator() {
            acc.push_derivation_edge(result, kind, edge.clone());
        }
        // Test harness hook: when a CaptureToken is bound on the current
        // thread, record the edge identity tuple in the per-request
        // ledger so duplicate-derivation tests can read snapshots. The
        // closure runs OUTSIDE the derivation lock (released above).
        // The `with_active_capture` call returns immediately when no
        // token is bound (the production hot path) — no lock, no
        // allocation, one thread-local lookup.
        //
        // Skip the capture-token edge ledger insert + the
        // `origin_edge_count` bump on the dedup path. The ledger / count
        // mirror the production-side ledger writes so test snapshots
        // observe the same dedup property.
        let elapsed_ns = start.elapsed().as_nanos();
        crate::capture_token::with_active_capture(|t| {
            if !already_recorded {
                let dep_signature_hash =
                    crate::capture_token::stable_hash_slice(&edge.edge_dep_signature);
                let identity = crate::capture_token::EdgeIdentity::from_record(
                    result,
                    kind,
                    edge.sources.as_ref(),
                    &edge.meta,
                    dep_signature_hash,
                );
                t.record_edge(identity);
                // Bump the per-call counter +
                // wall-clock cost only on actual ledger emissions. The
                // dedup-skipped path bypasses both so `origin_edge_count`
                // mirrors the ledger-write count and
                // `record_origin_edge_total_ns` reflects the cold-path
                // wall-clock the §4.3B benchmark gate evaluates against
                // the post-B2 baseline.
                t.record_origin_edge_call(elapsed_ns);
            }
        });
    }

    /// Read-only origin walk for a result node — yields every edge
    /// reachable from `node`, regardless of kind.
    ///
    /// Test-only enumeration accessor. Production origin consumption
    /// goes through [`Self::walk_origin_chain`] (the audit origin-graph
    /// builder); this whole-vector form exists for the derivation-layer
    /// tests. Origin edges are bounded best-effort provenance — see the
    /// `derivation` module docs.
    #[cfg(test)]
    #[must_use]
    pub fn origins(&self, node: SemanticNodeId) -> Vec<(OriginEdgeKind, OriginEdge)> {
        let store = self.derivation.lock();
        store.origins(node).map(|(k, e)| (k, e.clone())).collect()
    }

    /// Filtered read-only origin walk: only edges of the given kind.
    ///
    /// Test-only enumeration accessor — see [`Self::origins`].
    #[cfg(test)]
    #[must_use]
    pub fn origins_of_kind(&self, node: SemanticNodeId, kind: OriginEdgeKind) -> Vec<OriginEdge> {
        let store = self.derivation.lock();
        store.origins_of_kind(node, kind).cloned().collect()
    }

    /// Convenience helper: invoke `visitor` for every origin edge on
    /// `node`. The derivation lock is released before any visitor
    /// callback fires so visitors that recursively walk the chain
    /// (e.g. transitively via `origins_of_kind`) cannot deadlock against
    /// the same lock.
    pub fn walk_origin_chain<F>(&self, node: SemanticNodeId, mut visitor: F)
    where
        F: FnMut(OriginEdgeKind, &OriginEdge),
    {
        let edges = {
            let store = self.derivation.lock();
            store
                .origins(node)
                .map(|(kind, edge)| (kind, edge.clone()))
                .collect::<Vec<_>>()
        };
        for (kind, edge) in &edges {
            visitor(*kind, edge);
        }
    }

    /// Total origin edges across all result nodes. Mirrors the public
    /// [`SemanticGraphStats::origin_edge_count`].
    #[must_use]
    pub fn origin_edge_count(&self) -> usize {
        self.derivation.lock().edge_count()
    }

    /// Number of distinct `(result, kind)` derivation edge buckets
    /// currently retained. The derivation store bounds this with a FIFO
    /// retention budget; the bounded-retention proof asserts the count
    /// stays capped across many content edits.
    #[must_use]
    pub fn derivation_bucket_count(&self) -> usize {
        self.derivation.lock().bucket_count()
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn export_all_origin_edges(&self) -> Vec<(SemanticNodeId, OriginEdgeKind, OriginEdge)> {
        self.derivation.lock().all_edges()
    }

    // ──────────────────────────────────────────────────────────────────
    // Telemetry — public stats snapshot
    // ──────────────────────────────────────────────────────────────────

    /// Read an immutable snapshot of every counter the store maintains.
    /// Safe to call mid-request; counters are atomic and percentile
    /// computation locks-and-clones the sample reservoir so no torn
    /// reads.
    #[must_use]
    pub fn stats_snapshot(&self) -> SemanticGraphStats {
        let derivation = self.derivation.lock();
        let origin_edge_count = derivation.edge_count() as u64;
        // origin_edges_per_node percentiles are derived from the
        // derivation store directly (no separate sample reservoir
        // needed — the store already records the full edge layout).
        let mut by_node: FxHashMap<SemanticNodeId, u32> = FxHashMap::default();
        for (node, _kind, edges) in derivation.iter_edges() {
            let cell = by_node.entry(*node).or_insert(0);
            *cell = cell.saturating_add(edges.len() as u32);
        }
        drop(derivation);
        let mut per_node_counts: Vec<u32> = by_node.into_values().collect();
        per_node_counts.sort_unstable();
        let origin_edges_per_node_p50 = sorted_percentile(&per_node_counts, 0.5);
        let origin_edges_per_node_p95 = sorted_percentile(&per_node_counts, 0.95);

        let path_samples = self.stats.path_length_samples.lock();
        let path_length_p50 = path_samples.percentile(0.5);
        let path_length_p95 = path_samples.percentile(0.95);
        drop(path_samples);
        let proj_samples = self.stats.projection_depth_samples.lock();
        let projection_depth_p50 = proj_samples.percentile(0.5);
        let projection_depth_p95 = proj_samples.percentile(0.95);
        drop(proj_samples);

        SemanticGraphStats {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            same_path_sentinel_returns: self
                .stats
                .same_path_sentinel_returns
                .load(Ordering::Relaxed),
            in_flight_peak: self.stats.in_flight_peak.load(Ordering::Relaxed),
            waits_ms: self.stats.waits_ms.load(Ordering::Relaxed),
            memo_entry_count: self.memo_entry_count() as u64,
            joined_waits: self.stats.joined_waits.load(Ordering::Relaxed),
            inflight_aborted_retries: self.stats.inflight_aborted_retries.load(Ordering::Relaxed),
            cold_aborts_swept: self.stats.cold_aborts_swept.load(Ordering::Relaxed),
            origin_edge_count,
            origin_edges_emitted: self.stats.origin_edges_emitted.load(Ordering::Relaxed),
            origin_edges_per_node_p50,
            origin_edges_per_node_p95,
            instantiate_count: self.stats.instantiate_count.load(Ordering::Relaxed),
            conditional_decided_count: self.stats.conditional_decided_count.load(Ordering::Relaxed),
            conditional_deferred_count: self
                .stats
                .conditional_deferred_count
                .load(Ordering::Relaxed),
            branch_selections_true: self.stats.branch_selections_true.load(Ordering::Relaxed),
            branch_selections_false: self.stats.branch_selections_false.load(Ordering::Relaxed),
            budget_fallback_count: self.stats.budget_fallback_count.load(Ordering::Relaxed),
            path_length_p50,
            path_length_p95,
            projection_depth_p50,
            projection_depth_p95,
            decl_subexpression_lowering_count: self
                .stats
                .decl_subexpression_lowering_count
                .load(Ordering::Relaxed),
            relation_check_count: self.stats.relation_check_count.load(Ordering::Relaxed),
        }
    }

    /// Builder-side counter helpers. Builders increment these as they emit
    /// reusable work; the per-builder semantics are documented in plan
    /// §3 Phase C (where the real builders land).
    pub fn record_instantiate(&self) {
        self.stats.instantiate_count.fetch_add(1, Ordering::Relaxed);
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
    pub fn record_budget_fallback(&self) {
        self.stats
            .budget_fallback_count
            .fetch_add(1, Ordering::Relaxed);
    }
    /// Record one path-length sample into the bounded reservoir.
    /// Builders call this once per `ProjectPath` invocation in C-phase.
    pub fn record_path_length(&self, length: u32) {
        self.stats.path_length_samples.lock().push(length);
    }
    /// Record one projection-depth sample into the bounded reservoir.
    /// Builders call this once per recursive projection descent in
    /// C-phase.
    pub fn record_projection_depth(&self, depth: u32) {
        self.stats.projection_depth_samples.lock().push(depth);
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
    /// (`tests/semantic_graph_production_reads_validated.rs`) enforces
    /// that no seal-scope production file calls `get_unvalidated`.
    ///
    /// The name carries the contract: `get_unvalidated` returns an entry
    /// WITHOUT validating its carrier, so the unvalidated nature is
    /// explicit at every call site.
    #[must_use]
    pub fn get_unvalidated(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        let result = entries.get(&family).and_then(|slots| {
            slots.slot(slot).cloned().map(|entry| {
                // R3/R26/R28 - bubble the entry path-precise fact
                // observation set into any outer cold-compute scope
                // so transitive memo hits do not lose contributing
                // fact identities. AND-gate validation happens at
                // the outer fence revalidation point.
                entry.read_set_signature.bubble_via_tls();
                let dep_signature = Arc::clone(&entry.read_set_signature.legacy);
                CacheRead {
                    value: entry.result,
                    dep_signature,
                    walker_diagnostics: entry.walker_diagnostics,
                    cache_suppress: false,
                }
            })
        });
        if let Some(rctx) = crate::request_context::current_request_context() {
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
        }
        result
    }

    /// Carrier-aware warm read that validates BEFORE bubbling. Returns
    /// the cached value only when the entry's
    /// [`ReadSetSignature`](crate::fact_signature_helpers::ReadSetSignature)
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
    pub(crate) fn get_validated(
        &self,
        key: &SemanticQueryKey,
        ctx: &dyn crate::resolver_core::ResolverContext,
    ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        let result = entries.get(&family).and_then(|slots| {
            slots.slot(slot).cloned().and_then(|entry| {
                // Strict warm-read validation: validate the carrier
                // against the live store view, validating every
                // self-root canonical's `FileWholeHash` strictly. A
                // stale entry — a same-canonical content edit, or a
                // self-root the live store view no longer tracks —
                // fails and is NOT bubbled (a stale entry must not
                // pollute an outer tracer).
                if !entry.validate(ctx) {
                    return None;
                }
                entry.read_set_signature.bubble(ctx);
                let dep_signature = Arc::clone(&entry.read_set_signature.legacy);
                Some(CacheRead {
                    value: entry.result,
                    dep_signature,
                    walker_diagnostics: entry.walker_diagnostics,
                    cache_suppress: false,
                })
            })
        });
        if let Some(rctx) = crate::request_context::current_request_context() {
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
        }
        result
    }

    /// Presence probe — true iff the warm map currently holds a
    /// `(family, slot)` entry for `key`. Does NOT validate, does NOT
    /// bubble, does NOT bump any counter. Used by the prefix-backfill
    /// helper in `warm_publish_one_if_absent` to decide whether a
    /// publish would race a concurrent cold winner.
    #[must_use]
    pub(crate) fn contains_key(&self, key: &SemanticQueryKey) -> bool {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .is_some_and(|slots| slots.slot(slot).is_some())
    }

    /// Per-key result for the BFS bridge's batch dispatch (D103). Each
    /// frontier handle is resolved into either a node-id (success) or a
    /// typed reason describing why expansion could not proceed. Per-key
    /// errors are returned, NOT panic'd (D41 invariant: one batch entry → N
    /// keys → K admissions).
    ///
    /// Lookups happen via the validated warm read `get_validated(key,
    /// ctx)` only — `execute_cooperative_batch` is a non-admission
    /// probe; a stale (or absent) entry surfaces as
    /// [`BatchExpandError::EvictedNode`] so the BFS bridge re-issues a
    /// per-key cooperative cold build. Cold builds stay the
    /// responsibility of the per-query cooperative path.
    ///
    /// `#[cfg(test)]`: the non-admission batch probe has no production
    /// warm-read caller — the per-query cooperative path is the sole
    /// production warm-read entry point. The probe is retained for the
    /// substrate test suite that characterises its non-admission
    /// contract.
    #[cfg(test)]
    pub(crate) fn execute_cooperative_batch(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        keys: &[crate::semantic_query::SemanticQueryKey],
    ) -> Vec<Result<SemanticNodeId, BatchExpandError>> {
        keys.iter()
            .map(|key| {
                if let Some(hit) = self.get_validated(key, ctx) {
                    match hit.value {
                        QueryResult::Value(node) => Ok(node),
                        QueryResult::Recursive(node) => Ok(node),
                        QueryResult::Error(_) => Err(BatchExpandError::EvictedNode),
                    }
                } else {
                    // Cold or stale: from the BFS bridge's perspective,
                    // an unmaterialized OR stale key is treated as
                    // evicted; the bridge will surface a typed
                    // StaleAtFrontier envelope and the caller can decide
                    // whether to issue a per-key cooperative cold build.
                    Err(BatchExpandError::EvictedNode)
                }
            })
            .collect()
    }

    /// Cooperative execution entry point. Semantics:
    ///
    /// 1. If the key is already warm, return the cached result and signature.
    /// 2. If the current thread is already building this exact key further
    ///    up its own stack, return
    ///    [`QueryResult::Recursive(sentinel)`](QueryResult::Recursive) —
    ///    **never self-await.**
    /// 3. If another thread is building the key, block cooperatively on the
    ///    per-entry condvar until it publishes.
    /// 4. Otherwise claim ownership, invoke `build`, publish the result,
    ///    and wake joiners.
    ///
    /// **Joiner retry on canonical invalidation (B3).** When a joiner
    /// wakes from the condvar and observes `state.aborted = true` (set by
    /// [`Self::invalidate_canonical`] when the (family, slot) was swept),
    /// it re-enters dispatch from step 1 up to [`MAX_INFLIGHT_RETRIES`]
    /// times. After exhausting the retry budget the joiner returns the
    /// sentinel so its caller fails fast rather than spinning.
    ///
    /// `recursion_sentinel` produces a fallback [`SemanticNodeId`] when
    /// same-path recursion is detected.
    ///
    /// ## Warm-hit fast path
    ///
    /// Step 1 is implemented as a non-allocating fast path
    /// ([`Self::try_warm_hit_fast_path`]): a single non-diagnosed
    /// `entries.lock()` + slot read. On hit the fast path returns
    /// immediately, bumping a single per-request hit counter, the
    /// `WARM_HIT_FAST_PATH_HITS` instrumentation counter, and the
    /// production capture-token / test-only dispatch recorders. The
    /// fast path bypasses the cooperative-admission flow (no
    /// in-flight table touch, no `entries_lock_diagnosed` timing
    /// wrappers, no second `self.get(&key)` invocation). On a miss
    /// the fast path drops the entries lock and falls through to the
    /// cooperative slow path.
    ///
    /// ## Soundness
    ///
    /// The fast path returns a clone of the cached `MemoEntry`'s
    /// `(result, dep_signature)`. The dep signature flows back into the
    /// caller's dependency-fact set exactly as the slow path's warm-hit
    /// branch does, so warm-cache reuse stays bounded by dep-signature
    /// validation at the outer caller's publish-side
    /// completion-fence revalidation.
    /// Same-path recursion detection is unaffected: the cold winner
    /// publishes the warm slot AFTER the build closure returns, so a
    /// populated slot cannot represent a cycle currently being built
    /// — the only path that needs same-path recursion detection is
    /// the slow path's loop, which still runs on cache miss. Joiner
    /// waits and abort-driven retries are unaffected: a populated
    /// warm slot means no joiner participation is needed.
    #[must_use = "the CacheRead carries both the resolved node id and the dep signature callers must fold into their dependency-fact set for the publish-side completion-fence revalidation"]
    pub(crate) fn execute_cooperative<F, R, O>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> CacheRead<QueryResult<SemanticNodeId>>
    where
        F: FnOnce() -> O,
        O: Into<crate::project_semantic_dispatch::walk::QueryBuildOutput>,
        R: FnOnce() -> SemanticNodeId,
    {
        // Loop-5 instrumentation — count every logical entry. Logged
        // unconditionally so call counts include both fast-path and
        // slow-path entries.
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Warm-hit fast path. A single non-diagnosed `entries.lock()`
        // acquisition checks the slot; on a hit the entry's carrier is
        // validated strictly (self-roots through
        // `validates_self_root_whole_hash`) BEFORE the carrier is
        // bubbled or the value returned — a stale entry (a
        // same-canonical content edit, an untracked self-root) misses
        // and the slow path cold-recomputes. On hit it returns
        // immediately bypassing the slow path's `entries_lock_diagnosed`
        // `Instant::now`/capture-token wait+hold timing, the in-flight
        // table mutex, the second warm probe inside the loop's step 1,
        // the same-path recursion test, and the joiner-condvar
        // admission entry path. On miss the lock is released and
        // execution falls through to the cooperative slow path that
        // owns same-path recursion, in-flight admission, and cold-build
        // publish.
        if let Some(hit) = self.try_warm_hit_fast_path(ctx, &key) {
            return hit;
        }

        // Slow path — cooperative-admission flow. Handles same-path
        // recursion, joiner-condvar waits, cold-build publish.
        self.execute_cooperative_slow(ctx, key, recursion_sentinel, build)
    }

    /// Warm-hit fast path for [`Self::execute_cooperative`]. Returns
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
    /// Holds the lock ONLY for the slot read + `MemoEntry` clone, then
    /// releases it. Carrier validation (`entry.validate` — builds a
    /// resolver store view, walks the legacy dep-signature rail), the
    /// TLS fact-rail bubble, and instrumentation all run AFTER the
    /// lock is dropped, so an unrelated warm read or cold publish does
    /// not serialise on the single global memo mutex for the duration
    /// of validation. Mirrors the relation memo's `get_relation`.
    #[inline]
    fn try_warm_hit_fast_path(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &SemanticQueryKey,
    ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);

        // Clone the `MemoEntry` OUT of the `entries` lock before
        // validating. `entry.validate(ctx)` builds a resolver store
        // view and walks the legacy dep-signature rail; holding the
        // single global `entries` mutex across that work would
        // serialise every unrelated warm read and cold publish on the
        // memo mutex for the duration of validation. The clone is a
        // handful of `Arc::clone`s (`ReadSetSignature` rails,
        // `self_root_canonicals`, `walker_diagnostics`) — far cheaper
        // than holding the lock through validation. This mirrors the
        // relation memo's `get_relation`, which clones the
        // `RelationMemoEntry` out of its `DashMap` shard guard before
        // validating + bubbling.
        let entry: MemoEntry = {
            // Single non-diagnosed lock acquisition. The
            // `entries_lock_diagnosed` wrapper that adds Instant::now
            // wait+hold timing under capture-token is intentionally
            // bypassed here because the warm-hit hot path runs
            // hundreds of thousands of times per request and the
            // wrapper's per-acquisition cost dominates the warm-hit
            // wall-clock.
            let entries = self.entries.lock();
            entries
                .get(&family)
                .and_then(|slots| slots.slot(slot).cloned())?
        };

        // Validate + bubble OUTSIDE the critical section.
        //
        // Strict warm-read validation BEFORE bubbling: the entry's
        // carrier is validated against the live store view, validating
        // every self-root canonical's `FileWholeHash` strictly. A
        // stale entry — a same-canonical content edit, or a self-root
        // the live store view no longer tracks — fails and the fast
        // path reports a miss WITHOUT bubbling: a stale entry must not
        // pollute an outer tracer with observations that no longer
        // reflect current state, and the cooperative slow path
        // cold-recomputes.
        if !entry.validate(ctx) {
            return None;
        }
        // R3/R26/R28 - bubble the entry path-precise fact observation
        // set into any outer cold-compute scope so transitive memo
        // hits do not lose the contributing fact identities.
        entry.read_set_signature.bubble_via_tls();
        let hit = CacheRead {
            value: entry.result,
            dep_signature: Arc::clone(&entry.read_set_signature.legacy),
            walker_diagnostics: entry.walker_diagnostics,
            cache_suppress: false,
        };

        // Instrumentation — fast-path attribution.
        crate::loop5_instrumentation::WARM_HIT_FAST_PATH_HITS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_WARM_HITS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::loop5_instrumentation::FAMILY_MEMO_HITS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Production stats (lock-free atomic — same as slow path's
        // warm-hit branch at step 1).
        self.stats.hits.fetch_add(1, Ordering::Relaxed);

        // Per-request audit hit attribution. Bumped exactly once per
        // warm call (the slow path's old `let initial_hit = self.get_unvalidated(&key)`
        // observation plus the loop's step-1 `self.get_unvalidated(&key)` call
        // bumped this counter twice; the fast path bumps it once).
        if let Some(rctx) = crate::request_context::current_request_context() {
            rctx.cache_counters
                .semantic_graph
                .hits
                .fetch_add(1, Ordering::Relaxed);
        }

        // Per-context cache-event attribution (Hit). Same as the slow
        // path's step-1 warm branch, single TLS lookup.
        if let Some(ctx) = verter_scheduler::request_context::current_context() {
            ctx.0
                .record_cache_event(verter_scheduler::request_context::CacheEventKind::Hit);
        }

        // cfg-test dispatch recording (warm). Same as the slow path's
        // pre-loop `initial_hit` observation.
        #[cfg(test)]
        crate::project_semantic_dispatch::raise::record_dispatch_warm(key);

        // Production capture-token dispatch recording (warm). Same as
        // the slow path's pre-loop observation.
        crate::capture_token::with_active_capture(|t| t.record_dispatch(key, /* hit */ true));

        tracing::debug!(
            target: "verter::memo::hit",
            ?key,
            "memo_hit"
        );

        Some(hit)
    }

    /// Cooperative-admission slow path for
    /// [`Self::execute_cooperative`]. Reached only when
    /// [`Self::try_warm_hit_fast_path`] reported a miss. Owns
    /// same-path recursion detection, in-flight admission, joiner
    /// waits, abort-driven retries, cold-build execution, and warm
    /// publish.
    ///
    /// All instrumentation atomics + capture-token + cfg-test
    /// dispatch recording for the COLD/MISS / JOINER paths live here.
    /// The warm-hit branch is intentionally absent — the fast path
    /// handles every warm hit before this function is called.
    fn execute_cooperative_slow<F, R, O>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> CacheRead<QueryResult<SemanticNodeId>>
    where
        F: FnOnce() -> O,
        O: Into<crate::project_semantic_dispatch::walk::QueryBuildOutput>,
        R: FnOnce() -> SemanticNodeId,
    {
        let mut miss_recorded = false;
        let mut retries = 0usize;

        // Cold/miss instrumentation — count one logical miss per
        // call. The fast path already filtered every warm hit, so
        // any entry to this function is by definition a miss
        // (modulo a rare race where another thread publishes
        // between our fast-path check and the loop's step 1
        // re-check; that race is benign — the loop returns the
        // freshly-published value through its step-1 warm branch,
        // and we credit it as a hit there).
        crate::loop5_instrumentation::FAMILY_MEMO_MISSES
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // cfg-test dispatch recording (cold). Same as the
        // pre-refactor pre-loop observation.
        #[cfg(test)]
        crate::project_semantic_dispatch::raise::record_dispatch_cold(&key);

        // Production capture-token dispatch recording (cold).
        crate::capture_token::with_active_capture(|t| {
            t.record_dispatch(&key, /* hit */ false)
        });

        tracing::debug!(
            target: "verter::memo::miss",
            ?key,
            "memo_miss"
        );

        let (inflight, key) = loop {
            // 1. Warm memo hit. Reaches here only on the rare race
            //    where another thread published between our fast-path
            //    check and now (or on retry after an abort sweep). The
            //    warm read is validated strictly via `get_validated` —
            //    a freshly-published entry validates; a slot a
            //    concurrent invalidation made stale misses and the
            //    cold-build path below recomputes.
            if let Some(hit) = self.get_validated(&key, ctx) {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                if let Some(sched_ctx) = verter_scheduler::request_context::current_context() {
                    sched_ctx
                        .0
                        .record_cache_event(verter_scheduler::request_context::CacheEventKind::Hit);
                }
                return hit;
            }
            if !miss_recorded {
                // Count one miss per logical call, regardless of how many
                // retries step 3 performs.
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                if let Some(ctx) = verter_scheduler::request_context::current_context() {
                    ctx.0.record_cache_event(
                        verter_scheduler::request_context::CacheEventKind::Miss,
                    );
                }
                miss_recorded = true;
            }

            // 2. Same-path recursion detection — bail with a sentinel.
            let is_self_recursive =
                IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow().iter().any(|k| k == &key));
            if is_self_recursive {
                self.stats
                    .same_path_sentinel_returns
                    .fetch_add(1, Ordering::Relaxed);
                if let Some(ctx) = verter_scheduler::request_context::current_context() {
                    ctx.0.record_cache_event(
                        verter_scheduler::request_context::CacheEventKind::Sentinel,
                    );
                }
                return CacheRead {
                    value: QueryResult::Recursive(recursion_sentinel()),
                    dep_signature: empty_signature(),
                    walker_diagnostics: std::sync::Arc::from([]),
                    cache_suppress: false,
                };
            }

            // 3. Register or join the in-flight entry.
            let inflight = {
                let mut table = self.inflight.lock();
                table
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(InflightEntry::new()))
                    .clone()
            };

            // Claim ownership or wait for the winner to publish.
            let mut state = inflight.state.lock();
            if state.claimed {
                // Cooperative wait — block on the per-entry condvar until
                // `completed` is set OR the entry is aborted by a
                // canonical-invalidation sweep. Joiners never busy-spin.
                // Account wait time on the stats surface so the F3 corpus
                // benchmark surfaces non-zero `waits_ms`.
                let wait_start = Instant::now();
                inflight
                    .ready
                    .wait_while(&mut state, |s| s.completed.is_none() && !s.aborted);
                self.stats
                    .waits_ms
                    .fetch_add(wait_start.elapsed().as_millis() as u64, Ordering::Relaxed);
                // Count every cooperative wait return (`joined_waits`).
                // Retries after abort re-enter dispatch and may bump
                // this again on the next join.
                self.stats.joined_waits.fetch_add(1, Ordering::Relaxed);
                if let Some(ctx) = verter_scheduler::request_context::current_context() {
                    ctx.0.record_cache_event(
                        verter_scheduler::request_context::CacheEventKind::JoinedWait,
                    );
                }
                if state.aborted && retries < MAX_INFLIGHT_RETRIES {
                    // The (family, slot) this entry was serving was swept
                    // by a concurrent canonical invalidation. Retry the
                    // whole dispatch flow from step 1 — the warm slot is
                    // either already repopulated by another winner or
                    // still empty, in which case this caller may become
                    // the fresh cold winner.
                    retries += 1;
                    record_inflight_aborted_retry(&self.stats);
                    drop(state);
                    drop(inflight);
                    continue;
                }
                let result = state.completed.clone().unwrap_or_else(|| {
                    QueryResult::Error(QueryError::Other(Arc::from(
                        "joiner woke without completion after retry budget exhausted",
                    )))
                });
                let dep_signature = state.dep_signature.clone().unwrap_or_else(empty_signature);
                let graph_carrier = state.graph_carrier.clone();
                let winner_self_roots = Arc::clone(&state.self_root_canonicals);
                // Inherit the winner build's non-cacheability flag. A
                // `cache_suppress` winner is non-cacheable (tracer
                // overflow, pathological input, or an unrootable `None`
                // signature); the joiner MUST return the SAME
                // `cache_suppress` so a joiner inside an outer cold
                // query cannot — through a composition helper threading
                // this read — publish an outer entry that inherits
                // neither the suppressed child's suppression nor (via
                // the carrier bubble below) its dep facts.
                let cache_suppress = state.cache_suppress;
                let walker_diagnostics = state
                    .walker_diagnostics
                    .clone()
                    .unwrap_or_else(|| std::sync::Arc::from([]));
                // Drop the mutex guard before calling into the TLS fan-out
                // so the lock is released before any re-entrant observation
                // from a nested tracer scope.
                drop(state);

                // View-validation gate. A follower joining this
                // in-flight build is NOT guaranteed to be running
                // under the same view as the winner: two requests can
                // carry the same `SemanticQueryKey` while executing
                // under different overlays (a base context and a
                // session/overlay context, or two different overlays).
                // Their results are NOT interchangeable — each must
                // self-root-validate against its own content identity,
                // exactly as a warm hit (`MemoEntry::validate`) does.
                //
                // A cross-view joiner may reuse the winner's result
                // ONLY IF the winner's carrier carries a
                // view-discriminating self-root (a `FileWholeHash`
                // listed in `winner_self_roots`, routed through the
                // strict `validates_self_root_whole_hash`) AND that
                // self-root validates against THIS follower's `ctx`.
                // Both conditions are checked below; if either fails
                // the follower MUST NOT return the winner's node — it
                // forks and cold-recomputes for its own view.
                //
                // No-self-root fork. `validate_with_self_roots` only
                // DISCRIMINATES by view when the carrier carries a
                // self-root listed in `winner_self_roots`. A carrier
                // with no such fact validates VACUOUSLY against any
                // follower's `ctx` — the strict arm never fires — so a
                // bare `validate_with_self_roots` gate would coalesce a
                // follower in a different overlay onto the winner's
                // view-specific result. Three winner shapes have no
                // view-discriminating self-root:
                //
                //  - a tracer-overflow `cache_suppress` winner, whose
                //    carrier is a synthetic empty-fact carrier;
                //  - an unrootable `cache_suppress` winner
                //    (`semantic_graph_read_set_signature` returned
                //    `None`), whose carrier holds only cross-file
                //    *dependency* facts with an EMPTY `winner_self_roots`;
                //  - a NON-suppressed winner that completed with a
                //    view-specific `QueryResult::Error(Miss)` because a
                //    declaration is missing UNDER THE WINNER'S overlay:
                //    `cache_suppress` is `false`, yet the build could
                //    not self-root the keyed file and its carrier
                //    carries no listed self-root.
                //
                // The no-self-root fork therefore fires for ANY winner
                // lacking a view-discriminating self-root — it is NOT
                // gated on `cache_suppress`. The winner itself, and a
                // same-thread direct caller, still receive the winner's
                // result; only a cross-thread joiner under a
                // possibly-different overlay is forked. A genuinely
                // structural / view-invariant result also has no
                // self-root and is re-forked here — an accepted
                // redundant recompute: correctness over coalescing the
                // no-self-root class.
                //
                // A winner that DOES carry a real, view-discriminating
                // self-root (a `FileWholeHash` listed in
                // `winner_self_roots`) is left to the ordinary
                // `validate_with_self_roots` gate: a genuine same-view
                // joiner still coalesces — and a `cache_suppress`
                // winner with a real self-root still propagates
                // `cache_suppress` to that legitimately-coalescing
                // joiner.
                //
                // The fork removes the stale completed in-flight entry
                // (iff it is still the SAME `Arc` the follower joined —
                // a `ptr_eq` check; a third thread may already have
                // retired it and started a fresh flight) so the loop's
                // step 3 `table.entry(key)` creates a FRESH entry the
                // follower claims as cold winner. Forward progress is
                // deterministic: the follower does not re-join the same
                // completed entry, and the loop's step 1 warm read
                // (`get_validated`, validated under the follower's
                // `ctx`) also misses the winner's published entry, so
                // the follower runs its own cold build.
                if let Some(ref carrier) = graph_carrier {
                    let carrier_view_validates =
                        carrier.validate_with_self_roots(ctx, &winner_self_roots);
                    let lacks_view_discriminating_self_root =
                        !carrier.has_view_discriminating_self_root(&winner_self_roots);
                    if !carrier_view_validates || lacks_view_discriminating_self_root {
                        self.stats
                            .joiner_view_mismatch_forks
                            .fetch_add(1, Ordering::Relaxed);
                        {
                            let mut table = self.inflight.lock();
                            if table
                                .get(&key)
                                .is_some_and(|entry| Arc::ptr_eq(entry, &inflight))
                            {
                                table.remove(&key);
                            }
                        }
                        drop(inflight);
                        continue;
                    }
                }

                // Bubble the winner's path-precise fact rail into the
                // joiner thread's active tracer stack (if any). This
                // ensures that an outer cold-compute scope that spawned
                // this joiner sees all the facts the winner observed,
                // preserving completeness of the outer scope's
                // accumulated observation set. The winner records
                // `state.graph_carrier` for EVERY non-aborted build —
                // cacheable or `cache_suppress` — so this bubble carries
                // a suppressed child's transitive deps too.
                // Abort-path guard: `state.aborted` was checked above; the
                // `continue` path retries without reaching here, so we only
                // bubble on the non-aborted joiner path. The view-mismatch
                // fork above also `continue`s without reaching here, so the
                // bubble only carries a carrier the follower's view validated.
                if let Some(ref carrier) = graph_carrier {
                    carrier.bubble_via_tls();
                }
                if let Some(prov) = self.provenance.as_ref() {
                    prov.execute_cooperative_joiner_path
                        .fetch_add(1, Ordering::Relaxed);
                }
                return CacheRead {
                    value: result,
                    dep_signature,
                    walker_diagnostics,
                    cache_suppress,
                };
            }
            state.claimed = true;
            drop(state);
            break (inflight, key);
        };

        // Cold winner — record the in-flight presence for peak tracking.
        // The `InFlightStatsGuard` decrements `in_flight_current` on
        // drop so a panic in the cold build cannot leak the counter.
        self.stats.record_in_flight_enter();
        let _inflight_stats_guard = InFlightStatsGuard { stats: &self.stats };
        if let Some(prov) = self.provenance.as_ref() {
            prov.execute_cooperative_owner_path
                .fetch_add(1, Ordering::Relaxed);
        }

        // 4. Execute the cold build. Both the recursion stack entry and
        //    the in-flight admission are protected by RAII guards so a
        //    panic inside `build()` cannot deadlock future callers.
        let _recursion_guard = RecursionStackGuard::push(key.clone());
        let mut panic_guard =
            InflightPanicGuard::new(Arc::clone(&inflight), &self.inflight, key.clone());
        let build_start = Instant::now();
        let build_output: crate::project_semantic_dispatch::walk::QueryBuildOutput = build().into();
        let build_held_ns = build_start.elapsed().as_nanos() as u64;
        let crate::project_semantic_dispatch::walk::QueryBuildOutput {
            result,
            dep_signature,
            walker_diagnostics,
            cache_suppress,
            observed_self_roots: _,
            graph_carrier,
            self_root_canonicals,
            pending_prefix_backfills,
        } = build_output;
        let walker_diagnostics: std::sync::Arc<
            [crate::project_semantic_dispatch::walk::ShallowDiagnostic],
        > = std::sync::Arc::from(walker_diagnostics.into_boxed_slice());
        panic_guard.mark_finished();
        drop(panic_guard);
        drop(_recursion_guard);
        if let Some(prov) = self.provenance.as_ref() {
            prov.execute_cooperative_held_ns
                .fetch_add(build_held_ns, Ordering::Relaxed);
        }
        // Loop-5 instrumentation — count every cold build that
        // actually executed the build closure. `build_held_ns` is
        // accumulated alongside so the report can report mean ns/build.
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_COLD_BUILDS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::loop5_instrumentation::EXECUTE_COOPERATIVE_BUILD_NS_TOTAL
            .fetch_add(build_held_ns, std::sync::atomic::Ordering::Relaxed);

        // 5. Warm-publish only successful values; errors and recursion
        // sentinels never become shared-cache entries ( cache
        //    population). Successful results land in the requested
        //    `(family, slot)` and backfill every empty narrower slot in
        //    the same family — the backfill is a no-op against any slot a
        //    concurrent narrower compute already filled, so per-slot
        //    in-flight authority (§7.15) is preserved.
        //
        //    If a canonical invalidation swept this (family, slot) while
        //    the build was running, the winner's result is computed from
        //    pre-invalidation state — skip the warm publish so the sweep's
        //    eviction stays in effect. The next caller will run a fresh
        //    cold build under the new state of the world. This keeps the
        //    cache monotonic under invalidation: once the sweep removes a
        //    slot, no in-flight build from the pre-sweep epoch is allowed
        //    to resurrect it.
        //
        //    **TOCTOU guard.** We acquire `self.entries.lock()` FIRST and
        //    then re-check `inflight.state.aborted` under the entries
        //  lock before calling `publish`. Invalidation's also
        //    acquires `self.entries.lock()`; acquiring it here
        //    serialises us against invalidation. If invalidation got the
        //    entries lock first and aborted our in-flight via step 2,
        //    our re-check sees `aborted = true` and we skip publish. If
        //    we got the entries lock first, we publish and release;
        //  invalidation then evicts our fresh publish in its
        //    Either interleaving leaves the slot empty post-invalidation.
        //    A pre-lock check alone would leave a gap where a build
        //    result from a thread that checked `aborted=false` before
        //    acquiring `entries` could land AFTER invalidation's step 1
        //  completed but BEFORE set `aborted=true` — a stale
        //    slot whose dep-sig does NOT reference the invalidated
        //    canonical (so even HostFenceValidator does not catch it).
        // Refactor: cold-winner publish path is encapsulated in
        // `warm_publish_one` so that `publish_warm_if_absent` (used by
        // the §1.B prefix-backfill in `build_project_path`) can reuse the
        // same family/slot mapping + reverse-index registration without
        // duplicating the publish primitives. Pure refactor — TOCTOU
        // semantics, ResolvedNamedType bypass, and reverse-index
        // semantics all live inside the helper.
        // Memo no-poison contract: refuse insertion when the build's
        // `cache_suppress` flag is set (pathological-input cap fired,
        // or a transitive query produced a fatal `QueryError`). The
        // result still flows back to the caller, but the next request
        // re-runs cold so the suppression decision applies under the
        // current state of the world rather than being fossilised.
        // Resolve TWO carriers from the build output:
        //
        // - `broadcast_carrier` — ALWAYS resolved. It is bubbled into
        //   this winner thread's outer tracer AND recorded on the
        //   in-flight state so cross-thread joiners bubble the SAME
        //   fact rail. The production cold-build path
        //   (`ProjectSemanticDispatch::execute_via_cold_build_helper`)
        //   sets `graph_carrier` via `finalise_traced_build_output` —
        //   for a cacheable build the self-version-rooted carrier, and
        //   for a non-cacheable `Ok(traced)→None` build a non-admitted
        //   carrier still carrying the traced cross-file dep facts (so
        //   joiners inherit the suppressed child's transitive deps). A
        //   direct `execute_cooperative` caller that bypasses the
        //   dispatch's `traced_build` wrapper (test / debug drivers)
        //   leaves `graph_carrier` unset; such a build broadcasts a
        //   synthetic carrier — the legacy `dep_signature` rail and an
        //   empty fact rail. This is NOT a fence-fact reconstruction:
        //   the fact rail is left empty rather than reverse-engineered
        //   from the legacy `WholeHash` entries.
        //
        // - `publish_carrier` — `Some` only when the build is
        //   cacheable (`!cache_suppress`). A `cache_suppress` build
        //   (tracer overflow, pathological input, or an unrootable
        //   `None` signature) is non-cacheable: it is NEVER admitted to
        //   the memo. But its `broadcast_carrier` is still bubbled to
        //   joiners — broadcasting the build's dependency state is
        //   independent of memo admission.
        let broadcast_carrier: crate::fact_signature_helpers::ReadSetSignature = match graph_carrier
        {
            Some(boxed) => *boxed,
            None => crate::fact_signature_helpers::ReadSetSignature::new(
                crate::fact_signature_helpers::empty_fact_signature(),
                dep_signature.clone(),
            ),
        };
        let publish_carrier: Option<&crate::fact_signature_helpers::ReadSetSignature> =
            if cache_suppress {
                None
            } else {
                Some(&broadcast_carrier)
            };
        if let Some(carrier) = publish_carrier {
            let published = self.warm_publish_one(
                &key,
                &result,
                &walker_diagnostics,
                carrier,
                &self_root_canonicals,
                &inflight,
            );
            // Test-only injection point — parked AFTER `warm_publish_one`
            // published the parent and BEFORE the prefix-backfill loop,
            // so a race test can run `invalidate_all` (which marks this
            // winner's still-registered in-flight entry `aborted`) in the
            // exact window the `published` gate alone does not cover.
            // `None` (the production default) is a no-op.
            #[cfg(any(test, debug_assertions))]
            {
                let gate = self.cold_winner_pre_backfill_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
            // Prefix backfill: publish each accumulated backfill
            // record AFTER the parent entry is warm. The `published`
            // gate is the first abort check — a `false` return means
            // `warm_publish_one`'s TOCTOU re-check saw `aborted == true`
            // (a canonical invalidation or a project-generation reset
            // raced this cold build): the build's `SemanticNodeId`s were
            // interned against a now-stale id epoch, so the narrower
            // backfill nodes are equally stale and must NOT enter the
            // memo. But the gate alone is not sufficient: a reset can
            // start AFTER `warm_publish_one` returned `true` and before /
            // during this loop. Each `warm_publish_one_if_absent` call
            // therefore re-checks `inflight`'s `aborted` flag UNDER the
            // `entries` lock as well — symmetric with `warm_publish_one`,
            // so an aborted winner skips ALL its backfills regardless of
            // when the reset lands. See `invalidate_all`'s serialization
            // docs and `warm_publish_one_if_absent`'s abort fence.
            if published {
                for backfill in pending_prefix_backfills {
                    self.warm_publish_one_if_absent(
                        backfill.key,
                        QueryResult::Value(backfill.node),
                        carrier.clone(),
                        Arc::clone(&self_root_canonicals),
                        &inflight,
                    );
                }
            }
        } else {
            tracing::debug!(
                target: "verter::memo::suppress",
                key = ?key,
                "cache_suppress=true; refusing memo insertion (build-output suppression)"
            );
            // Per-request attribution of the no-poison gate. Bumped
            // when an in-flight build landed with `cache_suppress=true`
            // and we declined to publish at this gate. Surfaces on the
            // audit payload as `memo_publish_suppressed` so attribution
            // tests can assert "the cache_suppress gate fired during
            // this request" without inspecting host-global state.
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.memo_publish_suppressed.fetch_add(1, Ordering::Relaxed);
            }
        }
        {
            // Bubble the build's carrier fact rail into this winner
            // thread's still-active outer tracer (if any) —
            // UNCONDITIONALLY, cacheable or not. When this cold owner
            // build was nested under another semantic query, `build()`
            // installed a fresh tracer for the child build and popped
            // it before the carrier was available — so the child's
            // synthesised self-root `FileWholeHash` facts (added by
            // `semantic_graph_read_set_signature`, never observed onto
            // the tracer) live ONLY on this carrier. Without this
            // bubble the outer cold-compute scope's accumulated
            // observation set would miss the child's deps, and a parent
            // that cold-builds a child would publish with strictly
            // fewer deps than a parent that warm-hit / joined the same
            // child (the warm-hit fast path and the joiner path both
            // bubble the carrier). Bubbling here — for the
            // `cache_suppress` path too — makes a cold-built child, a
            // warm-hit child, and a joined child deliver the identical
            // fact set to the parent. The carrier is a poppable local;
            // bubbling consumes only its borrowed `facts` rail.
            broadcast_carrier.bubble(ctx);
        }

        // 6. Finalize in-flight and wake joiners. The completed flag
        //    guarantees any thread that acquired the flight before step 7
        //    retires the entry still observes the winner's result (or its
        //    abort sentinel, if the invalidation sweep set one while the
        //    winner was mid-build).
        {
            let mut state = inflight.state.lock();
            // Don't overwrite an abort sentinel planted by invalidation —
            // joiners that wake on the abort must observe `aborted = true`
            // and retry, not the (now-stale) winner result.
            if !state.aborted {
                state.completed = Some(result.clone());
                state.dep_signature = Some(dep_signature.clone());
                // Publish the SAME `broadcast_carrier` this winner
                // bubbled into its own outer tracer so cross-thread
                // joiners bubble the IDENTICAL fact rail into their own
                // active tracer stack. Winner and joiner therefore
                // agree on the identical fact set — for the
                // `cache_suppress` (non-cacheable) build too: the
                // carrier is broadcast regardless of memo admission, so
                // a joiner inside an outer cold query inherits the
                // suppressed child's transitive deps exactly as a
                // joiner of a cacheable child would. Only published on
                // the non-aborted path; the abort/retry branch above
                // (which executes `continue`) deliberately bypasses
                // this so joiners awakened by an abort sweep re-enter
                // dispatch rather than bubbling a stale carrier.
                state.graph_carrier = Some(Box::new(broadcast_carrier.clone()));
                // Publish the winner build's self-root canonicals so a
                // cross-thread joiner can validate `graph_carrier`
                // against ITS OWN view before reusing it. Two requests
                // can carry the same `SemanticQueryKey` while running
                // under different overlays; their results are NOT
                // interchangeable. The joiner validates the winner's
                // carrier strictly (self-roots through
                // `validates_self_root_whole_hash`) under the
                // follower's `ctx` — exactly as a warm hit
                // (`MemoEntry::validate`) does — and forks if it fails.
                state.self_root_canonicals = Arc::clone(&self_root_canonicals);
                // Propagate the build's non-cacheability flag so a
                // joiner returns the SAME `cache_suppress` the winner
                // returns. Without this a joiner of a `cache_suppress`
                // build returned `cache_suppress: false`; a composition
                // helper that threaded the joiner's read would then
                // publish an outer entry despite a non-cacheable
                // transitive child.
                state.cache_suppress = cache_suppress;
                state.walker_diagnostics = Some(std::sync::Arc::clone(&walker_diagnostics));
            }
        }
        inflight.ready.notify_all();

        // 7. Retire the in-flight entry regardless of publish status.
        //    Leaving the entry alive after a publish would let a later
        //    caller — e.g. after targeted invalidation drops the memo
        //    entry — latch onto the stale completed flag and skip the
        //    cold rebuild. Future callers after invalidation must start
        //    a fresh flight under the new state of the world.
        //
        //    The remove is `ptr_eq`-guarded: a cross-view joiner that
        //    failed this winner's carrier validation forks (see the
        //    view-validation gate above), removes THIS winner's entry,
        //    and may already have installed a FRESH `InflightEntry` for
        //    the same key as a new cold winner. An unconditional
        //    `remove` here would evict that fresh entry mid-build,
        //    spawning a redundant concurrent cold build. The guard
        //    removes only while the table still holds this winner's own
        //    `Arc` — exactly the entry whose stale `completed` flag
        //    step 7 exists to retire.
        {
            let mut table = self.inflight.lock();
            if table
                .get(&key)
                .is_some_and(|entry| Arc::ptr_eq(entry, &inflight))
            {
                table.remove(&key);
            }
        }
        // `_inflight_stats_guard` decrements `in_flight_current` on
        // scope exit (here on the normal-return path, also on panic
        // before this point thanks to the Drop impl).

        CacheRead {
            value: result,
            dep_signature,
            walker_diagnostics,
            cache_suppress,
        }
    }

    /// Cold-winner publish path. Extracted from
    /// [`Self::execute_cooperative`] step 5 (refactor — pure
    /// extraction, no behaviour change). Skips publish when the result is
    /// not a [`QueryResult::Value`] (errors / recursion sentinels never
    /// promote to warm cache entries — cache population). Skips
    /// the family memo for [`FamilyKey::ResolvedNamedType`] (§7.16 —
    /// ResolvedNamedType bypasses the family memo entirely; its
    /// DashMap-backed identity map is the cache).
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
    /// **Return value.** Returns `false` IFF the TOCTOU re-check
    /// observed `aborted == true` and the publish was skipped; returns
    /// `true` otherwise (published, or skipped for a non-abort reason —
    /// a non-`Value` result, or a `ResolvedNamedType` family). The
    /// caller's prefix-backfill loop is gated on this: an aborted winner
    /// was raced by a project-generation reset, so its build interned
    /// against a stale id epoch and its narrower backfills must be
    /// skipped too — see [`Self::invalidate_all`]'s serialization docs.
    fn warm_publish_one(
        &self,
        key: &SemanticQueryKey,
        result: &QueryResult<SemanticNodeId>,
        walker_diagnostics: &Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        inflight: &Arc<InflightEntry>,
    ) -> bool {
        let publishable = matches!(result, QueryResult::Value(_));
        if !publishable {
            // Not an abort — an error / recursion sentinel that never
            // promotes to a warm entry. The winner's build epoch is
            // still consistent, so backfills (if any) stay valid.
            return true;
        }
        let (family, slot) = family_and_slot(key);
        // ResolvedNamedType bypasses the family memo entirely (§7.16) —
        // its DashMap-backed identity map is the cache. Not an abort.
        if matches!(family, FamilyKey::ResolvedNamedType { .. }) {
            return true;
        }
        // The carrier is the COMPLETED self-version-rooted carrier the
        // shared cold-build helper produced via
        // `semantic_graph_read_set_signature` — it already leads with a
        // self-root `FileWholeHash` per observed self-root and merges
        // the traced cross-file fact set. `warm_publish_one` records it
        // verbatim; it never reconstructs facts from the legacy fence.
        let read_set_signature = read_set_signature.clone();
        let entry = MemoEntry {
            result: result.clone(),
            read_set_signature: read_set_signature.clone(),
            self_root_canonicals: Arc::clone(self_root_canonicals),
            walker_diagnostics: Arc::clone(walker_diagnostics),
        };
        let mut entries = self.entries_lock_diagnosed();
        // Test forcing: simulate a concurrent sweep that aborted
        // this in-flight entry just before the TOCTOU re-check.
        // Deterministically drives the `cold_aborts_swept` counter
        // for counter-helper coverage tests without needing a racy
        // real invalidation. The per-store flag default `false`
        // makes this branch a single relaxed atomic load on the
        // cold-build path under normal traffic, and — being
        // per-store — it cannot bleed into a concurrent unrelated
        // test's store (test hermeticity).
        if self.force_cold_abort_sweep.load(Ordering::Relaxed) {
            inflight.state.lock().aborted = true;
        }
        // Atomic re-check under the entries lock — `state` is briefly
        // locked nested inside `entries`; no AB-BA deadlock risk because
        // no path holds `state` then acquires `entries`.
        let aborted = inflight.state.lock().aborted;
        if aborted {
            drop(entries);
            // Canonical invalidation or a project-generation reset swept
            // this slot during the cold build; skip warm publish and
            // record the sweep. Returning `false` makes the caller skip
            // the prefix-backfill loop too — the build's ids were
            // interned against a now-stale epoch.
            record_cold_abort_swept(&self.stats);
            return false;
        }
        // Record whether this family is newly entering the memo so the
        // retention budget tracks one ledger record per family.
        let family_was_new = !entries.contains_key(&family);
        let populated_slots = entries
            .entry(family.clone())
            .or_default()
            .publish(slot, entry);
        // Per-request memo-insertion attribution. Each populated slot
        // (primary plus any backfilled narrower slots) counts as one
        // insertion under the active request's audit. The host-global
        // memo size remains the canonical signal for warm-cache state;
        // the per-request mirror lets attribution tests assert
        // "no synthesis-attributable insertions during this request"
        // without false positives from peer dispatches in
        // workspace-parallel runs.
        if !populated_slots.is_empty() {
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.memo_insertions
                    .fetch_add(populated_slots.len() as u64, Ordering::Relaxed);
            }
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
            self.record_family_admission_locked(&mut entries, &family);
        }
        // Γ.B reverse-index registration. Carrier-aware: register
        // each populated slot under EVERY canonical the entry's
        // carrier references — the union of the legacy `dep_signature`
        // rail AND the path-precise `facts` rail (`Parse(...)`,
        // `ResolveImports(...)`, `RouteSurface(...)`, etc.). Runs UNDER
        // the held `entries` lock: the `entries → canonical_to_entries
        // shards` lock order permits taking a shard mutex while `entries`
        // is held, and registering here — rather than after the lock
        // drop — keeps the live memo slot and its reverse-index
        // registration atomic against a concurrent `invalidate_all`. See
        // `register_reverse_index`'s docstring for the carrier contract.
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
        drop(entries);
        // Published cleanly under a non-aborted in-flight entry — the
        // winner's id epoch is consistent, so the caller may proceed to
        // publish its narrower prefix-backfills.
        true
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
    /// 2. The family is [`FamilyKey::ResolvedNamedType`] (per §7.16).
    /// 3. `self.get_unvalidated(&key).is_some()` — slot is already warm.
    /// 4. The in-flight table contains `key` — a cold winner is
    ///    currently building this exact key; let it publish.
    /// 5. The parent winner's in-flight entry is `aborted` (re-checked
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
    /// callers must pass the parent's authoritative carrier (legacy
    /// rail + path-precise traced facts captured under
    /// `install_fact_tracer`) so the backfilled entry's facts rail
    /// contains the parent's `Parse(...)` / `ResolveImports(...)` /
    /// `RouteSurface(...)` observations — never a fence-only
    /// reconstruction, which drops path-precise facts.
    fn warm_publish_one_if_absent(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        parent_inflight: &Arc<InflightEntry>,
    ) {
        if !matches!(result, QueryResult::Value(_)) {
            return;
        }
        let (family, slot) = family_and_slot(&key);
        if matches!(family, FamilyKey::ResolvedNamedType { .. }) {
            return;
        }
        // Skip if already warm OR currently in flight. Both checks
        // happen BEFORE acquiring the entries lock; a concurrent cold
        // winner publish that lands between this check and the publish
        // is benign (FamilySlots::publish overrides; both are computing
        // the same canonical prefix node so values agree).
        //
        // Use `contains_key` rather than `get` so the presence probe
        // does NOT bubble a stale entry's facts into the outer tracer
        // before declining to publish.
        if self.contains_key(&key) {
            return;
        }
        if self.inflight.lock().contains_key(&key) {
            return;
        }
        let entry = MemoEntry {
            result,
            read_set_signature: read_set_signature.clone(),
            self_root_canonicals,
            walker_diagnostics: Arc::from([]),
        };
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
        if parent_inflight.state.lock().aborted {
            drop(entries);
            record_cold_abort_swept(&self.stats);
            return;
        }
        let family_was_new = !entries.contains_key(&family);
        let populated_slots = entries
            .entry(family.clone())
            .or_default()
            .publish(slot, entry);
        // Per-request memo-insertion attribution — see
        // `warm_publish_one` for the full rationale; the prefix-backfill
        // path bumps the same per-request counter so attribution
        // tests cover both publish sites.
        if !populated_slots.is_empty() {
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.memo_insertions
                    .fetch_add(populated_slots.len() as u64, Ordering::Relaxed);
            }
        }
        // Family-memo consistency-cluster fence — record the
        // `memo_budget` admission under the held `entries` lock; see
        // `warm_publish_one`.
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, &family);
        }
        // Carrier-aware reverse-index registration — runs UNDER the held
        // `entries` lock; see `warm_publish_one` for the full carrier and
        // lock-order rationale.
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
        drop(entries);
    }

    /// Γ.B reverse-index registration helper. Shared by
    /// [`Self::warm_publish_one`] and
    /// [`Self::warm_publish_one_if_absent`]. The caller invokes this
    /// WHILE holding the `entries` lock — the `entries →
    /// canonical_to_entries shards` lock order permits taking a shard
    /// mutex while `entries` is held (`entries` is outermost), and
    /// registering under the lock keeps the live memo slot and its
    /// reverse-index registration atomic against a concurrent
    /// [`Self::invalidate_all`].
    ///
    /// **Carrier-aware registration.** The reverse index keys each
    /// populated slot under EVERY canonical the entry's
    /// [`ReadSetSignature`] references — the union of the legacy
    /// `dep_signature` rail AND the path-precise `facts` rail
    /// (`Parse(...)`, `ResolveImports(...)`, `RouteSurface(...)`,
    /// `FileWholeHash`, `DerivedFactHash`). The stored value in the
    /// per-canonical shard is the legacy `DepSignature` (kept as a
    /// diagnostic stamp of what was registered) but
    /// `invalidate_canonical`'s cross-canonical drain identifies
    /// registrations by `(family, slot)` entry identity rather than
    /// by `Arc::ptr_eq` on the stored signature. This prevents the
    /// "shared legacy Arc" hazard where two entries sharing a
    /// canonicalised legacy `Arc<DepSignature>` would have BOTH
    /// registrations stripped when only one is evicted — see the
    /// drain implementation in `invalidate_canonical` for the
    /// rationale. Iterating `read_set_signature.canonical_ids()`
    /// ensures fact-only canonicals (whose canonical does not appear
    /// in the legacy rail) still surface in `invalidate_canonical`'s
    /// shard-drain — without this, a `Parse(MemberPresence(Foo, a))`
    /// fact for a canonical that the legacy signature does not name
    /// would leave the memo entry orphaned across invalidation.
    ///
    /// **Consistency with `MaterializeStructureDb::register_post_publish`.**
    /// `MaterializeStructureDb` and `RefCycleResultDb` register their
    /// reverse indexes over the same `canonical_ids()` iteration so they
    /// drain on fact-only invalidation; this helper registers the family
    /// memo's reverse index over the identical iteration, so all three
    /// reverse indexes drain consistently.
    fn register_reverse_index(
        canonical_to_entries: &CanonicalToEntries,
        family: &FamilyKey,
        populated_slots: &[ModeSlot],
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        let registered_legacy = Arc::clone(&read_set_signature.legacy);
        for populated in populated_slots {
            for canonical in read_set_signature.canonical_ids() {
                let shard = canonical_to_entries
                    .entry(canonical)
                    .or_insert_with(|| Mutex::new(FxHashMap::default()));
                let lock_start = if timing_on {
                    Some(Instant::now())
                } else {
                    None
                };
                let mut map = shard.value().lock();
                let lock_wait = lock_start
                    .map(|t| t.elapsed())
                    .unwrap_or(std::time::Duration::ZERO);
                crate::host_manage::record_family_map_lock_acquisition(lock_wait);
                map.insert((family.clone(), *populated), Arc::clone(&registered_legacy));
            }
        }
    }

    /// Record a newly-admitted family against the memo retention
    /// budget and FIFO-evict the oldest families once the family count
    /// exceeds the budget cap. Evicting a still-valid family only forces
    /// a recompute; it never yields an incorrect result.
    ///
    /// Called exactly once per family that newly enters the `entries`
    /// memo (a re-publish into a different slot of an already-present
    /// family does not re-record).
    ///
    /// **Family-memo consistency-cluster fence.** The caller passes the
    /// `entries` guard it is already holding — the `memo_budget`
    /// admission is recorded, the FIFO victims are removed from
    /// `entries`, AND each evicted victim's `canonical_to_entries`
    /// reverse-index registrations are pruned, ALL WITHIN that same lock
    /// hold. The family memo is a three-member consistency cluster
    /// (`entries`, `memo_budget`, `canonical_to_entries`); a concurrent
    /// [`Self::invalidate_all`] clears all three under the SAME `entries`
    /// lock, so the publish's `(entries slot, memo_budget record,
    /// reverse-index register)` triple is atomic against the reset.
    ///
    /// Pruning the victim's reverse index here — rather than deferring it
    /// past the `entries`-lock drop — closes the race where a fresh
    /// same-`(family, slot)` re-publish registers into
    /// `canonical_to_entries` between the victim's `entries` removal and
    /// a deferred key-only reverse-index cleanup, which would delete the
    /// fresh registration and leave the live re-published memo slot
    /// invisible to `invalidate_canonical`. With the prune inside the
    /// `entries` lock — and the publish-side `register_reverse_index`
    /// also under that lock — no concurrent publish can register a fresh
    /// `(family, slot)` in the gap, so there is no gap. The `entries →
    /// canonical_to_entries shards` lock order PERMITS taking a shard
    /// mutex while `entries` is held (`entries` is outermost), and no
    /// path takes a `canonical_to_entries` shard mutex then `entries`, so
    /// this nesting is sound.
    ///
    /// The injection point armed by
    /// [`Self::test_publish_post_memo_budget_record_gate`] fires after
    /// the admission lands; the one armed by
    /// [`Self::test_publish_post_reverse_index_prune_gate`] fires after
    /// the victims' reverse-index registrations are pruned — both with
    /// the `entries` lock still held.
    fn record_family_admission_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        family: &FamilyKey,
    ) {
        const ALL_SLOTS: [ModeSlot; 6] = [
            ModeSlot::Single,
            ModeSlot::Identity,
            ModeSlot::Navigate,
            ModeSlot::Shallow,
            ModeSlot::Expanded,
            ModeSlot::Skeleton,
        ];
        let seq = crate::bounded_query_retention::next_retention_seq();
        let victims = self.memo_budget.record_admission(seq, family.clone());
        // Test-only injection point — parked after the `memo_budget`
        // admission lands and with the `entries` lock still held, so a
        // race test can assert the admission is recorded in the `entries`
        // lock domain. `None` (the production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.publish_post_memo_budget_record_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        // `record_admission` hands back `(seq, FamilyKey)` victims. The
        // removal here is by `FamilyKey` alone, NOT by admission seq —
        // sound because this whole method runs under the caller's
        // exclusive `entries` lock (`&mut FxHashMap<FamilyKey,
        // FamilySlots>`), the same lock domain every
        // `record_family_admission_locked` records under and that
        // `invalidate_all` clears `entries` + `memo_budget` under. With
        // the exclusive lock held no concurrent writer can re-admit a
        // FIFO victim's `FamilyKey` between `record_admission` and this
        // drain, so a key-based removal cannot evict a fresh same-key
        // re-admission. The `GlobalRetentionBudget` victim-identity
        // contract permits a key-based removal for exactly this
        // exclusive-lock-serialised case.
        // Tracks whether at least one victim reverse-index registration
        // was pruned, so the post-prune injection point fires only when a
        // FIFO eviction actually happened. `cfg`-gated — absent from
        // release builds, where the gate block is also compiled out.
        #[cfg(any(test, debug_assertions))]
        let mut pruned_any = false;
        for (_victim_seq, victim) in victims {
            // Remove the victim family from the held `entries` map (so
            // map and budget stay in lockstep), then immediately prune
            // each populated slot's `canonical_to_entries` reverse-index
            // registration under this same `entries`-lock hold so the
            // three cluster members stay consistent.
            if let Some(slots) = entries.remove(&victim) {
                for slot in ALL_SLOTS {
                    if let Some(entry) = slots.slot(slot) {
                        for canonical in entry.read_set_signature.canonical_ids() {
                            Self::prune_reverse_index_registration(
                                &self.canonical_to_entries,
                                &canonical,
                                &(victim.clone(), slot),
                            );
                            #[cfg(any(test, debug_assertions))]
                            {
                                pruned_any = true;
                            }
                        }
                    }
                }
            }
        }
        // Test-only injection point — parked after the FIFO victims'
        // `canonical_to_entries` reverse-index registrations have been
        // pruned and with the `entries` lock still held, so a race test
        // can assert the reverse-index prune runs in the `entries` lock
        // domain. Fires only when at least one victim registration was
        // pruned (so a publish that evicts nothing does not park).
        // `None` (the production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            if pruned_any {
                let gate = self.publish_post_reverse_index_prune_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
        }
    }

    /// Remove one `(family, slot)` registration from `canonical`'s
    /// reverse-index shard, then drop the outer shard entirely when that
    /// removal empties its inner map.
    ///
    /// Leaving an emptied shard resident strands an empty `Mutex<map>`
    /// plus the canonical `Arc<str>` under `canonical_to_entries` for
    /// the lifetime of the project generation; under churn across many
    /// distinct canonicals the reverse index would then grow unbounded,
    /// defeating the bound the `memo_budget` exists to enforce.
    ///
    /// **Lock domain.** Both production callers —
    /// [`Self::record_family_admission_locked`]'s FIFO-victim cleanup
    /// and [`Self::invalidate_canonical`]'s cross-canonical cleanup —
    /// invoke this WHILE holding the family memo's `entries` `Mutex`, the
    /// single lock domain the consistency cluster (`entries`,
    /// `memo_budget`, `canonical_to_entries`) is mutated under. The
    /// publish-side [`Self::register_reverse_index`] inserter also runs
    /// under `entries`, so a prune and a registration are serialised by
    /// the `entries` lock and can never interleave.
    ///
    /// **Shard-detach safety.** Even absent the `entries` lock the
    /// outer-shard drop is race-free: the inner-map removal releases the
    /// per-canonical `Mutex` before the outer drop is attempted; the
    /// outer drop is then a single [`DashMap::remove_if`] whose emptiness
    /// predicate runs while the `DashMap` shard write lock is held. A
    /// `register_reverse_index` inserter takes that same shard write lock
    /// for the whole `entry(canonical).or_insert_with(...)` + inner
    /// `insert`, so the two serialise: either the inserter runs first and
    /// `remove_if`'s predicate observes the inner map non-empty (drop
    /// skipped), or `remove_if` runs first, drops the empty outer entry,
    /// and the inserter's later `or_insert_with` re-creates a fresh shard
    /// cleanly. A registration can never be stranded in a just-removed
    /// outer entry. This mirrors the
    /// `BoundedCandidateMap::remove_candidate_by_seq` slot-detach
    /// pattern.
    fn prune_reverse_index_registration(
        canonical_to_entries: &CanonicalToEntries,
        canonical: &Arc<str>,
        registration: &(FamilyKey, ModeSlot),
    ) {
        if let Some(shard) = canonical_to_entries.get(canonical) {
            shard.value().lock().remove(registration);
        }
        // Drop the outer shard iff its inner map is now empty. The
        // predicate holds the `DashMap` shard write lock, so it cannot
        // race a concurrent inserter on this shard.
        canonical_to_entries.remove_if(canonical, |_, mutex| mutex.lock().is_empty());
    }

    /// Test-only accessor: read the entry's full
    /// [`ReadSetSignature`] (both rails) for `key`. Returns `None`
    /// when no entry is present.
    ///
    /// Unlike [`Self::get`] (which projects only the legacy
    /// `dep_signature` into the returned `CacheRead`), this accessor
    /// surfaces the path-precise `facts` rail so integration tests
    /// can assert what facts the entry's carrier actually holds.
    /// Used by the `prefix_backfill_carries_traced_facts` discriminator
    /// to assert that a backfilled prefix entry's `facts` rail contains
    /// the parent's traced `Parse(...)` fact rather than a fence-only
    /// reconstruction.
    #[doc(hidden)]
    #[must_use]
    pub fn entry_read_set_signature_for_tests(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .and_then(|slots| slots.slot(slot).cloned())
            .map(|entry| entry.read_set_signature)
    }

    /// Test-only direct publish path. Constructs a `MemoEntry` from the
    /// caller-supplied `(result, read_set_signature, self_root_canonicals)`
    /// tuple and routes it through the unified reverse-index
    /// registration. Mirrors the production publish path but accepts an
    /// explicit carrier + self-root canonical set so integration tests
    /// can seed entries whose `legacy` rail excludes canonicals that the
    /// `facts` rail names (the fact-only invalidation discriminator) and
    /// drive the strict self-root warm-read validator.
    ///
    /// A newly-keyed family records an admission against the
    /// `memo_budget` while holding the `entries` lock (matching
    /// `warm_publish_one`'s `record_family_admission_locked` call) so a
    /// test publishing many distinct families drives the same FIFO
    /// budget eviction — under the same map/budget lifecycle fence — as
    /// the production path does.
    ///
    /// Returns the number of populated slots after publish (always
    /// ≥1 for a `Value` result on a previously-empty slot).
    #[doc(hidden)]
    pub fn publish_with_carrier_for_tests(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
    ) -> usize {
        if !matches!(result, QueryResult::Value(_)) {
            return 0;
        }
        let (family, slot) = family_and_slot(&key);
        if matches!(family, FamilyKey::ResolvedNamedType { .. }) {
            return 0;
        }
        let entry = MemoEntry {
            result,
            read_set_signature: read_set_signature.clone(),
            self_root_canonicals,
            walker_diagnostics: Arc::from([]),
        };
        let mut entries = self.entries_lock_diagnosed();
        let family_was_new = !entries.contains_key(&family);
        let populated_slots = entries
            .entry(family.clone())
            .or_default()
            .publish(slot, entry);
        // Family-memo consistency-cluster fence — record the
        // `memo_budget` admission and register the `canonical_to_entries`
        // reverse index UNDER the held `entries` lock; see
        // `warm_publish_one`.
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, &family);
        }
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
        drop(entries);
        populated_slots.len()
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

/// Per-key error returned by `SemanticGraphStore::execute_cooperative_batch`
/// (D103). Mirrors the proto `BatchExpandError` enum so the BFS bridge can
/// project per-key failures into a typed `BridgeError::StaleAtFrontier`
/// envelope without losing the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchExpandError {
    /// Canonical's content hash changed between the surface envelope's
    /// `TypeHandle` stamp and this batch's read.
    StaleContentChanged,
    /// Canonical was deleted from the host between stamp and read.
    FileDeleted,
    /// The declaration the handle pointed at no longer exists in the
    /// current `OwnedTypeResolutionContext::declaration_fingerprints`
    /// table.
    DeclarationRemoved,
    /// The semantic node was evicted from the warm memo (e.g. by a
    /// generation bump under memory pressure) and would require a cold
    /// rebuild that the batch path is not authorised to perform.
    EvictedNode,
}

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
pub(crate) type ObservedGraphSelfRoot = (Arc<str>, crate::types::Hash16);

/// Build the [`ReadSetSignature`](crate::fact_signature_helpers::ReadSetSignature)
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
/// fact set the `install_fact_tracer` scope collected; `legacy` is the
/// whole-hash / project-generation `DepSignature` rail retained so
/// `ProjectGeneration` stays validated by `validate_dep_signature`
/// until the generation rail is unified. Re-reading a canonical's
/// current hash here would reopen the publish race the central
/// fact-signature helpers close.
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
///   file's version), or
/// - the legacy rail carries a `RouteGeneration` dependency: route
///   generation has no authoritative validating source — there is no
///   production emitter, and `HostFenceValidator` rejects it fail-safe
///   (the `RouteGeneration` arm returns `false`) — so an entry rooted
///   on it could not detect a content edit to the route-observed file.
///   No production path constructs the variant; producers refuse
///   admission so no entry's legacy rail carries it.
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
// (`tests/semantic_graph_signature_builder_provenance.rs`) bans
// `authoritative_current_content_hash`, `current_file_facts`,
// `parse_fact_ref(`, `self_root_fact`, and `shallow_file_state` inside
// this body.
pub(crate) fn semantic_graph_read_set_signature(
    observed_self_roots: &[ObservedGraphSelfRoot],
    traced_facts: &[crate::resolver_core::FactVersionRef],
    legacy: &DepSignature,
) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
    use crate::resolver_core::FactVersionRef;
    use crate::semantic_query::DepVersion;

    // A `RouteGeneration` legacy dependency cannot be soundly rooted —
    // it has no authoritative validator and no production emitter.
    // Refuse shared admission rather than caching an entry that cannot
    // detect a content edit to the route-observed file.
    if legacy
        .iter()
        .any(|(_, v)| matches!(v, DepVersion::RouteGeneration(_)))
    {
        return None;
    }

    // Collapse the observed self-roots into a per-canonical hash map;
    // a conflicting hash for the same canonical is a torn observation.
    let mut self_root_hashes: rustc_hash::FxHashMap<Arc<str>, crate::types::Hash16> =
        rustc_hash::FxHashMap::default();
    for (canonical, observed_hash) in observed_self_roots {
        match self_root_hashes.get(canonical) {
            Some(existing) if existing != observed_hash => return None,
            _ => {
                self_root_hashes.insert(Arc::clone(canonical), *observed_hash);
            }
        }
    }

    let mut facts: Vec<FactVersionRef> =
        Vec::with_capacity(self_root_hashes.len() + traced_facts.len());

    // Lead with one self-root `FileWholeHash` per observed self-root,
    // pinned to the OBSERVED content version.
    for (canonical, observed_hash) in &self_root_hashes {
        facts.push(FactVersionRef::FileWholeHash {
            canonical_id: canonical.as_ref().to_string(),
            hash: *observed_hash,
        });
    }

    // Merge the traced fact set. A traced `FileWholeHash` for a
    // self-root canonical is folded onto the observed self-root: it
    // MUST agree with the observed hash (else the dependency rail and
    // the observed self-root disagree on the keyed file's version — a
    // torn read). Every other traced fact is kept verbatim so
    // transitive cross-file invalidation is preserved.
    for fact in traced_facts {
        if let FactVersionRef::FileWholeHash { canonical_id, hash } = fact {
            if let Some(observed_hash) = self_root_hashes.get(canonical_id.as_str()) {
                if hash != observed_hash {
                    return None;
                }
                // Already emitted as a self-root above — do not
                // duplicate.
                continue;
            }
        }
        facts.push(fact.clone());
    }

    Some(crate::fact_signature_helpers::ReadSetSignature::new(
        Arc::from(facts),
        Arc::clone(legacy),
    ))
}

impl SemanticGraphStore {
    /// Test-only driver: set `aborted = true` on the in-flight entry
    /// for `key`, plant an `Error(Other)` sentinel on `completed` if
    /// absent, notify waiters, and remove the entry from the table.
    /// Mirrors `invalidate_canonical` exactly but bypasses the step 1
    /// warm-slot gate so joiner-retry tests don't have to race a real
    /// invalidation window between publish and inflight retirement.
    ///
    /// Returns `true` when an entry for `key` was aborted, `false` when
    /// the in-flight table did not contain the key.
    ///
    /// `#[doc(hidden)]` and reached only through the `for_tests`
    /// re-export shim (`crate::for_tests::test_trigger_inflight_abort`)
    /// so the integration-test surface in
    /// `crates/verter_session/tests/` can drive joiner retry without
    /// loosening the public API of `SemanticGraphStore`. In-crate
    /// tests reach the same body via the same shim function.
    #[doc(hidden)]
    pub fn test_trigger_inflight_abort_impl(&self, key: &SemanticQueryKey) -> bool {
        let mut table = self.inflight.lock();
        let Some(inflight) = table.remove(key) else {
            return false;
        };
        drop(table);
        {
            let mut state = inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "aborted by test_trigger_inflight_abort",
                ))));
                state.dep_signature = Some(empty_signature());
            }
        }
        inflight.ready.notify_all();
        true
    }

    /// Test-only observability accessor: non-destructively read the
    /// `Arc::strong_count` of the in-flight entry for `key`, or `0` if
    /// the table has no entry.
    ///
    /// Joiner-retry tests use this to deterministically synchronise:
    /// each caller of `execute_cooperative` clones the entry's `Arc`
    /// (step 3: `table.entry(key).or_insert_with(...).clone()`). While
    /// only the cold winner is mid-build, three references are live —
    /// the table entry, the winner's `inflight` local, and the
    /// `InflightPanicGuard`'s clone — so the count is `3`; an admitted
    /// joiner raises it to `4`. Polling this to `> 3` replaces a
    /// wall-clock `sleep` that races the joiner under parallel test
    /// load (test hermeticity) — it never touches the entry's state,
    /// so it cannot perturb the build it observes.
    ///
    /// `#[doc(hidden)]` and reached only through the `for_tests`
    /// re-export shim, mirroring `test_trigger_inflight_abort`.
    #[doc(hidden)]
    #[must_use]
    pub fn test_inflight_strong_count(&self, key: &SemanticQueryKey) -> usize {
        let table = self.inflight.lock();
        table.get(key).map_or(0, Arc::strong_count)
    }

    /// Test-only probe: `true` when the `inflight` table `Mutex` can be
    /// `try_lock`-acquired right now (no thread is holding it). The
    /// abort-loop lock-order test uses it to assert [`Self::invalidate_all`]
    /// does not hold the table lock while locking each collected entry's
    /// `state` (the collect-then-release order).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_inflight_table_is_unlocked(&self) -> bool {
        self.inflight.try_lock().is_some()
    }

    /// Public test driver: set this store's per-store cold-abort
    /// trigger for the duration of the returned guard so the next
    /// `execute_cooperative` cold-build on **this store**
    /// deterministically hits the TOCTOU abort path. Used by
    /// integration tests in `crates/verter_session/tests/` that drive
    /// the counter-helper plumbing.
    ///
    /// The trigger is scoped to the store the guard borrows — a test
    /// forcing an abort affects only its own store, never a
    /// concurrently-running unrelated test's store. The guard restores
    /// the flag to `false` on drop. Tests must hold the guard for the
    /// duration of the `execute_cooperative` call.
    #[doc(hidden)]
    #[must_use]
    pub fn test_force_cold_abort_sweep(&self) -> TestForceColdAbortGuard<'_> {
        self.force_cold_abort_sweep.store(true, Ordering::SeqCst);
        TestForceColdAbortGuard {
            flag: &self.force_cold_abort_sweep,
        }
    }
}

impl SemanticGraphRead for SemanticGraphStore {
    fn node_data(&self, node: SemanticNodeId) -> Arc<SemanticNodeData> {
        SemanticGraphStore::node_data(self, node).unwrap_or_else(|| {
            // Missing node id — fabricate an Opaque sentinel rather than
            // panicking. Ids are only handed out by `intern_node`, so this
            // is defensive; in debug builds the arena invariant is
            // expected to be consistent.
            Arc::new(SemanticNodeData::Opaque(QueryError::Miss))
        })
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for SemanticGraphStore {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[FileContent, TypeGraph, ResolverState, ProjectGeneration]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, ProjectGeneration) {
            // `invalidate_all` itself clears the resolved-named-type
            // identity map (and every other `SemanticNodeId`-keyed
            // structure) so no stale `SemanticNodeId`-keyed judgement
            // survives the project-generation bump. The node arena is
            // append-only and is not reset.
            let _ = self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for SemanticGraphStore {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let n_memo = self.invalidate_canonical(canonical_id);
        let n_named = self.invalidate_resolved_named_types_for_canonical(canonical_id);
        n_memo + n_named
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Public test driver: build an empty `DepSignature` for tests in the
/// integration-test crate that drive `execute_cooperative` directly.
/// The integration-test surface is not part of the production resolver
/// stack — its only job is to discriminate per-request counter
/// attribution, so an empty signature is sufficient.
#[doc(hidden)]
#[allow(dead_code)]
#[must_use]
pub fn empty_signature_for_tests() -> DepSignature {
    empty_signature()
}

/// Public test driver: trigger an in-flight abort for `key` on `store`.
/// Forwards to [`SemanticGraphStore::test_trigger_inflight_abort_impl`]
/// so integration tests in `crates/verter_session/tests/` and in-crate
/// `tests.rs` drive the same joiner-retry body through one call site.
#[doc(hidden)]
#[allow(dead_code)]
pub fn test_trigger_inflight_abort(store: &SemanticGraphStore, key: &SemanticQueryKey) -> bool {
    store.test_trigger_inflight_abort_impl(key)
}

/// Public test driver: read the in-flight entry's `Arc` strong count
/// for `key` on `store`. Forwards to
/// [`SemanticGraphStore::test_inflight_strong_count`] so joiner-retry
/// tests deterministically poll for joiner admission instead of
/// sleeping. See that method's docs for the strong-count contract
/// (`3` while only the winner is mid-build, `4` once a joiner joins).
#[doc(hidden)]
#[allow(dead_code)]
#[must_use]
pub fn test_inflight_strong_count(store: &SemanticGraphStore, key: &SemanticQueryKey) -> usize {
    store.test_inflight_strong_count(key)
}

// ──────────────────────────────────────────────────────────────────────────
// Counter helpers (Decision #5: single helper, dual-target write)
// ──────────────────────────────────────────────────────────────────────────
//
// `inflight_aborted_retries` and `cold_aborts_swept` need both
// host-owned global aggregates AND per-request attribution. Bumping
// only the global leaves the audit miner's `CacheOutcomeTally` blind;
// bumping only the per-request breaks every existing telemetry
// consumer that reads `stats_snapshot()`. The helpers below collapse
// both writes into one call site so the two halves cannot diverge.
//
// The helpers consult `current_request_context()` directly (the
// session-side TLS slot installed by `RequestContextGuard::install`).
// This matches the existing per-request helpers in `host_manage`
// (`record_materialize_structure_call`, `record_dep_signature_merge`,
// etc.) and keeps the audit-mining flow homogeneous: every counter
// the miner reads from `RequestContext` is written through a helper
// that consulted `current_request_context()`.
//
// Architecture guard `audit_counter_single_helper` (in
// `crates/verter_session/tests/architecture_guards.rs`) rejects any
// direct `self.stats.<counter>.fetch_add` for these two counters
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
fn record_inflight_aborted_retry(stats: &AtomicSemanticGraphStats) {
    stats
        .inflight_aborted_retries
        .fetch_add(1, Ordering::Relaxed);
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.inflight_aborted_retries.fetch_add(1, Ordering::Relaxed);
    }
}

/// Bump `cold_aborts_swept` on both the global `SemanticGraphStats`
/// AND, when a `RequestContext` is installed in TLS on the calling
/// thread, the per-request mirror.
///
/// Mirrors [`record_inflight_aborted_retry`] for the cold-abort sweep
/// counter that the TOCTOU re-check fires on a swept publish.
#[inline]
fn record_cold_abort_swept(stats: &AtomicSemanticGraphStats) {
    stats.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
    }
}

/// RAII guard returned by
/// [`SemanticGraphStore::test_force_cold_abort_sweep`]. Borrows the
/// driving store's per-store
/// [`force_cold_abort_sweep`](SemanticGraphStore::force_cold_abort_sweep)
/// flag and restores it to `false` on drop, so a panicking test does
/// not leak the trigger onto a later `execute_cooperative` on the same
/// store. The trigger never reaches another store, so sibling tests
/// running in parallel are unaffected regardless.
#[doc(hidden)]
pub struct TestForceColdAbortGuard<'a> {
    flag: &'a std::sync::atomic::AtomicBool,
}

impl Drop for TestForceColdAbortGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
