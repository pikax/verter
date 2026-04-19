//! Host-owned semantic-query memo table (Phase 2.2 core).
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

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

use crate::semantic_query::{
    CacheRead, DepSignature, HostResolvedNamedTypeKey, IndexKey, MapperKey, OriginEdge,
    OriginEdgeKind, PathSegment, ProjectionMode, QueryError, QueryResult, ResolveDeclKey,
    SemanticGraphRead, SemanticGraphStats, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    ValueRootKey,
};

// ──────────────────────────────────────────────────────────────────────────
// Node arena — append-only, stable ids
// ──────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct NodeArena {
    nodes: Mutex<Vec<Arc<SemanticNodeData>>>,
}

impl NodeArena {
    fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        let mut nodes = self.nodes.lock();
        let id = SemanticNodeId(nodes.len() as u64);
        nodes.push(Arc::new(data));
        id
    }

    fn get(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        let nodes = self.nodes.lock();
        nodes.get(id.0 as usize).cloned()
    }

    fn len(&self) -> usize {
        self.nodes.lock().len()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// In-flight admission — per-entry Mutex + Condvar pair
// ──────────────────────────────────────────────────────────────────────────

/// In-flight admission state for one cold build.
///
/// The inner mutex guards `state` exclusively; `ready` is signalled when
/// the winner publishes. Joiners wait on `ready` via `wait_while`, so they
/// do not busy-retry.
struct InflightEntry {
    state: Mutex<InflightState>,
    ready: Condvar,
}

#[derive(Default)]
struct InflightState {
    /// `None` while building; `Some` after the winner publishes.
    completed: Option<QueryResult<SemanticNodeId>>,
    /// Dep signature the winner observed.
    dep_signature: Option<DepSignature>,
    /// `true` once some thread owns the build. Subsequent threads wait on
    /// `ready` rather than trying to own it themselves.
    claimed: bool,
}

impl InflightEntry {
    fn new() -> Self {
        Self {
            state: Mutex::new(InflightState::default()),
            ready: Condvar::new(),
        }
    }
}

/// RAII guard that pops a key off [`IN_FLIGHT_ON_THIS_THREAD`] when dropped.
///
/// Ensures the recursion stack stays consistent even if the cold build
/// panics — otherwise a caught panic or unwind could leave a key on the
/// stack and future unrelated queries for that key from the same thread
/// would be misclassified as same-path recursion.
struct RecursionStackGuard {
    key: Option<SemanticQueryKey>,
}

impl RecursionStackGuard {
    fn push(key: SemanticQueryKey) -> Self {
        IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow_mut().push(key.clone()));
        Self { key: Some(key) }
    }
}

impl Drop for RecursionStackGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            IN_FLIGHT_ON_THIS_THREAD.with(|slot| {
                let mut v = slot.borrow_mut();
                if let Some(pos) = v.iter().rposition(|k| k == &key) {
                    v.remove(pos);
                }
            });
        }
    }
}

/// RAII guard that fails the in-flight entry if the cold build panics.
///
/// Without this guard, a panic inside the winner's build closure would
/// leave `state.claimed == true` with `state.completed == None`. Any
/// subsequent caller for the same key would block on the condvar forever
/// because no publish ever wakes it. The guard detects the abnormal drop
/// via a `completed` flag, marks the entry with an error sentinel, wakes
/// joiners, and removes the entry from the in-flight table so fresh
/// callers start a new build.
struct InflightPanicGuard<'a> {
    inflight: Arc<InflightEntry>,
    inflight_table: &'a Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
    key: SemanticQueryKey,
    finished: bool,
}

impl<'a> InflightPanicGuard<'a> {
    fn new(
        inflight: Arc<InflightEntry>,
        inflight_table: &'a Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
        key: SemanticQueryKey,
    ) -> Self {
        Self {
            inflight,
            inflight_table,
            key,
            finished: false,
        }
    }

    fn mark_finished(&mut self) {
        self.finished = true;
    }
}

impl<'a> Drop for InflightPanicGuard<'a> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Panic / early-return path — mark the entry completed with an
        // error sentinel so joiners can wake and fail fresh rather than
        // wait forever on a condvar that will never be signalled.
        {
            let mut state = self.inflight.state.lock();
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "cold build aborted (panic or early return)",
                ))));
                state.dep_signature = Some(empty_signature());
            }
        }
        self.inflight.ready.notify_all();
        let mut table = self.inflight_table.lock();
        table.remove(&self.key);
    }
}

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
    /// Family-keyed warm memo (plan §2 cache topology + B1b).
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
    /// gives per-`(family, mode_slot)` in-flight authority (plan §7.15) —
    /// concurrent `Navigate` and `Expanded` builds on the same family run
    /// as two independent in-flight entries.
    inflight: Mutex<FxHashMap<SemanticQueryKey, Arc<InflightEntry>>>,
    /// Identity map for Vue macro resolution artifacts keyed by
    /// [`HostResolvedNamedTypeKey`]. See the struct-level docs for the
    /// read-path shape. Per plan §7.16, `SemanticQueryKey::ResolvedNamedType`
    /// bypasses the family memo entirely — this `DashMap` is the cache,
    /// and `execute_cooperative` short-circuits straight to the build
    /// closure for that variant.
    named_type_index: DashMap<HostResolvedNamedTypeKey, SemanticNodeId>,
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

#[derive(Clone)]
struct MemoEntry {
    result: QueryResult<SemanticNodeId>,
    dep_signature: DepSignature,
}

// ──────────────────────────────────────────────────────────────────────────
// Family memo — mode-erased keys + per-mode slots (plan §2 + B1b + §7.15)
// ──────────────────────────────────────────────────────────────────────────

/// Mode-erased identity for one [`SemanticQueryKey`] family.
///
/// Two semantic queries that mean the same thing apart from `mode` produce
/// the same [`FamilyKey`]; their per-mode results live in distinct slots
/// inside [`FamilySlots`]. Variants without a `mode` field (everything
/// except [`SemanticQueryKey::ProjectMember`] /
/// [`SemanticQueryKey::IndexedAccess`] / [`SemanticQueryKey::ProjectPath`])
/// use only the `single` slot, exactly mirroring the pre-B1b behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FamilyKey {
    ResolveDecl(ResolveDeclKey),
    Instantiate {
        base: SemanticNodeId,
        args: Arc<[SemanticNodeId]>,
    },
    ProjectMember {
        base: SemanticNodeId,
        member: Arc<str>,
    },
    IndexedAccess {
        base: SemanticNodeId,
        index: IndexKey,
    },
    KeyOf {
        base: SemanticNodeId,
    },
    MappedType {
        source: SemanticNodeId,
        mapper: MapperKey,
    },
    Conditional {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    },
    TypeOf {
        value_root: ValueRootKey,
    },
    NormalizeUnion {
        members: Arc<[SemanticNodeId]>,
    },
    NormalizeIntersection {
        members: Arc<[SemanticNodeId]>,
    },
    ProjectPath {
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
    },
    /// Included for completeness so `family_and_slot` is total, but
    /// [`SemanticQueryKey::ResolvedNamedType`] bypasses the family memo at
    /// admission and never lands in the warm map (plan §7.16).
    ResolvedNamedType {
        key: Arc<HostResolvedNamedTypeKey>,
    },
}

/// Per-family slot selector. For non-mode variants only `Single` is used;
/// for mode-bearing variants one of `Identity` / `Navigate` / `Shallow` /
/// `Expanded` is selected from the key's `ProjectionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModeSlot {
    Single,
    Identity,
    Navigate,
    Shallow,
    Expanded,
}

/// Per-family per-slot warm storage. Each slot independently holds an
/// optional [`MemoEntry`]. Backfill on completion fills empty narrower
/// slots from a successful broader compute (see [`FamilySlots::publish`]).
#[derive(Default, Clone)]
struct FamilySlots {
    single: Option<MemoEntry>,
    identity: Option<MemoEntry>,
    navigate: Option<MemoEntry>,
    shallow: Option<MemoEntry>,
    expanded: Option<MemoEntry>,
}

impl FamilySlots {
    fn slot(&self, slot: ModeSlot) -> Option<&MemoEntry> {
        match slot {
            ModeSlot::Single => self.single.as_ref(),
            ModeSlot::Identity => self.identity.as_ref(),
            ModeSlot::Navigate => self.navigate.as_ref(),
            ModeSlot::Shallow => self.shallow.as_ref(),
            ModeSlot::Expanded => self.expanded.as_ref(),
        }
    }

    fn slot_mut(&mut self, slot: ModeSlot) -> &mut Option<MemoEntry> {
        match slot {
            ModeSlot::Single => &mut self.single,
            ModeSlot::Identity => &mut self.identity,
            ModeSlot::Navigate => &mut self.navigate,
            ModeSlot::Shallow => &mut self.shallow,
            ModeSlot::Expanded => &mut self.expanded,
        }
    }

    /// Publish `entry` to `slot` and backfill every narrower slot whose
    /// cell is empty. The narrower slots store the same `Arc`-shared
    /// [`MemoEntry`] (same result + same dep-signature) — this is the
    /// conservative "broader satisfies narrower" rule from plan §7.11; a
    /// dep-signature tightening pass against the actual narrower read-set
    /// is permitted follow-up work tracked in §1.4.
    fn publish(&mut self, slot: ModeSlot, entry: MemoEntry) {
        *self.slot_mut(slot) = Some(entry.clone());
        for narrower in backfill_targets(slot) {
            let cell = self.slot_mut(*narrower);
            if cell.is_none() {
                *cell = Some(entry.clone());
            }
        }
    }

    fn populated_count(&self) -> usize {
        let slots = [
            &self.single,
            &self.identity,
            &self.navigate,
            &self.shallow,
            &self.expanded,
        ];
        slots.iter().filter(|s| s.is_some()).count()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Derivation / origin layer (plan B2 + Derivation/Origin Layer Contract)
// ──────────────────────────────────────────────────────────────────────────

/// Sibling edge store for the derivation/origin layer. Co-owned by
/// [`SemanticGraphStore`] but conceptually a separate graph: edges are
/// keyed by `(result_node, kind)` and hold the source-set + per-edge
/// metadata + a snapshot of the publishing builder's active fence.
///
/// Edge dep-signatures are interned in `signature_pool` so builders that
/// emit dozens of edges with identical fences share one `Arc` allocation
/// (target: origin-store memory stays within 2× the semantic-node-arena
/// memory on the F3 corpus).
///
/// Multiple derivations of the same structural result produce multiple
/// edges with the same `(result, kind)` key — the layer supports this
/// by storing a `Vec<OriginEdge>` per key. Walkers walk the full vector;
/// dedup is the walker's responsibility (it usually is not — different
/// derivations carry different dep-sigs).
#[derive(Default)]
struct DerivationStore {
    edges: FxHashMap<(SemanticNodeId, OriginEdgeKind), Vec<OriginEdge>>,
    signature_pool: FxHashMap<DepSignature, Arc<DepSignature>>,
}

impl DerivationStore {
    fn intern_signature(&mut self, sig: DepSignature) -> Arc<DepSignature> {
        if let Some(existing) = self.signature_pool.get(&sig) {
            return Arc::clone(existing);
        }
        let arc = Arc::new(sig.clone());
        self.signature_pool.insert(sig, Arc::clone(&arc));
        arc
    }

    fn record(&mut self, result: SemanticNodeId, kind: OriginEdgeKind, edge: OriginEdge) {
        self.edges.entry((result, kind)).or_default().push(edge);
    }

    fn origins_of_kind(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
    ) -> impl Iterator<Item = &OriginEdge> {
        self.edges
            .get(&(result, kind))
            .into_iter()
            .flat_map(|v| v.iter())
    }

    fn origins(
        &self,
        result: SemanticNodeId,
    ) -> impl Iterator<Item = (OriginEdgeKind, &OriginEdge)> {
        self.edges
            .iter()
            .filter_map(move |((r, kind), edges)| {
                if *r == result {
                    Some(edges.iter().map(move |e| (*kind, e)))
                } else {
                    None
                }
            })
            .flatten()
    }

    fn edge_count(&self) -> usize {
        self.edges.values().map(Vec::len).sum()
    }

    fn max_edges_per_node(&self) -> usize {
        let mut by_node: FxHashMap<SemanticNodeId, usize> = FxHashMap::default();
        for ((node, _kind), edges) in &self.edges {
            *by_node.entry(*node).or_insert(0) += edges.len();
        }
        by_node.values().copied().max().unwrap_or(0)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Telemetry — atomic counters (plan B2 + §7.4)
// ──────────────────────────────────────────────────────────────────────────

/// Lock-free counter set updated on the hot path. Read into the immutable
/// [`SemanticGraphStats`] snapshot via [`SemanticGraphStore::stats_snapshot`].
#[derive(Default)]
struct AtomicSemanticGraphStats {
    hits: AtomicU64,
    misses: AtomicU64,
    same_path_sentinel_returns: AtomicU64,
    in_flight_current: AtomicU32,
    in_flight_peak: AtomicU32,
    waits_ms: AtomicU64,
    origin_edges_emitted: AtomicU64,
    instantiate_count: AtomicU64,
    conditional_decided_count: AtomicU64,
    conditional_deferred_count: AtomicU64,
    branch_selections_true: AtomicU64,
    branch_selections_false: AtomicU64,
    budget_fallback_count: AtomicU64,
    max_path_length: AtomicU32,
    max_projection_depth: AtomicU32,
}

impl AtomicSemanticGraphStats {
    fn record_in_flight_enter(&self) {
        let now = self.in_flight_current.fetch_add(1, Ordering::Relaxed) + 1;
        // Compare-exchange peak forward; relaxed ordering is fine because
        // the peak is purely observational.
        let mut peak = self.in_flight_peak.load(Ordering::Relaxed);
        while now > peak {
            match self.in_flight_peak.compare_exchange_weak(
                peak,
                now,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    fn record_in_flight_exit(&self) {
        self.in_flight_current.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Slot fan-out for backfill. `Expanded` satisfies `Shallow` / `Navigate` /
/// `Identity`; `Shallow` satisfies `Navigate` / `Identity`; `Navigate`
/// satisfies `Identity`. `Identity` and `Single` backfill nothing.
fn backfill_targets(slot: ModeSlot) -> &'static [ModeSlot] {
    match slot {
        ModeSlot::Single => &[],
        ModeSlot::Identity => &[],
        ModeSlot::Navigate => &[ModeSlot::Identity],
        ModeSlot::Shallow => &[ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Expanded => &[ModeSlot::Shallow, ModeSlot::Navigate, ModeSlot::Identity],
    }
}

fn mode_to_slot(mode: ProjectionMode) -> ModeSlot {
    match mode {
        ProjectionMode::Identity => ModeSlot::Identity,
        ProjectionMode::Navigate => ModeSlot::Navigate,
        ProjectionMode::Shallow => ModeSlot::Shallow,
        ProjectionMode::Expanded => ModeSlot::Expanded,
    }
}

/// Project a [`SemanticQueryKey`] onto its `(family, slot)` pair. For
/// mode-bearing variants the mode is stripped into the slot; for everything
/// else the slot is `Single`.
fn family_and_slot(key: &SemanticQueryKey) -> (FamilyKey, ModeSlot) {
    match key {
        SemanticQueryKey::ResolveDecl(decl) => {
            (FamilyKey::ResolveDecl(decl.clone()), ModeSlot::Single)
        }
        SemanticQueryKey::Instantiate { base, args } => (
            FamilyKey::Instantiate {
                base: *base,
                args: Arc::clone(args),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::ProjectMember { base, member, mode } => (
            FamilyKey::ProjectMember {
                base: *base,
                member: Arc::clone(member),
            },
            mode_to_slot(*mode),
        ),
        SemanticQueryKey::IndexedAccess { base, index, mode } => (
            FamilyKey::IndexedAccess {
                base: *base,
                index: index.clone(),
            },
            mode_to_slot(*mode),
        ),
        SemanticQueryKey::KeyOf { base } => (FamilyKey::KeyOf { base: *base }, ModeSlot::Single),
        SemanticQueryKey::MappedType { source, mapper } => (
            FamilyKey::MappedType {
                source: *source,
                mapper: mapper.clone(),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => (
            FamilyKey::Conditional {
                check: *check,
                extends: *extends,
                true_branch: *true_branch,
                false_branch: *false_branch,
                distributive: *distributive,
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::TypeOf { value_root } => (
            FamilyKey::TypeOf {
                value_root: value_root.clone(),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::NormalizeUnion { members } => (
            FamilyKey::NormalizeUnion {
                members: Arc::clone(members),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::NormalizeIntersection { members } => (
            FamilyKey::NormalizeIntersection {
                members: Arc::clone(members),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::ProjectPath { base, path, mode } => (
            FamilyKey::ProjectPath {
                base: *base,
                path: Arc::clone(path),
            },
            mode_to_slot(*mode),
        ),
        SemanticQueryKey::ResolvedNamedType { key } => (
            FamilyKey::ResolvedNamedType {
                key: Arc::clone(key),
            },
            ModeSlot::Single,
        ),
    }
}

/// Returns `true` iff `family` is rooted in a scope that names `canonical_id`.
/// Mirrors the conservative pre-B1b helper but operates on the mode-erased
/// family identity. Replaced in B3 by a dep-signature sweep.
fn family_references_canonical(family: &FamilyKey, canonical_id: &str) -> bool {
    match family {
        FamilyKey::ResolveDecl(decl_key) => decl_key.scope.canonical_id.as_ref() == canonical_id,
        FamilyKey::TypeOf { value_root } => value_root.scope.canonical_id.as_ref() == canonical_id,
        _ => false,
    }
}

thread_local! {
    /// Per-thread set of query keys currently being executed. Used to
    /// detect same-path recursion so callers return a sentinel instead of
    /// self-awaiting.
    static IN_FLIGHT_ON_THIS_THREAD: RefCell<Vec<SemanticQueryKey>> =
        const { RefCell::new(Vec::new()) };
}

impl SemanticGraphStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a new immutable [`SemanticNodeData`] and return its stable id.
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.arena.push(data)
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

    /// Invalidate every warm memo entry whose [`SemanticQueryKey`]
    /// references `canonical_id` in its scope. Called on file-content
    /// changes so subsequent queries for `ResolveDecl(a.ts::Foo)` recompute
    /// under the new file version instead of returning a stale node.
    ///
    /// Semantic node ids remain stable (the arena is append-only); only
    /// memo entries are cleared. Returns the number of entries evicted.
    ///
    /// Does not touch in-flight admission: an in-flight build for the
    /// stale canonical will still complete and publish; the next query
    /// after this call re-runs the build under the new version. This is
    /// acceptable because the plan's contract says "semantic memo caches
    /// are rooted in versioned semantic identities, so a change to `C.ts`
    /// creates new semantic nodes under `C@new_hash` while unrelated files
    /// stay warm" — the new semantic node is produced by the re-run, not
    /// by mutating the existing entry.
    pub fn invalidate_canonical(&self, canonical_id: &str) -> usize {
        let mut entries = self.entries.lock();
        let mut evicted = 0usize;
        entries.retain(|family, slots| {
            if family_references_canonical(family, canonical_id) {
                evicted += slots.populated_count();
                false
            } else {
                true
            }
        });
        evicted
    }

    /// Clear every warm memo entry. Used on project-generation bumps
    /// (`tsconfig` changes, active-TS-SDK swaps, workspace-folder changes)
    /// per plan § A0. Returns the number of slots cleared (summed across
    /// every family).
    pub fn invalidate_all(&self) -> usize {
        let mut entries = self.entries.lock();
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
    /// this; the walker walks all edges (plan §2 + §7.16).
    pub fn record_origin_edge(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        sources: Arc<[SemanticNodeId]>,
        meta: crate::semantic_query::OriginMeta,
        builder_fence: DepSignature,
    ) {
        let mut store = self.derivation.lock();
        let edge_dep_signature = store.intern_signature(builder_fence);
        store.record(
            result,
            kind,
            OriginEdge {
                sources,
                meta,
                edge_dep_signature,
            },
        );
        self.stats
            .origin_edges_emitted
            .fetch_add(1, Ordering::Relaxed);
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
    /// `node`. Useful for callers that want to walk without materialising
    /// the full Vec.
    pub fn walk_origin_chain<F>(&self, node: SemanticNodeId, mut visitor: F)
    where
        F: FnMut(OriginEdgeKind, &OriginEdge),
    {
        let store = self.derivation.lock();
        for (kind, edge) in store.origins(node) {
            visitor(kind, edge);
        }
    }

    /// Total origin edges across all result nodes. Mirrors the public
    /// [`SemanticGraphStats::origin_edge_count`].
    #[must_use]
    pub fn origin_edge_count(&self) -> usize {
        self.derivation.lock().edge_count()
    }

    /// Dispatch-side origin walk: visits every edge on `node` and merges
    /// each edge's `edge_dep_signature` into the supplied
    /// [`CompletionFence`](crate::completion_fence::CompletionFence) at
    /// hop-time. Returns the visited edges so the caller can recurse over
    /// `edge.sources` itself (the transitive walk is the caller's
    /// responsibility, per plan §7.16).
    ///
    /// Per plan §7.16, **edges are the only dep-sig propagation route for
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
    /// Safe to call mid-request; counters are atomic so no torn reads.
    #[must_use]
    pub fn stats_snapshot(&self) -> SemanticGraphStats {
        let derivation = self.derivation.lock();
        let origin_edge_count = derivation.edge_count() as u64;
        let max_origin_edges_per_node = derivation.max_edges_per_node() as u32;
        drop(derivation);
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
            origin_edge_count,
            origin_edges_emitted: self.stats.origin_edges_emitted.load(Ordering::Relaxed),
            max_origin_edges_per_node,
            instantiate_count: self.stats.instantiate_count.load(Ordering::Relaxed),
            conditional_decided_count: self.stats.conditional_decided_count.load(Ordering::Relaxed),
            conditional_deferred_count: self
                .stats
                .conditional_deferred_count
                .load(Ordering::Relaxed),
            branch_selections_true: self.stats.branch_selections_true.load(Ordering::Relaxed),
            branch_selections_false: self.stats.branch_selections_false.load(Ordering::Relaxed),
            budget_fallback_count: self.stats.budget_fallback_count.load(Ordering::Relaxed),
            max_path_length: self.stats.max_path_length.load(Ordering::Relaxed),
            max_projection_depth: self.stats.max_projection_depth.load(Ordering::Relaxed),
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
    pub fn record_path_length(&self, length: u32) {
        let mut current = self.stats.max_path_length.load(Ordering::Relaxed);
        while length > current {
            match self.stats.max_path_length.compare_exchange_weak(
                current,
                length,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }
    pub fn record_projection_depth(&self, depth: u32) {
        let mut current = self.stats.max_projection_depth.load(Ordering::Relaxed);
        while depth > current {
            match self.stats.max_projection_depth.compare_exchange_weak(
                current,
                depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
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
        let entries = self.entries.lock();
        entries.get(&family).and_then(|slots| {
            slots.slot(slot).cloned().map(|entry| CacheRead {
                value: entry.result,
                dep_signature: entry.dep_signature,
            })
        })
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
    /// `recursion_sentinel` produces a fallback [`SemanticNodeId`] when
    /// same-path recursion is detected.
    #[must_use = "the CacheRead carries both the resolved node id and the dep signature callers must merge into their active CompletionFence"]
    pub fn execute_cooperative<F, R>(
        &self,
        key: SemanticQueryKey,
        recursion_sentinel: R,
        build: F,
    ) -> CacheRead<QueryResult<SemanticNodeId>>
    where
        F: FnOnce() -> (QueryResult<SemanticNodeId>, DepSignature),
        R: FnOnce() -> SemanticNodeId,
    {
        // 1. Warm memo hit.
        if let Some(hit) = self.get(&key) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return hit;
        }
        self.stats.misses.fetch_add(1, Ordering::Relaxed);

        // 2. Same-path recursion detection — bail with a sentinel.
        let is_self_recursive =
            IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow().iter().any(|k| k == &key));
        if is_self_recursive {
            self.stats
                .same_path_sentinel_returns
                .fetch_add(1, Ordering::Relaxed);
            return CacheRead {
                value: QueryResult::Recursive(recursion_sentinel()),
                dep_signature: empty_signature(),
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
        let should_build = {
            let mut state = inflight.state.lock();
            if state.claimed {
                // Cooperative wait — block on the per-entry condvar until
                // `completed` is set. Joiners never busy-spin. Account
                // wait time on the stats surface so the F3 corpus
                // benchmark surfaces non-zero `waits_ms` (plan §6.3).
                let wait_start = std::time::Instant::now();
                inflight
                    .ready
                    .wait_while(&mut state, |s| s.completed.is_none());
                self.stats
                    .waits_ms
                    .fetch_add(wait_start.elapsed().as_millis() as u64, Ordering::Relaxed);
                let result = state.completed.clone().expect("winner must have published");
                let dep_signature = state.dep_signature.clone().unwrap_or_else(empty_signature);
                return CacheRead {
                    value: result,
                    dep_signature,
                };
            }
            state.claimed = true;
            true
        };
        debug_assert!(should_build);
        // Cold winner — record the in-flight presence for peak tracking
        // and ensure the exit decrement runs even on panic via the
        // existing InflightPanicGuard (extended below to cover this).
        self.stats.record_in_flight_enter();

        // 4. Execute the cold build. Both the recursion stack entry and
        //    the in-flight admission are protected by RAII guards so a
        //    panic inside `build()` cannot deadlock future callers.
        let _recursion_guard = RecursionStackGuard::push(key.clone());
        let mut panic_guard =
            InflightPanicGuard::new(Arc::clone(&inflight), &self.inflight, key.clone());
        let (result, dep_signature) = build();
        panic_guard.mark_finished();
        drop(panic_guard);
        drop(_recursion_guard);

        // 5. Warm-publish only successful values; errors and recursion
        //    sentinels never become shared-cache entries (plan §2 cache
        //    population). Successful results land in the requested
        //    `(family, slot)` and backfill every empty narrower slot in
        //    the same family — the backfill is a no-op against any slot a
        //    concurrent narrower compute already filled, so per-slot
        //    in-flight authority (§7.15) is preserved.
        let publishable = matches!(&result, QueryResult::Value(_));
        if publishable {
            let (family, slot) = family_and_slot(&key);
            // ResolvedNamedType bypasses the family memo entirely
            // (§7.16) — its DashMap-backed identity map is the cache.
            if !matches!(family, FamilyKey::ResolvedNamedType { .. }) {
                let entry = MemoEntry {
                    result: result.clone(),
                    dep_signature: dep_signature.clone(),
                };
                let mut entries = self.entries.lock();
                entries.entry(family).or_default().publish(slot, entry);
            }
        }

        // 6. Finalize in-flight and wake joiners. The completed flag
        //    guarantees any thread that acquired the flight before step 7
        //    retires the entry still observes the winner's result.
        {
            let mut state = inflight.state.lock();
            state.completed = Some(result.clone());
            state.dep_signature = Some(dep_signature.clone());
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
        // Decrement the in-flight presence counter. The peak counter is
        // monotonic; this only updates the current count.
        self.stats.record_in_flight_exit();

        CacheRead {
            value: result,
            dep_signature,
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

// ──────────────────────────────────────────────────────────────────────────
// Internal helpers
// ──────────────────────────────────────────────────────────────────────────

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{PrimitiveKind, ResolveDeclKey, ScopeId};

    fn scope(canonical: &str) -> ScopeId {
        ScopeId {
            canonical_id: Arc::from(canonical),
            local_scope: None,
        }
    }

    #[test]
    fn interning_returns_unique_stable_ids() {
        let store = SemanticGraphStore::new();
        let a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        assert_ne!(a, b);
        assert_eq!(a.0 + 1, b.0);
    }

    #[test]
    fn node_data_is_readable_via_graph_read_trait() {
        let store = SemanticGraphStore::new();
        let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let read: &dyn SemanticGraphRead = &store;
        let data = read.node_data(id);
        assert!(matches!(
            *data,
            SemanticNodeData::Primitive(PrimitiveKind::Boolean)
        ));
    }

    #[test]
    fn execute_cooperative_memoizes_winner_result() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });

        let mut call_count = 0u32;
        let _first = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                call_count += 1;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );

        // Second call must be a warm hit. The build closure is not invoked.
        let second = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                call_count += 1;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        );

        match second.value {
            QueryResult::Value(id) => {
                let data = store.node_data(id).unwrap();
                assert!(matches!(
                    *data,
                    SemanticNodeData::Primitive(PrimitiveKind::String)
                ));
            }
            other => panic!("expected warm value, got {other:?}"),
        }
        assert_eq!(call_count, 1, "cold build must run exactly once");
    }

    #[test]
    fn same_path_recursion_returns_sentinel_not_deadlock() {
        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Recursive"),
        });

        let store_ref = Arc::clone(&store);
        let key_ref = key.clone();

        let result = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                // Re-enter the same key from the same stack — this must
                // return a Recursive sentinel, not self-await.
                let inner = store_ref.execute_cooperative(
                    key_ref.clone(),
                    || store_ref.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        panic!("inner build must not run during same-path recursion");
                    },
                );
                match inner.value {
                    QueryResult::Recursive(_) => {
                        let id = store_ref
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                        (QueryResult::Value(id), empty_signature())
                    }
                    other => panic!("expected Recursive sentinel, got {other:?}"),
                }
            },
        );
        assert!(matches!(result.value, QueryResult::Value(_)));
    }

    #[test]
    fn errors_do_not_warm_shared_memo() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("BadBudget"),
        });

        let first = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Error(QueryError::Miss), empty_signature()),
        );
        assert!(matches!(first.value, QueryResult::Error(_)));
        assert_eq!(
            store.memo_entry_count(),
            0,
            "errors must not promote to warm memo entries"
        );

        let mut re_ran = false;
        let second = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                re_ran = true;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
        assert!(re_ran, "failed-result keys must not become warm");
        assert!(matches!(second.value, QueryResult::Value(_)));
    }

    #[test]
    fn dep_signature_is_returned_with_warm_hits() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });
        let sig: DepSignature = Arc::from(
            vec![(
                Arc::<str>::from("/w/a.ts"),
                crate::semantic_query::DepVersion::WholeHash([1u8; 16]),
            )]
            .into_boxed_slice(),
        );
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), sig.clone())
            },
        );
        let warm = store.get(&key).unwrap();
        assert_eq!(warm.dep_signature.len(), 1);
        assert_eq!(warm.dep_signature[0].0.as_ref(), "/w/a.ts");
    }

    /// A panic inside the cold build must not leave the in-flight entry
    /// in a `claimed=true, completed=None` state — otherwise the next
    /// caller for the same key would wait on the condvar forever.
    ///
    /// The `InflightPanicGuard` catches the drop and marks the entry with
    /// an `Error(Other)` sentinel so joiners fail fast and subsequent
    /// callers start a fresh build.
    #[test]
    fn panic_in_cold_build_does_not_deadlock_future_callers() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Explodes"),
        });

        // Cold build panics; `catch_unwind` turns it into an `Err`.
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            store.execute_cooperative(
                key.clone(),
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    panic!("simulated build panic");
                },
            )
        }));
        assert!(panicked.is_err(), "build must have unwound via panic");

        // The thread-local recursion stack must be empty (RAII guard) so
        // the same thread can query the same key without being flagged as
        // same-path recursion.
        let is_empty = IN_FLIGHT_ON_THIS_THREAD.with(|slot| slot.borrow().is_empty());
        assert!(is_empty, "recursion stack must be empty after panic");

        // A subsequent call for the same key must not deadlock. It must
        // be free to start a fresh cold build (the in-flight entry was
        // retired by the panic guard).
        let mut re_ran = false;
        let second = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                re_ran = true;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
        assert!(
            re_ran,
            "post-panic call must run a fresh cold build, not wait on the retired entry"
        );
        assert!(matches!(second.value, QueryResult::Value(_)));
    }

    /// `invalidate_canonical` removes every memo entry whose scope
    /// references the canonical — future queries compute fresh node ids
    /// under the new file version. Unrelated keys stay warm.
    #[test]
    fn invalidate_canonical_removes_only_matching_scope_keys() {
        let store = SemanticGraphStore::new();

        // Warm up `ResolveDecl(a.ts::Foo)`.
        let a_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            a_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );

        // Warm up `ResolveDecl(b.ts::Foo)` — same name, different canonical.
        let b_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/b.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            b_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), empty_signature())
            },
        );

        assert_eq!(store.memo_entry_count(), 2);

        // Invalidate only a.ts.
        let removed = store.invalidate_canonical("/w/a.ts");
        assert_eq!(removed, 1);
        assert_eq!(store.memo_entry_count(), 1);

        // b.ts still warm.
        assert!(store.get(&b_key).is_some());
        // a.ts gone — next call re-runs build.
        assert!(store.get(&a_key).is_none());
    }

    /// `invalidate_all` clears every memo entry — used on project-generation
    /// bumps per plan § A0 (tsconfig / SDK / workspace-folder changes).
    #[test]
    fn invalidate_all_clears_every_memo_entry() {
        let store = SemanticGraphStore::new();
        for name in ["X", "Y", "Z"] {
            let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: scope("/w/a.ts"),
                name: Arc::from(name),
            });
            let _ = store.execute_cooperative(
                key,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(id), empty_signature())
                },
            );
        }
        assert_eq!(store.memo_entry_count(), 3);
        let cleared = store.invalidate_all();
        assert_eq!(cleared, 3);
        assert_eq!(store.memo_entry_count(), 0);
    }

    #[test]
    fn recursive_sentinel_does_not_promote_to_warm_memo() {
        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("R"),
        });

        let id = store.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
        let res = store.execute_cooperative(
            key.clone(),
            || id,
            || (QueryResult::Recursive(id), empty_signature()),
        );
        assert!(matches!(res.value, QueryResult::Recursive(_)));
        assert_eq!(
            store.memo_entry_count(),
            0,
            "recursion sentinels must not promote to warm memo"
        );
    }

    /// Cross-thread waiter joins the in-flight key and observes the
    /// winner's published result. Exercises the `Condvar` pairing.
    #[test]
    fn cross_thread_joiner_waits_on_winner_publish() {
        use std::thread;
        use std::time::Duration;

        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Shared"),
        });

        let start_barrier = Arc::new(std::sync::Barrier::new(2));
        let store_owner = Arc::clone(&store);
        let key_owner = key.clone();
        let barrier_owner = Arc::clone(&start_barrier);

        let winner = thread::spawn(move || {
            store_owner.execute_cooperative(
                key_owner,
                || store_owner.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    barrier_owner.wait();
                    // Hold the build open briefly so the joiner reaches
                    // the condvar wait.
                    thread::sleep(Duration::from_millis(25));
                    let id =
                        store_owner.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(id), empty_signature())
                },
            )
        });

        // Let the winner claim first, then the joiner waits on the
        // condvar.
        start_barrier.wait();
        let joiner = thread::spawn({
            let store = Arc::clone(&store);
            let key = key.clone();
            move || {
                store.execute_cooperative(
                    key,
                    || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        panic!("joiner must never run the cold build");
                    },
                )
            }
        });

        let winner_result = winner.join().unwrap();
        let joiner_result = joiner.join().unwrap();

        // Both must see the winner's node id.
        match (winner_result.value, joiner_result.value) {
            (QueryResult::Value(w), QueryResult::Value(j)) => assert_eq!(w, j),
            other => panic!("unexpected combined result: {other:?}"),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Vue macro resolution identity map (former ResolvedNamedTypesDb)
    // ──────────────────────────────────────────────────────────────────

    use crate::semantic_query::HostResolvedNamedTypeKey;
    use verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey;
    use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

    fn make_key(canonical: &str, whole_hash: [u8; 16], name: &str) -> HostResolvedNamedTypeKey {
        HostResolvedNamedTypeKey {
            canonical_id: Arc::from(canonical),
            whole_hash,
            inner: ResolvedNamedTypeCacheKey {
                name: name.as_bytes().to_vec().into_boxed_slice(),
                surface: None,
                base_offset: 0,
                companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
                type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
            },
        }
    }

    /// Inserting a resolved-named-type entry stores the payload behind a
    /// `VueMacroElements` node and returns a stable [`SemanticNodeId`].
    /// Subsequent reads observe the same payload without rebuilding.
    #[test]
    fn resolved_named_type_insert_and_get_round_trip() {
        let store = SemanticGraphStore::new();
        let key = make_key("/w/a.ts", [1u8; 16], "Foo");
        let payload = Arc::new(ResolvedElements::default());
        let node_id = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

        // Identity lookup and payload lookup both succeed.
        assert_eq!(store.resolved_named_type_node_id(&key), Some(node_id));
        let round = store
            .get_resolved_named_type(&key)
            .expect("payload must be retrievable");
        assert!(Arc::ptr_eq(&payload, &round));
        assert_eq!(store.resolved_named_type_count(), 1);
    }

    /// Missing keys return `None` without allocating — the hot-path
    /// miss is refcount-free.
    #[test]
    fn resolved_named_type_missing_key_returns_none() {
        let store = SemanticGraphStore::new();
        let key = make_key("/w/a.ts", [0u8; 16], "Absent");
        assert!(store.get_resolved_named_type(&key).is_none());
        assert!(store.resolved_named_type_node_id(&key).is_none());
    }

    /// Per-canonical invalidation removes only matching entries; entries
    /// for unrelated canonicals stay warm.
    #[test]
    fn resolved_named_type_per_canonical_invalidation() {
        let store = SemanticGraphStore::new();
        let hash = [5u8; 16];
        let key_a = make_key("/w/a.ts", hash, "Foo");
        let key_b = make_key("/w/b.ts", hash, "Bar");
        store.insert_resolved_named_type(key_a.clone(), Arc::new(ResolvedElements::default()));
        store.insert_resolved_named_type(key_b.clone(), Arc::new(ResolvedElements::default()));
        assert_eq!(store.resolved_named_type_count(), 2);

        let removed = store.invalidate_resolved_named_types_for_canonical("/w/a.ts");
        assert_eq!(removed, 1);
        assert!(store.get_resolved_named_type(&key_a).is_none());
        assert!(store.get_resolved_named_type(&key_b).is_some());
    }

    /// Global clear removes every entry (used on project-generation
    /// bumps / epoch bumps).
    #[test]
    fn resolved_named_type_global_clear() {
        let store = SemanticGraphStore::new();
        let key = make_key("/w/a.ts", [1u8; 16], "Foo");
        store.insert_resolved_named_type(key.clone(), Arc::new(ResolvedElements::default()));
        assert_eq!(store.resolved_named_type_count(), 1);
        store.clear_resolved_named_types();
        assert_eq!(store.resolved_named_type_count(), 0);
        assert!(store.get_resolved_named_type(&key).is_none());
    }

    /// Repeat writes under the same key overwrite the identity mapping —
    /// two successive inserts leave one entry and the latest payload
    /// becomes observable. This matches the `NamedTypeCache` trait's
    /// "insert overwrites any prior entry under the same key" contract.
    #[test]
    fn resolved_named_type_repeated_insert_overwrites_identity_mapping() {
        let store = SemanticGraphStore::new();
        let key = make_key("/w/a.ts", [1u8; 16], "Foo");
        let first = Arc::new(ResolvedElements::default());
        let second = Arc::new(ResolvedElements {
            has_call_signature: true,
            ..ResolvedElements::default()
        });

        store.insert_resolved_named_type(key.clone(), Arc::clone(&first));
        store.insert_resolved_named_type(key.clone(), Arc::clone(&second));

        assert_eq!(
            store.resolved_named_type_count(),
            1,
            "same key must not duplicate identity entries"
        );
        let observed = store.get_resolved_named_type(&key).unwrap();
        assert!(
            Arc::ptr_eq(&second, &observed),
            "latest insert wins — identity map points at the second payload",
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // B1b family-memo backfill matrix (plan §3 B1b + §7.15)
    // ──────────────────────────────────────────────────────────────────

    fn family_test_path() -> Arc<[PathSegment]> {
        Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice())
    }

    fn family_test_key(base: SemanticNodeId, mode: ProjectionMode) -> SemanticQueryKey {
        SemanticQueryKey::ProjectPath {
            base,
            path: family_test_path(),
            mode,
        }
    }

    fn family_test_dep_signature() -> DepSignature {
        Arc::from(
            vec![(
                Arc::<str>::from("/w/family.ts"),
                crate::semantic_query::DepVersion::WholeHash([7u8; 16]),
            )]
            .into_boxed_slice(),
        )
    }

    /// Run a cold build for `mode` with a stable result + dep-signature.
    /// Returns the published `SemanticNodeId`.
    fn warm_family_slot(
        store: &SemanticGraphStore,
        base: SemanticNodeId,
        mode: ProjectionMode,
    ) -> SemanticNodeId {
        let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let key = family_test_key(base, mode);
        let read = store.execute_cooperative(
            key,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(value_id), family_test_dep_signature()),
        );
        match read.value {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        }
    }

    fn assert_warm_at(
        store: &SemanticGraphStore,
        base: SemanticNodeId,
        mode: ProjectionMode,
        expected_id: SemanticNodeId,
    ) {
        let warm = store
            .get(&family_test_key(base, mode))
            .unwrap_or_else(|| panic!("expected warm hit at mode {mode:?}"));
        match warm.value {
            QueryResult::Value(id) => assert_eq!(id, expected_id, "wrong node id at {mode:?}"),
            other => panic!("expected Value at {mode:?}, got {other:?}"),
        }
        assert_eq!(
            warm.dep_signature.as_ref(),
            family_test_dep_signature().as_ref(),
            "narrower-slot dep_signature must match the broader compute's at {mode:?}",
        );
    }

    fn assert_cold_at(store: &SemanticGraphStore, base: SemanticNodeId, mode: ProjectionMode) {
        assert!(
            store.get(&family_test_key(base, mode)).is_none(),
            "{mode:?} slot must NOT be backfilled",
        );
    }

    // 1. Expanded backfills each narrower slot (×4: source + 3 narrower).

    #[test]
    fn family_expanded_backfills_shallow_navigate_identity_share_dep_signature() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let id = warm_family_slot(&store, base, ProjectionMode::Expanded);

        // The Expanded slot itself.
        assert_warm_at(&store, base, ProjectionMode::Expanded, id);
        // All three narrower slots backfilled with the same id and same dep_sig.
        assert_warm_at(&store, base, ProjectionMode::Shallow, id);
        assert_warm_at(&store, base, ProjectionMode::Navigate, id);
        assert_warm_at(&store, base, ProjectionMode::Identity, id);
        assert_eq!(store.memo_entry_count(), 4, "all 4 slots populated");
    }

    // 2. Shallow backfills Navigate + Identity (×3).

    #[test]
    fn family_shallow_backfills_navigate_and_identity() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let id = warm_family_slot(&store, base, ProjectionMode::Shallow);

        assert_warm_at(&store, base, ProjectionMode::Shallow, id);
        assert_warm_at(&store, base, ProjectionMode::Navigate, id);
        assert_warm_at(&store, base, ProjectionMode::Identity, id);
        // Expanded MUST stay cold — narrower never satisfies broader.
        assert_cold_at(&store, base, ProjectionMode::Expanded);
        assert_eq!(store.memo_entry_count(), 3);
    }

    // 3. Navigate backfills Identity only (×2).

    #[test]
    fn family_navigate_backfills_identity_only() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let id = warm_family_slot(&store, base, ProjectionMode::Navigate);

        assert_warm_at(&store, base, ProjectionMode::Navigate, id);
        assert_warm_at(&store, base, ProjectionMode::Identity, id);
        assert_cold_at(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Expanded);
        assert_eq!(store.memo_entry_count(), 2);
    }

    // 4. Identity backfills NOTHING (single test, the negative case for it).

    #[test]
    fn family_identity_does_not_backfill_anything() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let id = warm_family_slot(&store, base, ProjectionMode::Identity);

        assert_warm_at(&store, base, ProjectionMode::Identity, id);
        assert_cold_at(&store, base, ProjectionMode::Navigate);
        assert_cold_at(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Expanded);
        assert_eq!(store.memo_entry_count(), 1);
    }

    // 5. Six negative cases: narrower never satisfies broader.

    #[test]
    fn family_navigate_does_not_satisfy_shallow_or_expanded() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let _ = warm_family_slot(&store, base, ProjectionMode::Navigate);
        assert_cold_at(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Expanded);
    }

    #[test]
    fn family_shallow_does_not_satisfy_expanded() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let _ = warm_family_slot(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Expanded);
    }

    #[test]
    fn family_identity_does_not_satisfy_navigate_shallow_expanded() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let _ = warm_family_slot(&store, base, ProjectionMode::Identity);
        assert_cold_at(&store, base, ProjectionMode::Navigate);
        assert_cold_at(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Expanded);
    }

    // 6. Concurrent narrower + broader cold builds — both run independently
    //    per `(family, mode_slot)` in-flight authority (§7.15).

    #[test]
    fn family_concurrent_navigate_and_expanded_both_complete_independently() {
        use std::sync::Barrier;
        use std::thread;
        let store = Arc::new(SemanticGraphStore::new());
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let nav_value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let exp_value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        // Barrier prevents either build closure from publishing until the
        // other has also entered its body — exercises per-(family, slot)
        // in-flight authority deterministically (without a barrier the
        // race is real and one thread can publish + backfill before the
        // other starts).
        let barrier = Arc::new(Barrier::new(2));

        let store_nav = Arc::clone(&store);
        let bar_nav = Arc::clone(&barrier);
        let store_exp = Arc::clone(&store);
        let bar_exp = Arc::clone(&barrier);
        let t_nav = thread::spawn(move || {
            store_nav.execute_cooperative(
                family_test_key(base, ProjectionMode::Navigate),
                || store_nav.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    bar_nav.wait();
                    (QueryResult::Value(nav_value), family_test_dep_signature())
                },
            )
        });
        let t_exp = thread::spawn(move || {
            store_exp.execute_cooperative(
                family_test_key(base, ProjectionMode::Expanded),
                || store_exp.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    bar_exp.wait();
                    (QueryResult::Value(exp_value), family_test_dep_signature())
                },
            )
        });
        let nav_read = t_nav.join().unwrap();
        let exp_read = t_exp.join().unwrap();

        let nav_id = match nav_read.value {
            QueryResult::Value(id) => id,
            other => panic!("nav: {other:?}"),
        };
        let exp_id = match exp_read.value {
            QueryResult::Value(id) => id,
            other => panic!("exp: {other:?}"),
        };
        // Each cold build returned its own value — both ran to completion
        // independently because per-(family, slot) in-flight authority
        // kept them on separate Condvar pairings, and the barrier kept
        // the publish ordering from racing them.
        assert_eq!(nav_id, nav_value);
        assert_eq!(exp_id, exp_value);
    }

    // 7. Wider backfill is a no-op when the narrower slot already filled.

    #[test]
    fn family_wider_backfill_noop_when_narrower_slot_already_filled() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        // Narrow build first — Navigate completes and fills Navigate +
        // Identity slots.
        let nav_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let _ = store.execute_cooperative(
            family_test_key(base, ProjectionMode::Navigate),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(nav_id), family_test_dep_signature()),
        );
        assert_warm_at(&store, base, ProjectionMode::Navigate, nav_id);
        assert_warm_at(&store, base, ProjectionMode::Identity, nav_id);

        // Now an Expanded build with a DIFFERENT result. Backfill writes
        // only into empty slots, so Navigate + Identity must keep their
        // narrower-build result; only Shallow + Expanded get the new id.
        let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            family_test_key(base, ProjectionMode::Expanded),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(exp_id), family_test_dep_signature()),
        );
        assert_warm_at(&store, base, ProjectionMode::Expanded, exp_id);
        assert_warm_at(&store, base, ProjectionMode::Shallow, exp_id);
        // Critical: the populated narrower slots survive — backfill is a
        // no-op against them.
        assert_warm_at(&store, base, ProjectionMode::Navigate, nav_id);
        assert_warm_at(&store, base, ProjectionMode::Identity, nav_id);
    }

    // 8. Cancelled / errored results do not backfill any slot.

    #[test]
    fn family_cancelled_does_not_backfill_any_slot() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let read = store.execute_cooperative(
            family_test_key(base, ProjectionMode::Expanded),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Error(QueryError::Miss), empty_signature()),
        );
        assert!(matches!(read.value, QueryResult::Error(_)));

        // Every slot — Expanded itself + the would-be backfilled narrower
        // slots — must stay cold. Errors never warm, ever.
        assert_cold_at(&store, base, ProjectionMode::Expanded);
        assert_cold_at(&store, base, ProjectionMode::Shallow);
        assert_cold_at(&store, base, ProjectionMode::Navigate);
        assert_cold_at(&store, base, ProjectionMode::Identity);
        assert_eq!(store.memo_entry_count(), 0);
    }

    // 9. ResolvedNamedType bypasses the family memo entirely (plan §7.16).
    //    The DashMap-backed identity map remains the only cache. After a
    //    successful execute_cooperative path returning Value via the build
    //    closure, the family memo's entries map stays empty for this key.

    // ──────────────────────────────────────────────────────────────────
    // B2 derivation/origin layer + telemetry tests
    // ──────────────────────────────────────────────────────────────────

    fn dep_sig_for(canonical: &str, hash: u8) -> DepSignature {
        Arc::from(
            vec![(
                Arc::<str>::from(canonical),
                crate::semantic_query::DepVersion::WholeHash([hash; 16]),
            )]
            .into_boxed_slice(),
        )
    }

    /// Multiple edges of the same kind on the same result are stored as a
    /// list — walkers see all of them. This is the multi-derivation
    /// support the contract requires (plan §2 + §7.16).
    #[test]
    fn origin_multiple_edges_same_kind() {
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let src_a = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let src_b = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));

        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![src_a].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/a.ts", 1),
        );
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![src_b].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/b.ts", 2),
        );

        let edges = store.origins_of_kind(result, OriginEdgeKind::Normalize);
        assert_eq!(edges.len(), 2, "both Normalize derivations preserved");
        assert_eq!(store.origin_edge_count(), 2);
    }

    /// `origins(node)` returns every edge across kinds. Sources are
    /// preserved verbatim from the recording call.
    #[test]
    fn origin_walk_returns_all_sources() {
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let decl = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![decl, arg].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/a.ts", 1),
        );

        let edges = store.origins(result);
        assert_eq!(edges.len(), 1);
        let (kind, edge) = &edges[0];
        assert_eq!(*kind, OriginEdgeKind::Instantiate);
        assert_eq!(edge.sources.as_ref(), &[decl, arg]);
    }

    /// `AliasResolve` edges from the unwrapped target back to the alias
    /// declaration identity are walkable. Each hop emits one edge so a
    /// chain is reconstructible.
    #[test]
    fn alias_resolve_edge_walk_returns_declaration_identity() {
        let store = SemanticGraphStore::new();
        let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let alias_decl = store.intern_node(SemanticNodeData::Alias(target));

        store.record_origin_edge(
            target,
            OriginEdgeKind::AliasResolve,
            Arc::from(vec![alias_decl].into_boxed_slice()),
            crate::semantic_query::OriginMeta::MemberName(Arc::from("AliasName")),
            dep_sig_for("/w/a.ts", 1),
        );

        let alias_edges = store.origins_of_kind(target, OriginEdgeKind::AliasResolve);
        assert_eq!(alias_edges.len(), 1);
        assert_eq!(alias_edges[0].sources.as_ref(), &[alias_decl]);
        assert!(matches!(
            &alias_edges[0].meta,
            crate::semantic_query::OriginMeta::MemberName(name) if name.as_ref() == "AliasName"
        ));
    }

    /// A barrel/re-export alias chain `X → Y → A` emits one
    /// `AliasResolve` edge per hop and the chain is walkable end-to-end.
    #[test]
    fn alias_chain_multiple_hops_walk() {
        let store = SemanticGraphStore::new();
        let final_target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let middle_alias = store.intern_node(SemanticNodeData::Alias(final_target));
        let outer_alias = store.intern_node(SemanticNodeData::Alias(middle_alias));

        // final_target ← middle_alias (one hop)
        store.record_origin_edge(
            final_target,
            OriginEdgeKind::AliasResolve,
            Arc::from(vec![middle_alias].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/a.ts", 1),
        );
        // middle_alias ← outer_alias (second hop)
        store.record_origin_edge(
            middle_alias,
            OriginEdgeKind::AliasResolve,
            Arc::from(vec![outer_alias].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/b.ts", 2),
        );

        // Walk from final_target — caller follows sources transitively.
        let mut chain: Vec<SemanticNodeId> = vec![final_target];
        let mut current = final_target;
        loop {
            let edges = store.origins_of_kind(current, OriginEdgeKind::AliasResolve);
            if edges.is_empty() {
                break;
            }
            current = edges[0].sources[0];
            chain.push(current);
        }
        assert_eq!(chain, vec![final_target, middle_alias, outer_alias]);
    }

    /// `stats_snapshot` increments hits + misses on warm + cold paths.
    #[test]
    fn stats_counters_increment_on_hit_and_miss() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/stats.ts"),
            name: Arc::from("Foo"),
        });

        let stats0 = store.stats_snapshot();
        assert_eq!(stats0.hits, 0);
        assert_eq!(stats0.misses, 0);

        // Cold call → misses increments by 1; hits stays 0.
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
        let stats1 = store.stats_snapshot();
        assert_eq!(stats1.misses, 1);
        assert_eq!(stats1.hits, 0);

        // Warm call → hits increments; misses stays at 1.
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || panic!("warm hit must skip the build closure"),
        );
        let stats2 = store.stats_snapshot();
        assert_eq!(stats2.misses, 1);
        assert_eq!(stats2.hits, 1);
    }

    /// `origins_with_fence` merges each edge's `edge_dep_signature` into
    /// the supplied fence at hop-time.
    #[test]
    fn origins_with_fence_merges_edge_dep_signature_at_each_hop() {
        use crate::completion_fence::CompletionFence;
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![src].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/inst.ts", 1),
        );
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![src].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/norm.ts", 2),
        );

        let fence = CompletionFence::new();
        let visited = store.origins_with_fence(result, &fence);
        assert_eq!(visited.len(), 2, "both edges visited");
        // Fence should now carry both canonicals' dep facts.
        let snapshot = fence.observed_signature();
        let canonicals: Vec<&str> = snapshot.iter().map(|(c, _v)| c.as_ref()).collect();
        assert!(
            canonicals.contains(&"/w/inst.ts"),
            "fence missing /w/inst.ts"
        );
        assert!(
            canonicals.contains(&"/w/norm.ts"),
            "fence missing /w/norm.ts"
        );
    }

    /// `origins(node)` (the read-only walk) does NOT touch any fence.
    /// Outside-execute consumers (LSP hover, debug dumps) use this form.
    #[test]
    fn plain_origins_walk_does_not_touch_active_fence() {
        use crate::completion_fence::CompletionFence;
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![src].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/x.ts", 1),
        );

        let fence = CompletionFence::new();
        let _ = store.origins(result);
        let snapshot = fence.observed_signature();
        assert!(
            snapshot.is_empty(),
            "plain origins() must NOT merge into active fence"
        );
    }

    /// Multiple derivations of the SAME structural result store as
    /// distinct edges with distinct dep-signatures. Walkers see all of
    /// them — there is no "canonical publisher" shortcut (plan §7.16).
    #[test]
    fn multiple_derivations_of_same_node_all_contribute_their_edges() {
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let src1 = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let src2 = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        // Two distinct Instantiate derivations producing the same result.
        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![src1].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/p1.ts", 1),
        );
        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![src2].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/p2.ts", 2),
        );

        let edges = store.origins_of_kind(result, OriginEdgeKind::Instantiate);
        assert_eq!(edges.len(), 2);
        let canonicals: Vec<&str> = edges
            .iter()
            .flat_map(|e| e.edge_dep_signature.iter().map(|(c, _)| c.as_ref()))
            .collect();
        assert!(canonicals.contains(&"/w/p1.ts"));
        assert!(canonicals.contains(&"/w/p2.ts"));
    }

    /// A purely structural node that no builder ever recorded an edge for
    /// has zero origins — the walk yields nothing and the caller's fence
    /// stays untouched. Structural / primitive / shared-literal nodes have
    /// no version identity, so this is correct.
    #[test]
    fn structural_node_has_zero_origin_edges_and_contributes_no_dep_sig() {
        use crate::completion_fence::CompletionFence;
        let store = SemanticGraphStore::new();
        let primitive = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let fence = CompletionFence::new();

        let visited = store.origins_with_fence(primitive, &fence);
        assert!(
            visited.is_empty(),
            "structural primitive node must have zero origin edges"
        );
        assert_eq!(store.origin_edge_count(), 0);
        assert!(
            fence.observed_signature().is_empty(),
            "fence must carry no facts when node has no origin edges"
        );
    }

    /// Edge dep-signature interning: two edges committed with identical
    /// fences share one `Arc<DepSignature>` allocation.
    #[test]
    fn edge_dep_signatures_intern_identical_fences() {
        let store = SemanticGraphStore::new();
        let result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let sig = dep_sig_for("/w/shared.ts", 1);
        store.record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![src].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            sig.clone(),
        );
        store.record_origin_edge(
            result,
            OriginEdgeKind::Normalize,
            Arc::from(vec![src].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            sig.clone(),
        );

        let edges = store.origins(result);
        assert_eq!(edges.len(), 2);
        let arc1 = &edges[0].1.edge_dep_signature;
        let arc2 = &edges[1].1.edge_dep_signature;
        assert!(
            Arc::ptr_eq(arc1, arc2),
            "identical fences must share one interned Arc<DepSignature>"
        );
    }

    /// `stats_snapshot()` is consistent mid-request: counters are atomic
    /// so concurrent readers never see torn values, and the per-call
    /// snapshot is internally consistent.
    #[test]
    fn stats_snapshot_is_consistent_mid_request() {
        let store = SemanticGraphStore::new();
        let _ = store.execute_cooperative(
            SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: scope("/w/snap.ts"),
                name: Arc::from("Foo"),
            }),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );

        let s1 = store.stats_snapshot();
        let s2 = store.stats_snapshot();
        assert_eq!(s1, s2, "two consecutive snapshots must be identical");
        assert_eq!(s1.misses, 1);
        assert_eq!(s1.memo_entry_count, 1);
    }

    #[test]
    fn resolved_named_type_refcount_path_unchanged_after_family_rewrite() {
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

        let store = SemanticGraphStore::new();
        let key = make_key("/w/named.ts", [9u8; 16], "Foo");
        let payload = Arc::new(ResolvedElements::default());
        let inserted_id = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

        // The family memo has zero entries — ResolvedNamedType is exempt.
        assert_eq!(
            store.memo_entry_count(),
            0,
            "ResolvedNamedType must NOT populate the family memo",
        );

        // Hot-path read still works refcount-only.
        let observed = store.get_resolved_named_type(&key).expect("warm");
        assert!(Arc::ptr_eq(&payload, &observed));

        // Formal `execute_cooperative` path: even if the build closure
        // succeeds with a Value, the family memo must not be populated for
        // this variant.
        let formal_key = SemanticQueryKey::ResolvedNamedType {
            key: Arc::new(key.clone()),
        };
        let read = store.execute_cooperative(
            formal_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store
                    .resolved_named_type_node_id(&key)
                    .expect("identity map populated above");
                (QueryResult::Value(id), empty_signature())
            },
        );
        match read.value {
            QueryResult::Value(id) => assert_eq!(id, inserted_id),
            other => panic!("expected Value via build, got {other:?}"),
        }
        assert_eq!(
            store.memo_entry_count(),
            0,
            "ResolvedNamedType warm-publish must NOT populate the family memo",
        );
        assert!(
            store.get(&formal_key).is_none(),
            "store.get must return None for ResolvedNamedType — it is bypassed"
        );
    }
}
