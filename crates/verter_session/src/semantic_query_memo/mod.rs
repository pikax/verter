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
mod derivation;
mod family;
mod inflight;
mod interner;
mod stats;

#[allow(unused_imports)]
pub use interner::DepSignatureInterner;
#[cfg(test)]
use interner::SWEEP_INTERVAL;

use arena::NodeArena;
#[cfg(test)]
use arena::{shard_index_for, NUM_SHARDS};
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
    /// read-path shape. Per, `SemanticQueryKey::ResolvedNamedType`
    /// bypasses the family memo entirely — this `DashMap` is the cache,
    /// and `execute_cooperative` short-circuits straight to the build
    /// closure for that variant.
    named_type_index: DashMap<HostResolvedNamedTypeKey, SemanticNodeId>,
    /// Relation-engine memo. Maps `(source, target)` semantic-node pairs
    /// to the tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) plus the
    /// self-version-rooted carrier + the self-root canonical set used
    /// for strict warm-hit validation. Separate from the family memo
    /// because relation identity is pairwise, not single-node.
    ///
    /// The stored [`RelationMemoEntry`] is validated on every warm read
    /// (`get_relation`) — every self-root canonical's `FileWholeHash` is
    /// validated strictly, so a same-canonical content edit to either
    /// the source's or the target's originating file misses the warm
    /// relation judgement and forces a recompute.
    relation_memo: DashMap<(SemanticNodeId, SemanticNodeId), RelationMemoEntry>,
    /// Sibling derivation/origin layer (plan B2 + Derivation/Origin Layer
    /// Contract). Edges are keyed by `(result_node, kind)`; multiple
    /// derivations of the same structural result store multiple edges per
    /// key. Edge dep-signatures are interned in the store's signature pool
    /// so per-builder fence snapshots share allocations.
    derivation: Mutex<DerivationStore>,
    /// Lock-free telemetry counters (plan B2 + §7.4). Read via
    /// [`Self::stats_snapshot`] into the public [`SemanticGraphStats`]
    /// surface.
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
    /// **Lock order.** `entries → canonical_to_entries shards`. Code
    /// must NEVER acquire a `canonical_to_entries` shard mutex while
    /// holding `entries`, and never acquire `entries` while holding
    /// any `canonical_to_entries` shard mutex. The DashMap shard
    /// boundary is the per-canonical Mutex.
    canonical_to_entries: CanonicalToEntries,
    /// Global insertion-ordered total-size budget for the family memo.
    /// Each `FamilyKey` is built from content-derived `SemanticNodeId`s
    /// / a `DeclIdentity` embedding the file whole-hash, so a content
    /// edit produces fresh families. The reverse-index drain reclaims
    /// only on per-canonical invalidation, which an owner-content edit
    /// no longer triggers — this budget is the routine reclamation:
    /// publishing a newly-keyed family records an admission and the
    /// oldest families past the cap are FIFO-evicted write-side.
    memo_budget: crate::bounded_query_retention::GlobalRetentionBudget<FamilyKey>,
    /// Global total-size budget for the relation memo. `(source,
    /// target)` `SemanticNodeId` pairs are content-derived, so the
    /// relation memo grows with content edits without this cap.
    relation_budget:
        crate::bounded_query_retention::GlobalRetentionBudget<(SemanticNodeId, SemanticNodeId)>,
    /// Global total-size budget for the Vue-macro resolved-named-type
    /// identity map. `HostResolvedNamedTypeKey` embeds the owner's
    /// content hash, so each owner content version is a fresh key.
    named_type_budget:
        crate::bounded_query_retention::GlobalRetentionBudget<HostResolvedNamedTypeKey>,
}

/// Γ.B reverse-index type alias. See
/// [`SemanticGraphStore::canonical_to_entries`] for the contract.
type CanonicalToEntries = DashMap<Arc<str>, Mutex<FxHashMap<(FamilyKey, ModeSlot), DepSignature>>>;

/// One entry in the relation memo
/// ([`SemanticGraphStore::relation_memo`]).
///
/// A relation judgement for a `(source, target)` semantic-node pair is
/// self-version-rooted: `carrier` leads with a self-root `FileWholeHash`
/// for each file-derived input node's originating file, so a content
/// edit to either the source's or the target's file misses the warm
/// relation read. `self_root_canonicals` is the strict self-root
/// canonical set the warm validator
/// ([`SemanticGraphStore::get_relation`]) checks via
/// [`crate::fact_signature_helpers::ReadSetSignature::validate_with_self_roots`].
#[derive(Clone)]
struct RelationMemoEntry {
    /// The self-version-rooted carrier — built by
    /// [`semantic_graph_read_set_signature`] from the relation build's
    /// observed self-roots, the traced fact set, and the legacy
    /// `DepSignature` rail.
    carrier: crate::fact_signature_helpers::ReadSetSignature,
    /// The strict self-root canonical set — the file-derived origins of
    /// `source` and `target`.
    self_root_canonicals: Arc<[Arc<str>]>,
    /// The cached tri-state relation result.
    result: crate::semantic_query::RelationResult,
}

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

    /// Diagnosis accessor: number of distinct interned
    /// `DepSignature` payloads in the derivation-signature pool. Used
    /// by the diagnosis benchmark to record the pool's growth across
    /// scenarios — `record_signature_pool_size` on the active capture
    /// token reads this value at end-of-capture.
    #[must_use]
    pub fn derivation_signature_pool_size(&self) -> usize {
        self.derivation.lock().signature_pool.len()
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
    /// `arena: NodeArena` and `relation_memo: DashMap` in production code.
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

        // Drain the per-canonical
        // (family, slot) → registered_dep_signature map for
        // `canonical_id`. The drain releases the per-canonical mutex
        // before acquires `entries`, preserving the
        // documented `entries → canonical_to_entries shards` lock
        // order. `affected_pairs` is retained so (in-flight
        // abort) can drop matching in-flight entries even when phase
        // 2's `Arc::ptr_eq` check rejects an entry (e.g., a fresh
        // post-publish write replaced the registered dep_signature).
        let mut affected_pairs: FxHashSet<(FamilyKey, ModeSlot)> = FxHashSet::default();
        let drained: Vec<((FamilyKey, ModeSlot), DepSignature)> = {
            let timing_on = verter_scheduler::request_context::current_timing_enabled();
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
                    let drained: Vec<_> = map.drain().collect();
                    drained
                }
                None => {
                    // Still account for the canonical-shard removal itself
                    // as one observed acquisition; the DashMap shard read
                    // is implicit in `remove`. When the entry was absent
                    // there is no inner mutex to time, so the wait is
                    // zero.
                    crate::host_manage::record_family_map_lock_acquisition(
                        std::time::Duration::ZERO,
                    );
                    Vec::new()
                }
            }
        };
        for ((family, slot), _) in &drained {
            affected_pairs.insert((family.clone(), *slot));
        }

        // Walk the
        // drained set under the entries lock. Drop each slot whose
        // current dep_signature `Arc::ptr_eq`-matches the registered
        // dep_signature. ptr_eq distinguishes "our entry" from "a
        // fresh post-publish write that beat us". Track a fallback
        // dep-sig walk for any slot that did not ptr_eq (the
        // registered dep_sig was replaced by a fresh build whose
        // dep_sig also references the canonical).
        let mut evicted = 0usize;
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
        // only one is evicted, the previous `map.retain` walked the
        // other canonical shards with `Arc::ptr_eq(registered, sig)`
        // and removed BOTH entries' registrations because they
        // pointed to the same Arc. The surviving entry then had no
        // reverse-index registration for its legacy canonicals and a
        // later `invalidate_canonical` of those canonicals would
        // miss it, leaving stale warm data. Codex round-3 P2.
        let mut evicted_entries: Vec<(
            (FamilyKey, ModeSlot),
            crate::fact_signature_helpers::ReadSetSignature,
        )> = Vec::new();
        {
            let mut entries = self.entries_lock_diagnosed();
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
            // does not later return an already-removed family.
            entries.retain(|family, slots| {
                if slots.populated_count() > 0 {
                    true
                } else {
                    self.memo_budget.forget(family);
                    false
                }
            });
        }

        // For each evicted entry, walk every canonical its carrier
        // referenced (union of `legacy` + path-precise `facts` via
        // `canonical_ids()`) and remove THAT entry's `(family, slot)`
        // registration from the canonical's shard. Removal is by entry
        // identity, not by `Arc::ptr_eq` on the stored legacy
        // signature — see the comment on `evicted_entries` above for
        // the shared-Arc hazard this avoids. Walking the full
        // `canonical_ids()` set (not only `legacy.iter()`) also drains
        // the path-precise fact rail's registrations, matching what
        // `register_reverse_index` populated. Lock order respected:
        // `entries` was unlocked at the close of the previous block
        // before any shard mutex is acquired here.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        for (evicted_key, evicted_carrier) in &evicted_entries {
            for other_canonical in evicted_carrier.canonical_ids() {
                if other_canonical.as_ref() == canonical_id {
                    continue;
                }
                if let Some(shard) = self.canonical_to_entries.get(&other_canonical) {
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
                    // Remove only the registration keyed by THIS
                    // evicted entry's `(family, slot)`. Other entries
                    // sharing the same legacy `Arc<DepSignature>` keep
                    // their reverse-index registrations intact.
                    map.remove(evicted_key);
                } else {
                    // Shard absent: account for the canonical-shard
                    // probe as one observed acquisition with zero wait.
                    crate::host_manage::record_family_map_lock_acquisition(
                        std::time::Duration::ZERO,
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

    /// Clear every warm memo entry. Used on project-generation bumps
    /// (`tsconfig` changes, active-TS-SDK swaps, workspace-folder
    /// changes). Returns the number of slots cleared (summed across
    /// every family).
    ///
    /// ## `SemanticNodeId` reuse contract
    ///
    /// The final step reuses the node arena's id space ([`NodeArena::reset`]
    /// allocates fresh ids from 0). That is sound ONLY because every
    /// structure that stores or is keyed by a [`SemanticNodeId`] is
    /// cleared first, in this method, before the arena reset. The
    /// complete set of id-holding structures, all cleared below:
    ///
    /// - `entries` — the family memo. `FamilyKey` embeds `SemanticNodeId`s
    ///   (`ProjectMember.base`, `Conditional.*`, `Instantiate.args`, …)
    ///   and `MemoEntry.result` is a `QueryResult<SemanticNodeId>`.
    /// - `inflight` — in-flight admission table. `SemanticQueryKey` keys
    ///   embed `SemanticNodeId`s for node-keyed variants. Entries are
    ///   aborted (so any cooperative joiner wakes and re-enters dispatch)
    ///   then the table is drained.
    /// - `named_type_index` — `DashMap<HostResolvedNamedTypeKey,
    ///   SemanticNodeId>`; the value is an arena id.
    /// - `relation_memo` — `DashMap<(SemanticNodeId, SemanticNodeId),
    ///   RelationMemoEntry>`; the key is a node-id pair.
    /// - `derivation` — the [`DerivationStore`]: `edges` is keyed by
    ///   `(SemanticNodeId, OriginEdgeKind)`, and its `signature_pool` is
    ///   cleared in the same `clear` call.
    /// - `canonical_to_entries` — the Γ.B reverse index; its inner-map
    ///   key is `(FamilyKey, ModeSlot)` and `FamilyKey` embeds
    ///   `SemanticNodeId`s.
    ///
    /// Their retention budgets (`memo_budget`, `relation_budget`,
    /// `named_type_budget`) are dropped in lockstep so no ledger retains
    /// a key whose entry is gone. If a future change adds another
    /// `SemanticNodeId`-keyed structure to this store, it MUST be cleared
    /// here too, or `arena.reset()` will reuse ids that still collide
    /// with that structure's stale keys.
    pub fn invalidate_all(&self) -> usize {
        let mut entries = self.entries_lock_diagnosed();
        let removed: usize = entries.values().map(FamilySlots::populated_count).sum();
        entries.clear();
        drop(entries);
        // Abort every in-flight admission before draining the table —
        // `SemanticQueryKey` keys embed `SemanticNodeId`s, and a
        // cooperative joiner blocked on the condvar must wake and
        // re-enter dispatch rather than join a stale, soon-to-be-invalid
        // entry. Mirrors the per-canonical abort in `invalidate_canonical`.
        {
            let mut table = self.inflight.lock();
            for inflight in table.values() {
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
            }
            table.clear();
        }
        // Drop every other `SemanticNodeId`-keyed structure (see the
        // method docs for the exhaustive list) so no stale id-keyed
        // entry survives the arena reset below.
        self.named_type_index.clear();
        self.relation_memo.clear();
        self.derivation.lock().clear();
        self.canonical_to_entries.clear();
        // Drop the retention ledgers in lockstep so no budget retains a
        // stale family / relation / named-type key.
        self.memo_budget.clear();
        self.relation_budget.clear();
        self.named_type_budget.clear();
        // The node arena's dense storage is append-only across content
        // edits; a project-generation reset is the safe point to
        // reclaim it. Every memo entry, in-flight admission, relation
        // judgement, named-type mapping, derivation edge, and reverse-
        // index entry has been cleared above, so no stored
        // `SemanticNodeId` survives — reusing the arena's id space is
        // sound here.
        self.arena.reset();
        removed
    }

    /// Insert a Vue macro resolution artifact under `key`. Interns the
    /// payload as a [`SemanticNodeData::VueMacroElements`] node in the
    /// arena and records the identity mapping in
    /// [`named_type_index`](Self::named_type_index). Subsequent reads via
    /// [`Self::get_resolved_named_type`] are refcount-only.
    pub fn insert_resolved_named_type(
        &self,
        key: HostResolvedNamedTypeKey,
        elements: Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
    ) -> SemanticNodeId {
        let node_id = self.intern_node(SemanticNodeData::VueMacroElements(elements));
        let is_new = !self.named_type_index.contains_key(&key);
        self.named_type_index.insert(key.clone(), node_id);
        // Global retention budget: a newly-keyed resolved-named-type
        // entry records an admission; the oldest entries past the cap
        // are FIFO-evicted. The key embeds the owner's content hash, so
        // each owner content version is a distinct, accumulating key.
        if is_new {
            let seq = crate::bounded_query_retention::next_retention_seq();
            for victim in self.named_type_budget.record_admission(seq, key) {
                self.named_type_index.remove(&victim);
            }
        }
        node_id
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
        let node_id = *self.named_type_index.get(key)?;
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
        self.named_type_index.get(key).map(|entry| *entry.value())
    }

    /// Drop every entry in the Vue macro resolution identity map. Invoked
    /// on project-generation bumps / per-canonical evictions — the
    /// append-only node arena keeps the interned
    /// [`SemanticNodeData::VueMacroElements`] payloads alive only as long
    /// as something else references their ids, which is fine because the
    /// identity map was the only external reachability path to them.
    pub fn clear_resolved_named_types(&self) {
        self.named_type_index.clear();
        self.named_type_budget.clear();
    }

    /// Remove every entry in the Vue macro resolution identity map whose
    /// key's `canonical_id` matches `canonical_id`. Called from
    /// [`ProjectTypeStore::evict_canonical`](crate::project_type_store::ProjectTypeStore::evict_canonical)
    /// so stale artifacts do not keep a retired file's spans alive.
    /// Returns the number of entries evicted.
    pub fn invalidate_resolved_named_types_for_canonical(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        let mut forgotten: Vec<HostResolvedNamedTypeKey> = Vec::new();
        self.named_type_index.retain(|key, _| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                forgotten.push(key.clone());
                false
            } else {
                true
            }
        });
        for key in &forgotten {
            self.named_type_budget.forget(key);
        }
        removed
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
        // that re-entry would deadlock.
        let entry = self
            .relation_memo
            .get(&(source, target))
            .map(|e| e.value().clone())?;
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
        let key = (source, target);
        let is_new = !self.relation_memo.contains_key(&key);
        self.relation_memo.insert(
            key,
            RelationMemoEntry {
                carrier,
                self_root_canonicals,
                result,
            },
        );
        // Global retention budget: a newly-keyed relation records an
        // admission; the oldest relations past the cap are FIFO-evicted.
        if is_new {
            let seq = crate::bounded_query_retention::next_retention_seq();
            for victim in self.relation_budget.record_admission(seq, key) {
                self.relation_memo.remove(&victim);
            }
        }
    }

    /// Count of relation memo entries. Useful for tests and counters.
    #[must_use]
    pub fn relation_memo_count(&self) -> usize {
        self.relation_memo.len()
    }

    /// Drop every entry in the relation memo. Invoked on
    /// project-generation bumps so warm relation judgements cannot leak
    /// across a version boundary.
    pub fn clear_relation_memo(&self) {
        self.relation_memo.clear();
        self.relation_budget.clear();
    }

    // ──────────────────────────────────────────────────────────────────
    // Derivation / origin layer (plan B2)
    // ──────────────────────────────────────────────────────────────────

    /// Record a derivation/origin edge for `result`. Builders call this
    /// whenever they produce a reusable result — the edge captures the
    /// source-set, per-edge metadata, and a snapshot of the publishing
    /// builder's active fence (`builder_fence`). The fence snapshot is
    /// interned in the store's signature pool so identical fences share
    /// one allocation.
    ///
    /// Multiple derivations of the same structural `result` produce
    /// multiple edges with the same `(result, kind)` — the layer supports
    /// this; the walker walks all edges.
    ///
    /// **Issue #11** (B-B7d's diagnosis report identified
    /// duplicate edges as 12.8%–18.7% of every origin-edge emission on
    /// the `repo_first_pass` corpus). The cooperative-admission cold-
    /// winner path in `build_project_path`'s prefix-backfill loop emits
    /// origin edges even when the prefix-backfill target is already
    /// warm in `SemanticGraphStore::entries`. Different `build_project_path`
    /// invocations that walk through the same intermediate hop emit the
    /// same `(result, kind, sources, meta, fence)` identity tuple
    /// repeatedly, inflating the ledger and the per-request audit cost.
    ///
    /// The fix dedups by edge identity at the call site: before
    /// recording into [`DerivationStore::edges`], we check whether an
    /// edge with the exact same identity tuple is already present and
    /// skip the ledger write if so. The audit-mining contract is
    /// preserved: the [`request_context::current_accumulator`] push
    /// remains unconditional so the footprint miner observes every
    /// derivation hop the production hot path would have emitted.
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
    /// reachable from `node`, regardless of kind. Outside-execute
    /// consumers (LSP hover, debug dumps, compat rendering) use this
    /// form; it never touches any active completion fence.
    #[must_use]
    pub fn origins(&self, node: SemanticNodeId) -> Vec<(OriginEdgeKind, OriginEdge)> {
        let store = self.derivation.lock();
        store.origins(node).map(|(k, e)| (k, e.clone())).collect()
    }

    /// Filtered read-only origin walk: only edges of the given kind.
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

    /// Dispatch-side origin walk: visits every edge on `node` and merges
    /// each edge's `edge_dep_signature` into the supplied
    /// [`CompletionFence`](crate::completion_fence::CompletionFence) at
    /// hop-time. Returns the visited edges so the caller can recurse over
    /// `edge.sources` itself (the transitive walk is the caller's
    /// responsibility, per).
    ///
    /// Per, **edges are the only dep-sig propagation route for
    /// builders** — there is intentionally no `publisher_of(node)` /
    /// `dep_signature_of(node)` API. Structurally interned nodes can be
    /// reached by multiple derivations with different dep-signatures;
    /// selecting a "canonical" publisher would pick an arbitrary owner
    /// and merge an incomplete fence, which is unsound.
    pub fn origins_with_fence(
        &self,
        node: SemanticNodeId,
        fence: &crate::completion_fence::CompletionFence,
    ) -> Vec<(OriginEdgeKind, OriginEdge)> {
        let store = self.derivation.lock();
        let mut visited: Vec<(OriginEdgeKind, OriginEdge)> = Vec::new();
        for (kind, edge) in store.origins(node) {
            fence.merge_signature(&edge.edge_dep_signature);
            visited.push((kind, edge.clone()));
        }
        visited
    }

    // ──────────────────────────────────────────────────────────────────
    // Telemetry — public stats snapshot (plan B2 + §7.4)
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
    /// `(result, dep_signature)`. The dep signature flows back to the
    /// caller's `CompletionFence` exactly as the slow path's warm-hit
    /// branch does, so warm-cache reuse stays bounded by dep-signature
    /// validation at the outer caller's fence-revalidation point.
    /// Same-path recursion detection is unaffected: the cold winner
    /// publishes the warm slot AFTER the build closure returns, so a
    /// populated slot cannot represent a cycle currently being built
    /// — the only path that needs same-path recursion detection is
    /// the slow path's loop, which still runs on cache miss. Joiner
    /// waits and abort-driven retries are unaffected: a populated
    /// warm slot means no joiner participation is needed.
    #[must_use = "the CacheRead carries both the resolved node id and the dep signature callers must merge into their active CompletionFence"]
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
            self.warm_publish_one(
                &key,
                &result,
                &walker_diagnostics,
                carrier,
                &self_root_canonicals,
                &inflight,
            );
            // Prefix backfill: publish each accumulated backfill
            // record AFTER the parent entry is warm.
            for backfill in pending_prefix_backfills {
                self.warm_publish_one_if_absent(
                    backfill.key,
                    QueryResult::Value(backfill.node),
                    carrier.clone(),
                    Arc::clone(&self_root_canonicals),
                );
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
    fn warm_publish_one(
        &self,
        key: &SemanticQueryKey,
        result: &QueryResult<SemanticNodeId>,
        walker_diagnostics: &Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
        read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: &Arc<[Arc<str>]>,
        inflight: &Arc<InflightEntry>,
    ) {
        let publishable = matches!(result, QueryResult::Value(_));
        if !publishable {
            return;
        }
        let (family, slot) = family_and_slot(key);
        // ResolvedNamedType bypasses the family memo entirely (§7.16) —
        // its DashMap-backed identity map is the cache.
        if matches!(family, FamilyKey::ResolvedNamedType { .. }) {
            return;
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
            // Canonical invalidation swept this slot during the cold
            // build; skip warm publish and record the sweep.
            record_cold_abort_swept(&self.stats);
            return;
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
        // Γ.B reverse-index registration. Carrier-aware: register
        // each populated slot under EVERY canonical the entry's
        // carrier references — the union of the legacy `dep_signature`
        // rail AND the path-precise `facts` rail (`Parse(...)`,
        // `ResolveImports(...)`, `RouteSurface(...)`, etc.). Lock
        // order is `entries → canonical_to_entries shards`: drop the
        // entries lock before acquiring any per-canonical mutex. See
        // `register_reverse_index`'s docstring for the carrier
        // contract — codex P2.B closes the fact-only invalidation
        // hole this widening covers.
        drop(entries);
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
        if family_was_new && !populated_slots.is_empty() {
            self.note_memo_family_admission(&family);
        }
    }

    /// Variant of [`Self::warm_publish_one`]: publish
    /// `(key, result, dep_signature)` into the warm map only when no
    /// entry already exists AND no concurrent in-flight build owns the
    /// key. No TOCTOU re-check (the caller does not own an in-flight
    /// entry). Used by the prefix-backfill path in
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
    ///
    /// **Carrier contract.** The published `MemoEntry` stores the
    /// caller-supplied [`ReadSetSignature`] verbatim. Prefix-backfill
    /// callers must pass the parent's authoritative carrier (legacy
    /// rail + path-precise traced facts captured under
    /// `install_fact_tracer`) so the backfilled entry's facts rail
    /// contains the parent's `Parse(...)` / `ResolveImports(...)` /
    /// `RouteSurface(...)` observations. Pre-fix the caller passed
    /// only a legacy `DepSignature` and this helper reconstructed
    /// facts via `fact_signature_from_fence` — that bridge drops
    /// path-precise facts, leaving a sibling-share short-circuit
    /// through the prefix unable to validate or bubble those
    /// dependencies (codex P2.C).
    pub(crate) fn warm_publish_one_if_absent(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
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
        // Carrier-aware reverse-index registration — see
        // `warm_publish_one` for the full carrier rationale.
        drop(entries);
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
        if family_was_new && !populated_slots.is_empty() {
            self.note_memo_family_admission(&family);
        }
    }

    /// Γ.B reverse-index registration helper. Shared by
    /// [`Self::warm_publish_one`] and
    /// [`Self::warm_publish_one_if_absent`]. Caller must have dropped
    /// the `entries` lock before calling per the `entries →
    /// canonical_to_entries shards` lock order.
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
    /// **Why match `MaterializeStructureDb::register_post_publish`.**
    /// Track 4 introduced the same canonical-ids() iteration for
    /// `MaterializeStructureDb` and `RefCycleResultDb` so their
    /// reverse-indexes drain on fact-only invalidation. The memo
    /// equivalent was missed in the original Block 1.I landing
    /// (codex P2.B). This helper now matches that pattern.
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
    fn note_memo_family_admission(&self, family: &FamilyKey) {
        let seq = crate::bounded_query_retention::next_retention_seq();
        let victims = self.memo_budget.record_admission(seq, family.clone());
        for victim in victims {
            self.evict_memo_family_for_budget(&victim);
        }
    }

    /// Remove one whole family chosen by the retention budget for FIFO
    /// eviction: drop it from the `entries` memo and drain its
    /// reverse-index registrations under every canonical its carrier
    /// referenced. Mirrors the per-canonical drain `invalidate_canonical`
    /// performs for one entry, but keyed by the budget's victim family.
    fn evict_memo_family_for_budget(&self, victim: &FamilyKey) {
        const ALL_SLOTS: [ModeSlot; 6] = [
            ModeSlot::Single,
            ModeSlot::Identity,
            ModeSlot::Navigate,
            ModeSlot::Shallow,
            ModeSlot::Expanded,
            ModeSlot::Skeleton,
        ];
        // Drop the family from the memo, capturing each populated
        // slot's carrier so the reverse-index registrations can be
        // drained.
        let evicted_carriers: Vec<(ModeSlot, crate::fact_signature_helpers::ReadSetSignature)> = {
            let mut entries = self.entries_lock_diagnosed();
            match entries.remove(victim) {
                Some(slots) => ALL_SLOTS
                    .iter()
                    .filter_map(|&slot| {
                        slots
                            .slot(slot)
                            .map(|entry| (slot, entry.read_set_signature.clone()))
                    })
                    .collect(),
                None => return,
            }
        };
        // Lock order: `entries` released above before any
        // `canonical_to_entries` shard mutex is acquired.
        for (slot, carrier) in &evicted_carriers {
            for canonical in carrier.canonical_ids() {
                if let Some(shard) = self.canonical_to_entries.get(&canonical) {
                    shard.value().lock().remove(&(victim.clone(), *slot));
                }
            }
        }
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
    /// (codex P2.C) to assert that a backfilled prefix entry's
    /// `facts` rail contains the parent's traced `Parse(...)` fact
    /// rather than a fence-only reconstruction.
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
        let populated_slots = entries
            .entry(family.clone())
            .or_default()
            .publish(slot, entry);
        drop(entries);
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
        );
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
            // structure) before resetting the arena id space.
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
