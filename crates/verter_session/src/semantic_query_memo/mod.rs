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
    dep_signature_references_canonical, family_and_slot, FamilyKey, FamilySlots, MemoEntry,
    ModeSlot,
};
pub(crate) use inflight::FORCE_COLD_ABORT_SWEEP;
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
    /// Relation-engine memo. Added in Phase D §5.4
    /// WIP-S. Maps `(source, target)` semantic-node pairs to the tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) plus the
    /// dep-signature used for warm-hit revalidation. Separate from the
    /// family memo because relation identity is pairwise, not single-node.
    relation_memo: DashMap<
        (SemanticNodeId, SemanticNodeId),
        (DepSignature, crate::semantic_query::RelationResult),
    >,
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
                let dep_signature = format!("{:?}", slot.dep_signature);
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
        let mut evicted_dep_sigs: Vec<DepSignature> = Vec::new();
        {
            let mut entries = self.entries_lock_diagnosed();
            for ((family, slot), registered_sig) in &drained {
                let Some(slots) = entries.get_mut(family) else {
                    continue;
                };
                let Some(current_entry) = slots.slot(*slot) else {
                    continue;
                };
                let drop = Arc::ptr_eq(&current_entry.dep_signature, registered_sig)
                    || dep_signature_references_canonical(
                        &current_entry.dep_signature,
                        canonical_id,
                    );
                if drop {
                    let entry_sig = Arc::clone(&current_entry.dep_signature);
                    *slots.slot_mut(*slot) = None;
                    evicted += 1;
                    evicted_dep_sigs.push(entry_sig);
                }
            }
            entries.retain(|_, slots| slots.populated_count() > 0);
        }

        // For each evicted entry's
        // dep_signature, walk every other canonical it referenced and
        // drop the matching `(family, slot)` registration if it still
        // ptr_eq-matches our dep_signature. Lock order respected:
        // `entries` was unlocked at the close of before any
        // shard mutex is acquired here.
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        for entry_sig in &evicted_dep_sigs {
            for (other_canonical, _) in entry_sig.iter() {
                if other_canonical.as_ref() == canonical_id {
                    continue;
                }
                if let Some(shard) = self.canonical_to_entries.get(other_canonical) {
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
                    map.retain(|_, registered_sig| {
                        // Keep entries whose registered_sig is a
                        // different `Arc` (fresh build) — only drop
                        // the exact registration tied to this
                        // evicted entry.
                        !Arc::ptr_eq(registered_sig, entry_sig)
                    });
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
    pub fn invalidate_all(&self) -> usize {
        let mut entries = self.entries_lock_diagnosed();
        let removed: usize = entries.values().map(FamilySlots::populated_count).sum();
        entries.clear();
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
        self.named_type_index.insert(key, node_id);
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
    }

    /// Remove every entry in the Vue macro resolution identity map whose
    /// key's `canonical_id` matches `canonical_id`. Called from
    /// [`ProjectTypeStore::evict_canonical`](crate::project_type_store::ProjectTypeStore::evict_canonical)
    /// so stale artifacts do not keep a retired file's spans alive.
    /// Returns the number of entries evicted.
    pub fn invalidate_resolved_named_types_for_canonical(&self, canonical_id: &str) -> usize {
        let mut removed = 0usize;
        self.named_type_index.retain(|key, _| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                false
            } else {
                true
            }
        });
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

    /// Warm-hit read of a cached relation judgement for `(source, target)`.
    /// Returns the tri-state [`RelationResult`](crate::semantic_query::RelationResult)
    /// plus the `DepSignature` recorded at publish so warm hits can
    /// revalidate under content changes via
    /// [`HostFenceValidator`](crate::resolver_core::host_fence_validator::HostFenceValidator).
    #[must_use]
    pub fn get_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(DepSignature, crate::semantic_query::RelationResult)> {
        self.relation_memo
            .get(&(source, target))
            .map(|entry| entry.value().clone())
    }

    /// Publish a relation judgement for `(source, target)`. Writes to the
    /// dedicated relation memo DashMap, separate from the family memo so
    /// pairwise identity does not inflate the single-node keyspace.
    pub fn insert_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        fence: DepSignature,
        result: crate::semantic_query::RelationResult,
    ) {
        self.relation_memo.insert((source, target), (fence, result));
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

    /// Warm-lookup a key. Returns the memoized result + its recorded
    /// dependency signature when the requested `(family, mode_slot)` is
    /// populated. Backfill from broader-mode computes lands in narrower
    /// slots eagerly at publish time, so a `Navigate` lookup after a
    /// successful `Expanded` build hits the (backfilled) `Navigate` slot
    /// directly without any per-call satisfaction logic here.
    #[must_use]
    pub fn get(&self, key: &SemanticQueryKey) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        let result = entries.get(&family).and_then(|slots| {
            slots.slot(slot).cloned().map(|entry| CacheRead {
                value: entry.result,
                dep_signature: entry.dep_signature,
                walker_diagnostics: entry.walker_diagnostics,
                cache_suppress: false,
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

    /// Per-key result for the BFS bridge's batch dispatch (D103). Each
    /// frontier handle is resolved into either a node-id (success) or a
    /// typed reason describing why expansion could not proceed. Per-key
    /// errors are returned, NOT panic'd (D41 invariant: one batch entry → N
    /// keys → K admissions).
    ///
    /// Lookups happen via warm `get(key)` only — `execute_cooperative_batch`
    /// is a non-admission probe; cold builds stay the responsibility of the
    /// per-query cooperative path.
    pub fn execute_cooperative_batch(
        &self,
        keys: &[crate::semantic_query::SemanticQueryKey],
    ) -> Vec<Result<SemanticNodeId, BatchExpandError>> {
        keys.iter()
            .map(|key| {
                if let Some(hit) = self.get(key) {
                    match hit.value {
                        QueryResult::Value(node) => Ok(node),
                        QueryResult::Recursive(node) => Ok(node),
                        QueryResult::Error(_) => Err(BatchExpandError::EvictedNode),
                    }
                } else {
                    // Cold: from the BFS bridge's perspective, an unmaterialized
                    // key is treated as evicted; the bridge will surface a
                    // typed StaleAtFrontier envelope and the caller can decide
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
    pub fn execute_cooperative<F, R, O>(
        &self,
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
        // acquisition checks the slot; on hit it returns immediately
        // bypassing the slow path's `entries_lock_diagnosed`
        // `Instant::now`/capture-token wait+hold timing, the
        // in-flight table mutex, the second `self.get(&key)`
        // invocation inside the loop's step 1, the same-path
        // recursion test, and the joiner-condvar admission entry
        // path. On miss the lock is released and execution falls
        // through to the cooperative slow path that owns same-path
        // recursion, in-flight admission, and cold-build publish.
        if let Some(hit) = self.try_warm_hit_fast_path(&key) {
            return hit;
        }

        // Slow path — cooperative-admission flow. Handles same-path
        // recursion, joiner-condvar waits, cold-build publish.
        self.execute_cooperative_slow(key, recursion_sentinel, build)
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
    /// step 1 `self.get(&key)` returns `None`, preserving slow-path
    /// counter discipline.
    ///
    /// **Lock discipline.** Acquires `self.entries` directly (no
    /// `entries_lock_diagnosed` Instant::now/capture-token wrapping).
    /// Holds the lock only for the slot read + clone, drops before
    /// instrumentation. parking_lot::Mutex is uncontended on the
    /// warm-read hot path.
    #[inline]
    fn try_warm_hit_fast_path(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<CacheRead<QueryResult<SemanticNodeId>>> {
        let (family, slot) = family_and_slot(key);
        let hit = {
            // Single non-diagnosed lock acquisition. The
            // `entries_lock_diagnosed` wrapper that adds Instant::now
            // wait+hold timing under capture-token is intentionally
            // bypassed here because the warm-hit hot path runs
            // hundreds of thousands of times per request and the
            // wrapper's per-acquisition cost dominates the warm-hit
            // wall-clock.
            let entries = self.entries.lock();
            entries.get(&family).and_then(|slots| {
                slots.slot(slot).cloned().map(|entry| CacheRead {
                    value: entry.result,
                    dep_signature: entry.dep_signature,
                    walker_diagnostics: entry.walker_diagnostics,
                    cache_suppress: false,
                })
            })
        };

        let hit = hit?;

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
        // warm call (the slow path's old `let initial_hit = self.get(&key)`
        // observation plus the loop's step-1 `self.get(&key)` call
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
            //    check and now (or on retry after an abort sweep).
            if let Some(hit) = self.get(&key) {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                if let Some(ctx) = verter_scheduler::request_context::current_context() {
                    ctx.0
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
                let walker_diagnostics = state
                    .walker_diagnostics
                    .clone()
                    .unwrap_or_else(|| std::sync::Arc::from([]));
                if let Some(prov) = self.provenance.as_ref() {
                    prov.execute_cooperative_joiner_path
                        .fetch_add(1, Ordering::Relaxed);
                }
                return CacheRead {
                    value: result,
                    dep_signature,
                    walker_diagnostics,
                    cache_suppress: false,
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
        if !cache_suppress {
            self.warm_publish_one(
                &key,
                &result,
                &dep_signature,
                &walker_diagnostics,
                &inflight,
            );
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
        {
            let mut table = self.inflight.lock();
            table.remove(&key);
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
    /// Test-only forcing flag [`FORCE_COLD_ABORT_SWEEP`] simulates a
    /// concurrent sweep without racing a real invalidation window.
    fn warm_publish_one(
        &self,
        key: &SemanticQueryKey,
        result: &QueryResult<SemanticNodeId>,
        dep_signature: &DepSignature,
        walker_diagnostics: &Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
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
        let fact_dep_signature =
            crate::component_meta_materialize::fact_signature_from_fence(dep_signature.as_ref());
        let entry = MemoEntry {
            result: result.clone(),
            dep_signature: dep_signature.clone(),
            fact_dep_signature,
            walker_diagnostics: Arc::clone(walker_diagnostics),
        };
        let mut entries = self.entries_lock_diagnosed();
        // Test forcing: simulate a concurrent sweep that aborted
        // this in-flight entry just before the TOCTOU re-check.
        // Deterministically drives the `cold_aborts_swept` counter
        // for counter-helper coverage tests without needing a racy
        // real invalidation. The flag default `false` makes this
        // branch a single relaxed atomic load on the cold-build
        // path under normal traffic.
        if FORCE_COLD_ABORT_SWEEP.load(Ordering::Relaxed) {
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
        // Γ.B reverse-index registration. For each populated
        // slot (the primary plus any backfilled narrower slots),
        // register the (family, slot) → dep_signature mapping under
        // every canonical the dep_signature references. Lock order is
        // `entries → canonical_to_entries shards`: drop the entries lock
        // before acquiring any per-canonical mutex.
        drop(entries);
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            dep_signature,
        );
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
    /// 3. `self.get(&key).is_some()` — slot is already warm.
    /// 4. The in-flight table contains `key` — a cold winner is
    ///    currently building this exact key; let it publish.
    pub(crate) fn warm_publish_one_if_absent(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        dep_signature: DepSignature,
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
        if self.get(&key).is_some() {
            return;
        }
        if self.inflight.lock().contains_key(&key) {
            return;
        }
        let fact_dep_signature =
            crate::component_meta_materialize::fact_signature_from_fence(dep_signature.as_ref());
        let entry = MemoEntry {
            result,
            dep_signature: dep_signature.clone(),
            fact_dep_signature,
            walker_diagnostics: Arc::from([]),
        };
        let mut entries = self.entries_lock_diagnosed();
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
        drop(entries);
        Self::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &dep_signature,
        );
    }

    /// Γ.B reverse-index registration helper. Shared by
    /// [`Self::warm_publish_one`] and
    /// [`Self::warm_publish_one_if_absent`]. Caller must have dropped
    /// the `entries` lock before calling per the `entries →
    /// canonical_to_entries shards` lock order.
    fn register_reverse_index(
        canonical_to_entries: &CanonicalToEntries,
        family: &FamilyKey,
        populated_slots: &[ModeSlot],
        dep_signature: &DepSignature,
    ) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        for populated in populated_slots {
            for (canonical, _) in dep_signature.iter() {
                let shard = canonical_to_entries
                    .entry(Arc::clone(canonical))
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
                map.insert((family.clone(), *populated), Arc::clone(dep_signature));
            }
        }
    }

    /// Path-prefix backfill API. Publishes a
    /// `(key, value, dep_signature)` triple via the same warm-publish
    /// helper that [`Self::execute_cooperative`] uses (extracted as
    /// [`Self::warm_publish_one_if_absent`]), gated by the "absent
    /// only" check. Never blocks, never starts compute, never
    /// participates in the in-flight admission flow.
    ///
    /// **PRECONDITION:** `key.mode == ProjectionMode::Navigate`. Phase
    /// 1B only backfills intermediate path hops, which by the
    /// path-precise rule (CLAUDE.md "Macro Type Traversal Rule") must
    /// be Navigate-mode entries. Calling this with any other mode is a
    /// programming error and trips a debug assertion.
    pub(crate) fn publish_warm_if_absent(
        &self,
        key: SemanticQueryKey,
        value: SemanticNodeId,
        dep_signature: DepSignature,
    ) {
        debug_assert!(
            matches!(
                &key,
                SemanticQueryKey::ProjectPath {
                    mode: crate::semantic_query::ProjectionMode::Navigate,
                    ..
                }
            ),
            "publish_warm_if_absent only takes ProjectPath{{Navigate}} keys (path-precise rule)"
        );
        self.warm_publish_one_if_absent(key, QueryResult::Value(value), dep_signature);
    }
}

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

    /// Public test driver: set the `FORCE_COLD_ABORT_SWEEP` flag for
    /// the duration of the returned guard so the next
    /// `execute_cooperative` cold-build deterministically hits the
    /// TOCTOU abort path. Used by integration tests in
    /// `crates/verter_session/tests/` that drive the counter-helper
    /// plumbing.
    ///
    /// The guard restores the flag to `false` on drop. Tests must
    /// hold the guard for the duration of the `execute_cooperative`
    /// call.
    #[doc(hidden)]
    #[must_use]
    pub fn test_force_cold_abort_sweep() -> TestForceColdAbortGuard {
        inflight::FORCE_COLD_ABORT_SWEEP.store(true, Ordering::SeqCst);
        TestForceColdAbortGuard { _private: () }
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
            let _ = self.invalidate_all();
            self.clear_resolved_named_types();
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
/// [`SemanticGraphStore::test_force_cold_abort_sweep`]. Restores the
/// `FORCE_COLD_ABORT_SWEEP` flag to `false` on drop so a panicking
/// test does not leak the flag onto sibling tests sharing the same
/// process.
#[doc(hidden)]
pub struct TestForceColdAbortGuard {
    _private: (),
}

impl Drop for TestForceColdAbortGuard {
    fn drop(&mut self) {
        inflight::FORCE_COLD_ABORT_SWEEP.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
