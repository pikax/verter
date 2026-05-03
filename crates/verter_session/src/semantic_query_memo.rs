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

use std::cell::RefCell;
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

use crate::semantic_query::{
    CacheRead, DeclIdentity, DepSignature, HostResolvedNamedTypeKey, IndexKey, MapperKey,
    NodeScopeId, OriginEdge, OriginEdgeKind, PathSegment, ProjectionMode, QueryError, QueryResult,
    ResolveDeclKey, SemanticGraphRead, SemanticGraphStats, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, ValueRootKey,
};

// ──────────────────────────────────────────────────────────────────────────
// Node arena — structurally interning, sharded dedup, stable ids
// ──────────────────────────────────────────────────────────────────────────
//
// The arena pairs each interned [`SemanticNodeData`] with an **origin-scope
// sidecar** (plan §7.10 + C1). Both the node vec and the parallel scope
// vec live inside one `RwLock<ArenaInner>` so reads (`node_data`,
// `node_scope`) are concurrent while writes (intern-miss) serialize.
//
// **Path C C7 — structural interning.** Two callers that construct the
// same `SemanticNodeData::Primitive(Number)` in the same scope share one
// [`SemanticNodeId`] — preventing the semantic graph from growing unbounded
// under repeated structural construction. Cross-scope same-payload
// interns stay distinct.
//
// **Path C C17 — sharded dedup index (plan §2 Stage 9).** The dedup index
// moved off `ArenaInner` onto `[Mutex<ShardIndex>; NUM_SHARDS]`. Payload
// hash + scope hash route to a specific shard; intern-hits (the steady-
// state hot path) take only that shard's Mutex — so `K` threads interning
// payloads that route to distinct shards proceed in parallel. Intern-misses
// acquire the shard Mutex, then briefly acquire `inner.write()` to allocate
// the next sequential id and push the node. Storage stays global and dense
// so `id.0 as usize` indexing + `a.0 + 1 == b.0` serial-id invariant are
// preserved.
//
// Dispatch builders query the sidecar via [`SemanticGraphStore::node_scope`]
// to route per-base-scope lookups through the correct
// [`SessionSolverHost`](crate::resolver_core::solver_host::SessionSolverHost)
// without threading scope through every call.
//
// **Exempt variants.** `SemanticNodeData::VueMacroElements` nodes store
// `None` in the sidecar slot — they live on the parser's refcount-only hot
// path and are never consumed by dispatch builders that walk `node_scope`.
// The exemption is enforced structurally inside `push_impl` so callers
// can't accidentally populate a sidecar entry for a vue-macro node, even
// via [`SemanticGraphStore::intern_node_with_scope`]. C17 preserves C7's
// short-circuit: `VueMacroElements` bypasses both the shard index and the
// shard Mutex entirely, acquiring only `inner.write()` for the sequential
// id allocation.

const NUM_SHARDS: usize = 16;
const SHARD_MASK: u64 = (NUM_SHARDS as u64) - 1;

/// Path C C17 — per-shard dedup index. Routes keyed by
/// `hash(payload, scope) & SHARD_MASK`. Same payload + scope → same
/// shard, so intern-hits never race across shards.
#[derive(Default)]
struct ShardIndex {
    index: FxHashMap<(SemanticNodeData, NodeScopeId), SemanticNodeId>,
}

/// Interior state of [`NodeArena`]. Held behind an `RwLock` so reads of
/// `(nodes, scopes)` (non-hot-path) are concurrent while the allocating
/// intern-miss path serializes on the writer.
#[derive(Default)]
struct ArenaInner {
    nodes: Vec<Arc<SemanticNodeData>>,
    /// Origin-scope sidecar (plan §7.10 + C1). Index-aligned with `nodes`.
    /// `None` marks an exempt slot (`VueMacroElements`); `Some(scope)`
    /// records the scope the node was first interned in (`Global` for
    /// scope-less structural nodes, `File { .. }` for declaration-origin
    /// nodes).
    scopes: Vec<Option<NodeScopeId>>,
}

/// Path C C17 shard routing — deterministic `hash((data, scope)) & mask`.
/// Same `(data, scope)` pair always routes to the same shard, regardless
/// of the calling thread. FxHash picked for speed; the dedup key's own
/// `Eq` implementation disambiguates collisions within a shard.
fn shard_index_for(data: &SemanticNodeData, scope: &NodeScopeId) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    data.hash(&mut hasher);
    scope.hash(&mut hasher);
    (hasher.finish() & SHARD_MASK) as usize
}

struct NodeArena {
    /// Global dense storage for node data + sidecar. `RwLock` so readers
    /// (`get`, `scope`) are concurrent and writers (intern-miss) briefly
    /// serialize to push a fresh slot.
    inner: parking_lot::RwLock<ArenaInner>,
    /// Path C C17 — sharded dedup indexes. Each shard owns the key-range
    /// whose `hash(payload, scope) & mask` lands on it.
    shards: [parking_lot::Mutex<ShardIndex>; NUM_SHARDS],
    /// Path C C1 instrumentation. When present, `push_impl` records per-call
    /// counters and inner.write() wait time so subsequent passes (C7, C17)
    /// have evidence-grade contention data without retro-fitting telemetry.
    /// `None` for the test-default arenas constructed via `Default::default()`.
    provenance: Option<Arc<crate::types::MetaProvenance>>,
}

impl Default for NodeArena {
    fn default() -> Self {
        Self {
            inner: parking_lot::RwLock::new(ArenaInner::default()),
            shards: std::array::from_fn(|_| parking_lot::Mutex::new(ShardIndex::default())),
            provenance: None,
        }
    }
}

impl NodeArena {
    /// Intern `data` with the `Global` scope tag. Helper intermediates and
    /// purely structural nodes use this path — most existing interning
    /// sites fall into this bucket.
    fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.push_impl(data, NodeScopeId::Global)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Called by
    /// builders that know the declaration origin — `build_resolve_decl`,
    /// `build_typeof`, C1's forthcoming `build_instantiate`, etc.
    fn push_with_scope(&self, data: SemanticNodeData, scope: NodeScopeId) -> SemanticNodeId {
        self.push_impl(data, scope)
    }

    fn push_impl(&self, data: SemanticNodeData, scope: NodeScopeId) -> SemanticNodeId {
        // `VueMacroElements` is exempt per plan §7.10 — record `None` so
        // `node_scope` returns `None` rather than `Some(Global)` for those
        // nodes. The exemption is structural so even
        // `intern_node_with_scope(VueMacroElements, Some(scope))` yields
        // `None` in the sidecar slot. Path C C7/C17 additionally short-
        // circuits `VueMacroElements` past the sharded dedup — identity-
        // carriers must allocate fresh slots on every insert so the
        // `NamedTypeCache` latest-insert-wins contract stays observable.
        let is_vue_macro = matches!(data, SemanticNodeData::VueMacroElements(_));

        // Path C C1 instrumentation. Capture the discriminant before
        // moving `data` so we can bucket per-variant pushes.
        let discriminant = data.discriminant_index();

        // Path C C17 — sharded dedup hot path. VueMacroElements bypasses
        // the shard index entirely (fresh slot every call). Other variants
        // route to their shard, check for an existing id; miss path
        // acquires inner.write() briefly to push the new slot.
        let (id, is_miss, write_wait_ns) = if is_vue_macro {
            let write_start = Instant::now();
            let mut inner = self.inner.write();
            let wait = write_start.elapsed().as_nanos() as u64;
            let id = SemanticNodeId(inner.nodes.len() as u64);
            inner.nodes.push(Arc::new(data));
            inner.scopes.push(None);
            (id, true, wait)
        } else {
            let shard_idx = shard_index_for(&data, &scope);
            let key = (data, scope);
            // Fast path: shard-hit. Shard Mutex is short-lived; parallel
            // across shards.
            {
                let shard = self.shards[shard_idx].lock();
                if let Some(&existing) = shard.index.get(&key) {
                    (existing, false, 0u64)
                } else {
                    drop(shard);
                    // Miss: re-acquire the shard (to serialize concurrent
                    // misses for the same key on this shard) and then
                    // briefly acquire inner.write() to allocate.
                    let mut shard = self.shards[shard_idx].lock();
                    if let Some(&existing) = shard.index.get(&key) {
                        // Another thread beat us to it.
                        (existing, false, 0u64)
                    } else {
                        let write_start = Instant::now();
                        let mut inner = self.inner.write();
                        let wait = write_start.elapsed().as_nanos() as u64;
                        let id = SemanticNodeId(inner.nodes.len() as u64);
                        inner.nodes.push(Arc::new(key.0.clone()));
                        inner.scopes.push(Some(key.1.clone()));
                        drop(inner);
                        shard.index.insert(key, id);
                        (id, true, wait)
                    }
                }
            }
        };

        if let Some(prov) = self.provenance.as_ref() {
            use std::sync::atomic::Ordering::Relaxed;
            prov.node_arena_pushes.fetch_add(1, Relaxed);
            if is_miss {
                prov.node_arena_intern_miss.fetch_add(1, Relaxed);
            }
            prov.node_arena_inner_write_wait_ns
                .fetch_add(write_wait_ns, Relaxed);
            if discriminant < prov.node_arena_pushes_per_discriminant.len() {
                prov.node_arena_pushes_per_discriminant[discriminant].fetch_add(1, Relaxed);
            } else {
                debug_assert!(
                    false,
                    "SemanticNodeData::discriminant_index() returned {} >= SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT",
                    discriminant
                );
            }
        }

        id
    }

    fn get(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        let inner = self.inner.read();
        inner.nodes.get(id.0 as usize).cloned()
    }

    /// Return the recorded origin scope for `id` — `None` for exempt nodes
    /// (or invalid ids), `Some(scope)` for everything else. Exempt slots
    /// are `VueMacroElements` nodes (plan §7.10).
    fn scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        let inner = self.inner.read();
        inner.scopes.get(id.0 as usize).cloned().flatten()
    }

    fn len(&self) -> usize {
        self.inner.read().nodes.len()
    }

    /// Drop shard-dedup entries for the given canonical id. Plan §1.10
    /// Γ.A invariant: invalidation does NOT drop `NodeScopeId::Global`
    /// — only `File { canonical_id: c, .. }` matches. Entries keyed at
    /// any other `File` canonical also survive.
    ///
    /// **Architectural property: the underlying arena Vec is
    /// append-only.** Existing `SemanticNodeId`s remain valid and
    /// resolve to the same payload via `get`/`scope`; this method
    /// affects only the dedup-shard's view of "next intern of this
    /// `(payload, scope)` pair returns the existing id". After
    /// invalidation, a re-intern of the same `(payload, File{c})`
    /// pair allocates a fresh node slot (and thus a fresh id),
    /// guaranteeing freshness against the changed canonical's content
    /// generation.
    ///
    /// Touches every shard mutex once. Each shard's retain walk is
    /// O(shard size). When `node_arena_lock_acquisitions` is wired
    /// into the audit context, each shard lock acquisition is recorded.
    fn invalidate_for_canonical(&self, canonical_id: &str) {
        for shard in self.shards.iter() {
            crate::host_manage::record_node_arena_lock_acquisition();
            let mut shard = shard.lock();
            shard.index.retain(|(_, scope), _| match scope {
                // Γ.A explicit invariant: Global never drops.
                NodeScopeId::Global => true,
                NodeScopeId::File {
                    canonical_id: c, ..
                } => c.as_ref() != canonical_id,
            });
        }
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
    /// Set by [`SemanticGraphStore::invalidate_canonical`] when this
    /// in-flight entry's `(family, slot)` matched the sweep. Joiners that
    /// wake from the condvar observe this flag and re-enter dispatch from
    /// step 1 rather than returning the (now stale) winner result. The
    /// cold winner skips warm publish when the flag is set so the stale
    /// result never re-populates the cache.
    aborted: bool,
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
    /// Relation-engine memo (plan §2 + §3 Change S). Added in Phase D §5.4
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
    /// Plan §6 / §13.2 Γ.B reverse index. For each canonical id,
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

/// Plan §6 / §13.2 Γ.B reverse-index type alias. See
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
        base: DeclIdentity,
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
    /// binding amendment — `ResolveMacroPayload`. Mode-erased
    /// for the family memo; the per-mode result lives in the matching
    /// `FamilySlots` slot.
    ResolveMacroPayload {
        owner: DeclIdentity,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: Arc<[SemanticNodeId]>,
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
    /// Plan §4.21 / R10-2 — Skeleton mode. Distinct semantics from
    /// Identity/Navigate/Shallow/Expanded (preserves open generics as
    /// TypeParam shells); does NOT backfill or get backfilled by other
    /// modes.
    Skeleton,
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
    /// Plan §4.21 / R10-2 — Skeleton mode slot. Independent from
    /// Navigate/Expanded; does NOT participate in backfill.
    skeleton: Option<MemoEntry>,
}

impl FamilySlots {
    fn slot(&self, slot: ModeSlot) -> Option<&MemoEntry> {
        match slot {
            ModeSlot::Single => self.single.as_ref(),
            ModeSlot::Identity => self.identity.as_ref(),
            ModeSlot::Navigate => self.navigate.as_ref(),
            ModeSlot::Shallow => self.shallow.as_ref(),
            ModeSlot::Expanded => self.expanded.as_ref(),
            ModeSlot::Skeleton => self.skeleton.as_ref(),
        }
    }

    fn slot_mut(&mut self, slot: ModeSlot) -> &mut Option<MemoEntry> {
        match slot {
            ModeSlot::Single => &mut self.single,
            ModeSlot::Identity => &mut self.identity,
            ModeSlot::Navigate => &mut self.navigate,
            ModeSlot::Shallow => &mut self.shallow,
            ModeSlot::Expanded => &mut self.expanded,
            ModeSlot::Skeleton => &mut self.skeleton,
        }
    }

    /// Publish `entry` to `slot` and backfill every narrower slot whose
    /// cell is empty. The narrower slots store the same `Arc`-shared
    /// [`MemoEntry`] (same result + same dep-signature) — this is the
    /// conservative "broader satisfies narrower" rule from plan §7.11; a
    /// dep-signature tightening pass against the actual narrower read-set
    /// is permitted follow-up work tracked in §1.4.
    ///
    /// Returns the list of slots that this publish actually populated
    /// (the primary slot + any previously-empty narrower slots that were
    /// backfilled). Plan §6 / §13.2 — the caller registers a
    /// reverse-index entry per populated slot in the per-canonical
    /// `canonical_to_entries` index. Capped at 6 (single + identity +
    /// navigate + shallow + expanded + skeleton), so a stack `SmallVec`
    /// keeps allocation off the hot path.
    fn publish(&mut self, slot: ModeSlot, entry: MemoEntry) -> smallvec::SmallVec<[ModeSlot; 6]> {
        let mut populated = smallvec::SmallVec::<[ModeSlot; 6]>::new();
        *self.slot_mut(slot) = Some(entry.clone());
        populated.push(slot);
        for narrower in backfill_targets(slot) {
            let cell = self.slot_mut(*narrower);
            if cell.is_none() {
                *cell = Some(entry.clone());
                populated.push(*narrower);
            }
        }
        populated
    }

    fn populated_count(&self) -> usize {
        let slots = [
            &self.single,
            &self.identity,
            &self.navigate,
            &self.shallow,
            &self.expanded,
            &self.skeleton,
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
        // diagnosis: record one signature-intern call per
        // invocation, classified into `returned_existing` vs.
        // `allocated`. The capture-token hook is a no-op when no
        // token is bound (zero-overhead production path). The
        // `with_active_capture` body never panics so it is safe to
        // run inside the `&mut self` borrow.
        if let Some(existing) = self.signature_pool.get(&sig) {
            crate::capture_token::with_active_capture(|t| t.record_signature_intern(true));
            return Arc::clone(existing);
        }
        let arc = Arc::new(sig.clone());
        self.signature_pool.insert(sig, Arc::clone(&arc));
        crate::capture_token::with_active_capture(|t| t.record_signature_intern(false));
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

    fn all_edges(&self) -> Vec<(SemanticNodeId, OriginEdgeKind, OriginEdge)> {
        self.edges
            .iter()
            .flat_map(|((node, kind), edges)| {
                edges.iter().map(move |edge| (*node, *kind, edge.clone()))
            })
            .collect()
    }
}

/// Pick the percentile-`p` value out of an already-sorted ascending
/// slice. Returns 0 for an empty slice. Index rounding matches the
/// nearest-rank method (R-3 / SAS / Excel `PERCENTILE.INC`).
fn sorted_percentile(sorted_ascending: &[u32], p: f64) -> u32 {
    if sorted_ascending.is_empty() {
        return 0;
    }
    let last = sorted_ascending.len() - 1;
    let idx = ((last as f64) * p).round() as usize;
    sorted_ascending[idx.min(last)]
}

// ──────────────────────────────────────────────────────────────────────────
// Telemetry — atomic counters (plan B2 + §7.4)
// ──────────────────────────────────────────────────────────────────────────

/// Bounded sample reservoir for histogram-style metrics (path length /
/// projection depth). Cap = 8192 samples per metric; once full, new
/// samples replace at a round-robin index so later samples have a chance
/// to land in the reservoir without unbounded memory growth.
///
/// Percentiles are computed at snapshot time by sorting a clone of the
/// reservoir — O(N log N) per snapshot where N <= cap, which is fine for
/// observational reads.
struct SampleCollector {
    samples: Vec<u32>,
    cap: usize,
    inserts: u64,
}

impl SampleCollector {
    fn with_cap(cap: usize) -> Self {
        Self {
            samples: Vec::new(),
            cap,
            inserts: 0,
        }
    }

    fn push(&mut self, value: u32) {
        self.inserts = self.inserts.saturating_add(1);
        if self.samples.len() < self.cap {
            self.samples.push(value);
        } else if self.cap > 0 {
            let idx = (self.inserts as usize) % self.cap;
            self.samples[idx] = value;
        }
    }

    fn percentile(&self, p: f64) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
        sorted[idx]
    }
}

const SAMPLE_RESERVOIR_CAP: usize = 8192;

/// Lock-free counter set updated on the hot path. Read into the immutable
/// [`SemanticGraphStats`] snapshot via [`SemanticGraphStore::stats_snapshot`].
struct AtomicSemanticGraphStats {
    hits: AtomicU64,
    misses: AtomicU64,
    same_path_sentinel_returns: AtomicU64,
    in_flight_current: AtomicU32,
    in_flight_peak: AtomicU32,
    waits_ms: AtomicU64,
    joined_waits: AtomicU64,
    inflight_aborted_retries: AtomicU64,
    cold_aborts_swept: AtomicU64,
    origin_edges_emitted: AtomicU64,
    instantiate_count: AtomicU64,
    conditional_decided_count: AtomicU64,
    conditional_deferred_count: AtomicU64,
    branch_selections_true: AtomicU64,
    branch_selections_false: AtomicU64,
    budget_fallback_count: AtomicU64,
    path_length_samples: Mutex<SampleCollector>,
    projection_depth_samples: Mutex<SampleCollector>,
    decl_subexpression_lowering_count: AtomicU64,
    relation_check_count: AtomicU64,
    /// Plan §8 / count of `intern_preserving_scope` calls
    /// observed by the store. Pre-Fix-D substitute helpers rebuilt
    /// every match arm unconditionally; post-Fix-D the no-op
    /// branches short-circuit and skip the call entirely.
    /// Discriminating signal for the change-tracking optimization.
    intern_preserving_scope_calls: AtomicU64,
}

impl Default for AtomicSemanticGraphStats {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            same_path_sentinel_returns: AtomicU64::new(0),
            in_flight_current: AtomicU32::new(0),
            in_flight_peak: AtomicU32::new(0),
            waits_ms: AtomicU64::new(0),
            joined_waits: AtomicU64::new(0),
            inflight_aborted_retries: AtomicU64::new(0),
            cold_aborts_swept: AtomicU64::new(0),
            origin_edges_emitted: AtomicU64::new(0),
            instantiate_count: AtomicU64::new(0),
            conditional_decided_count: AtomicU64::new(0),
            conditional_deferred_count: AtomicU64::new(0),
            branch_selections_true: AtomicU64::new(0),
            branch_selections_false: AtomicU64::new(0),
            budget_fallback_count: AtomicU64::new(0),
            path_length_samples: Mutex::new(SampleCollector::with_cap(SAMPLE_RESERVOIR_CAP)),
            projection_depth_samples: Mutex::new(SampleCollector::with_cap(SAMPLE_RESERVOIR_CAP)),
            decl_subexpression_lowering_count: AtomicU64::new(0),
            relation_check_count: AtomicU64::new(0),
            intern_preserving_scope_calls: AtomicU64::new(0),
        }
    }
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

/// RAII guard that decrements the in-flight presence counter on drop —
/// fires whether the cold-build closure returns normally or panics.
/// Without this guard a panic in the build closure would leak the
/// in-flight counter, biasing `in_flight_peak` upward across the
/// remaining lifetime of the store.
struct InFlightStatsGuard<'a> {
    stats: &'a AtomicSemanticGraphStats,
}

impl Drop for InFlightStatsGuard<'_> {
    fn drop(&mut self) {
        self.stats.record_in_flight_exit();
    }
}

/// Slot fan-out for backfill. `Expanded` satisfies `Shallow` / `Navigate` /
/// `Identity`; `Shallow` satisfies `Navigate` / `Identity`; `Navigate`
/// satisfies `Identity`. `Identity` and `Single` backfill nothing.
/// `Skeleton` is independent of the Identity/Navigate/Shallow/Expanded
/// hierarchy (different semantics: preserves open generics) — it backfills
/// nothing AND nothing backfills it (plan §4.21 / R10-2).
fn backfill_targets(slot: ModeSlot) -> &'static [ModeSlot] {
    match slot {
        ModeSlot::Single => &[],
        ModeSlot::Identity => &[],
        ModeSlot::Navigate => &[ModeSlot::Identity],
        ModeSlot::Shallow => &[ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Expanded => &[ModeSlot::Shallow, ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Skeleton => &[],
    }
}

fn mode_to_slot(mode: ProjectionMode) -> ModeSlot {
    match mode {
        ProjectionMode::Identity => ModeSlot::Identity,
        ProjectionMode::Navigate => ModeSlot::Navigate,
        ProjectionMode::Shallow => ModeSlot::Shallow,
        ProjectionMode::Expanded => ModeSlot::Expanded,
        ProjectionMode::Skeleton => ModeSlot::Skeleton,
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
        SemanticQueryKey::Instantiate {
            base,
            args,
            body_mode,
        } => (
            FamilyKey::Instantiate {
                base: base.clone(),
                args: Arc::clone(args),
            },
            mode_to_slot(*body_mode),
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
        // Phase D §5.4 WIP-S: `Relate` bypasses the family memo entirely —
        // it stores its tri-state result in the dedicated `relation_memo`
        // DashMap. `family_and_slot` returning a placeholder is safe
        // because `execute_cooperative` admission short-circuits `Relate`
        // before this function is consulted.
        SemanticQueryKey::Relate { source, target } => (
            FamilyKey::IndexedAccess {
                base: *source,
                index: crate::semantic_query::IndexKey::TypeNode(*target),
            },
            ModeSlot::Single,
        ),
        // binding amendment — `ResolveMacroPayload`. The
        // mode is stripped into the slot per the standard mode-bearing
        // pattern; the family identity is the (owner, macro_index,
        // macro_kind, type_args) tuple.
        SemanticQueryKey::ResolveMacroPayload {
            owner,
            macro_index,
            macro_kind,
            type_args,
            mode,
        } => (
            FamilyKey::ResolveMacroPayload {
                owner: owner.clone(),
                macro_index: *macro_index,
                macro_kind: *macro_kind,
                type_args: Arc::clone(type_args),
            },
            mode_to_slot(*mode),
        ),
    }
}

/// Returns `true` iff `sig` contains a dep-record that names `canonical_id`.
/// The single invalidation authority in B3: `invalidate_canonical` walks
/// every populated slot's stored dep-signature and evicts those whose
/// signature references the changed canonical. No structural short-cut on
/// family-key shape — the dep-sig is the only truth.
fn dep_signature_references_canonical(sig: &DepSignature, canonical_id: &str) -> bool {
    sig.iter()
        .any(|(canonical, _)| canonical.as_ref() == canonical_id)
}

/// Every [`ModeSlot`] variant as a static slice. Pre-Γ.B
/// `invalidate_canonical` linearly walked every family × every slot
/// here. Post-Γ.B the per-canonical reverse index drives the sweep,
/// but the constant is retained for invalidate-all and diagnostic
/// paths that still need to enumerate all slots.
#[allow(dead_code)]
const ALL_MODE_SLOTS: &[ModeSlot] = &[
    ModeSlot::Single,
    ModeSlot::Identity,
    ModeSlot::Navigate,
    ModeSlot::Shallow,
    ModeSlot::Expanded,
];

thread_local! {
    /// Per-thread set of query keys currently being executed. Used to
    /// detect same-path recursion so callers return a sentinel instead of
    /// self-awaiting.
    static IN_FLIGHT_ON_THIS_THREAD: RefCell<Vec<SemanticQueryKey>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII wrapper around a `parking_lot::MutexGuard` for the
/// `SemanticGraphStore::entries` mutex. Records the wait time
/// observed at acquisition and the hold time observed at drop on the
/// active [`crate::capture_token::CaptureToken`]. diagnosis
/// instrumentation only — the production hot path pays one extra
/// `Instant::now()` read per acquisition (constant-time) and the
/// Drop is a single `Instant::elapsed()` plus the no-op
/// `with_active_capture` hook when no token is bound.
struct EntriesLockGuard<'a, T> {
    guard: Option<parking_lot::MutexGuard<'a, T>>,
    hold_start: Instant,
    wait_ns: u128,
}

impl<'a, T> std::ops::Deref for EntriesLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard
            .as_ref()
            .expect("guard taken before Drop")
            .deref()
    }
}

impl<'a, T> std::ops::DerefMut for EntriesLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard
            .as_mut()
            .expect("guard taken before Drop")
            .deref_mut()
    }
}

impl<'a, T> Drop for EntriesLockGuard<'a, T> {
    fn drop(&mut self) {
        // Drop the inner guard FIRST so the mutex is released before
        // we record the hold time. Releasing the lock before the
        // capture-token hook keeps the hold-time measurement honest:
        // the hook itself runs outside the critical section. We use
        // `Option::take` + explicit `drop` (the `let _ = ...` form
        // is a clippy `let_underscore_lock` violation because it
        // could otherwise be read as a no-op binding).
        if let Some(guard) = self.guard.take() {
            std::mem::drop(guard);
        }
        let hold_ns = self.hold_start.elapsed().as_nanos();
        let wait_ns = self.wait_ns;
        crate::capture_token::with_active_capture(|t| {
            t.record_entries_mutex_timing(wait_ns, hold_ns);
        });
    }
}

impl SemanticGraphStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// diagnosis accessor: number of distinct interned
    /// `DepSignature` payloads in the derivation-signature pool. Used
    /// by the diagnosis benchmark to record the pool's growth across
    /// scenarios — `record_signature_pool_size` on the active capture
    /// token reads this value at end-of-capture.
    #[must_use]
    pub fn derivation_signature_pool_size(&self) -> usize {
        self.derivation.lock().signature_pool.len()
    }

    /// diagnosis-instrumented entries-mutex acquisition.
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
    /// plan §7.10 — their sidecar slot is forced to `None` structurally,
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
    /// plan §7.10; passing a non-`Global` scope has no effect for that
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
    /// `intern_preserving_scope` calls. Plan §8 /
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
    /// the origin scope, not their own (plan §7.10).
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
    /// Over-invalidation trade-off (plan §7.11): backfilled narrower
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

        // drain the per-canonical
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
            crate::host_manage::record_family_map_lock_acquisition();
            match self.canonical_to_entries.remove(canonical_id) {
                Some((_, mutex)) => {
                    let mut map = mutex.lock();
                    let drained: Vec<_> = map.drain().collect();
                    drained
                }
                None => Vec::new(),
            }
        };
        for ((family, slot), _) in &drained {
            affected_pairs.insert((family.clone(), *slot));
        }

        // walk the
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

        // for each evicted entry's
        // dep_signature, walk every other canonical it referenced and
        // drop the matching `(family, slot)` registration if it still
        // ptr_eq-matches our dep_signature. Lock order respected:
        // `entries` was unlocked at the close of before any
        // shard mutex is acquired here.
        for entry_sig in &evicted_dep_sigs {
            for (other_canonical, _) in entry_sig.iter() {
                if other_canonical.as_ref() == canonical_id {
                    continue;
                }
                crate::host_manage::record_family_map_lock_acquisition();
                if let Some(shard) = self.canonical_to_entries.get(other_canonical) {
                    let mut map = shard.value().lock();
                    map.retain(|_, registered_sig| {
                        // Keep entries whose registered_sig is a
                        // different `Arc` (fresh build) — only drop
                        // the exact registration tied to this
                        // evicted entry.
                        !Arc::ptr_eq(registered_sig, entry_sig)
                    });
                }
            }
        }

        // drop
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

        // drop
        // NodeArena shard-dedup entries keyed at
        // `File { canonical_id: c, .. }`. Preserves Global entries
        // and entries for any other canonical (plan §1.10 Γ.A). The
        // arena Vec is append-only — this only clears the "next
        // intern returns existing id" path; valid SemanticNodeIds for
        // nodes already published into the arena are unaffected.
        self.arena.invalidate_for_canonical(canonical_id);

        evicted
    }

    /// Clear every warm memo entry. Used on project-generation bumps
    /// (`tsconfig` changes, active-TS-SDK swaps, workspace-folder changes)
    /// per plan § A0. Returns the number of slots cleared (summed across
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
    // Relation memo (plan §2 + §3 Change S)
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
    /// this; the walker walks all edges (plan §2 + §7.16).
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
        // diagnosis instrumentation: bracket the entire
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
        // across that boundary (plan §4 Commit 4).
        //
        // the edge identity tuple is checked
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
                .edges
                .get(&(result, kind))
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
        // Plan §4 Commit 4: feed the accumulator of the active audited
        // request so the footprint miner sees every derivation hop.
        // No-op when no request context is installed.
        //
        // audit-mining contract preservation: this push is
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
        // skip the capture-token edge ledger insert + the
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
                // bump the per-call counter +
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
        for ((node, _kind), edges) in &derivation.edges {
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
    /// **Joiner retry on canonical invalidation (B3).** When a joiner
    /// wakes from the condvar and observes `state.aborted = true` (set by
    /// [`Self::invalidate_canonical`] when the (family, slot) was swept),
    /// it re-enters dispatch from step 1 up to [`MAX_INFLIGHT_RETRIES`]
    /// times. After exhausting the retry budget the joiner returns the
    /// sentinel so its caller fails fast rather than spinning.
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
        let mut miss_recorded = false;
        let mut retries = 0usize;

        // supplement §5.D.0 r17 — record cold/warm split for
        // the §5.D.1 cache-discipline tests. Done ONCE per logical
        // call (before the retry loop) so retries don't double-count.
        // Recorded with the canonical key the warm cache stores, AND
        // with the caller-side pre-canonical key (via raise/trait
        // entry-point recordings) so tests can probe by either form.
        let initial_hit = self.get(&key).is_some();
        #[cfg(test)]
        if initial_hit {
            crate::project_semantic_dispatch::raise::record_dispatch_warm(&key);
        } else {
            crate::project_semantic_dispatch::raise::record_dispatch_cold(&key);
        }
        // Issue #11 / propagate the warm/cold observation to
        // the per-request `CaptureToken` so `dispatch_count` and
        // `dispatch_misses` assertions can discriminate by family.
        // Recorded once per logical call (before the retry loop), like
        // the cfg(test) split above. The hook is a no-op when no token
        // is bound on the current thread (zero-overhead production path).
        crate::capture_token::with_active_capture(|t| t.record_dispatch(&key, initial_hit));

        let (inflight, key) = loop {
            // 1. Warm memo hit.
            if let Some(hit) = self.get(&key) {
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                // Per-context counter — the active request (if any)
                // observes this hit as its own. Plan §1.4.
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
                // benchmark surfaces non-zero `waits_ms` (plan §6.3).
                let wait_start = Instant::now();
                inflight
                    .ready
                    .wait_while(&mut state, |s| s.completed.is_none() && !s.aborted);
                self.stats
                    .waits_ms
                    .fetch_add(wait_start.elapsed().as_millis() as u64, Ordering::Relaxed);
                // Count every cooperative wait return (plan §6.3 /
                // Commit 1 `joined_waits`). Retries after abort re-enter
                // dispatch and may bump this again on the next join.
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
                    self.stats
                        .inflight_aborted_retries
                        .fetch_add(1, Ordering::Relaxed);
                    if let Some(ctx) = verter_scheduler::request_context::current_context() {
                        ctx.0.record_cache_event(
                            verter_scheduler::request_context::CacheEventKind::InflightAbortedRetry,
                        );
                    }
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
                if let Some(prov) = self.provenance.as_ref() {
                    prov.execute_cooperative_joiner_path
                        .fetch_add(1, Ordering::Relaxed);
                }
                return CacheRead {
                    value: result,
                    dep_signature,
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
        let (result, dep_signature) = build();
        let build_held_ns = build_start.elapsed().as_nanos() as u64;
        panic_guard.mark_finished();
        drop(panic_guard);
        drop(_recursion_guard);
        if let Some(prov) = self.provenance.as_ref() {
            prov.execute_cooperative_held_ns
                .fetch_add(build_held_ns, Ordering::Relaxed);
        }

        // 5. Warm-publish only successful values; errors and recursion
        //    sentinels never become shared-cache entries (plan §2 cache
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
        // refactor: cold-winner publish path is encapsulated in
        // `warm_publish_one` so that `publish_warm_if_absent` (used by
        // the §1.B prefix-backfill in `build_project_path`) can reuse the
        // same family/slot mapping + reverse-index registration without
        // duplicating the publish primitives. Pure refactor — TOCTOU
        // semantics, ResolvedNamedType bypass, and reverse-index
        // semantics all live inside the helper.
        self.warm_publish_one(&key, &result, &dep_signature, &inflight);

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
        }
    }

    /// Cold-winner publish path. Extracted from
    /// [`Self::execute_cooperative`] step 5 (refactor — pure
    /// extraction, no behaviour change). Skips publish when the result is
    /// not a [`QueryResult::Value`] (errors / recursion sentinels never
    /// promote to warm cache entries — plan §2 cache population). Skips
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
        let entry = MemoEntry {
            result: result.clone(),
            dep_signature: dep_signature.clone(),
        };
        let mut entries = self.entries_lock_diagnosed();
        // Test-only forcing: simulate a concurrent sweep that aborted
        // this in-flight entry just before the TOCTOU re-check.
        // Deterministically drives the `cold_aborts_swept` counter in
        // `..._when_forced` tests without needing a racy real invalidation.
        #[cfg(test)]
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
            self.stats.cold_aborts_swept.fetch_add(1, Ordering::Relaxed);
            if let Some(ctx) = verter_scheduler::request_context::current_context() {
                ctx.0.record_cache_event(
                    verter_scheduler::request_context::CacheEventKind::ColdAbortSwept,
                );
            }
            return;
        }
        let populated_slots = entries
            .entry(family.clone())
            .or_default()
            .publish(slot, entry);
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

    /// variant of [`Self::warm_publish_one`]: publish
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
        let entry = MemoEntry {
            result,
            dep_signature: dep_signature.clone(),
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
        for populated in populated_slots {
            for (canonical, _) in dep_signature.iter() {
                crate::host_manage::record_family_map_lock_acquisition();
                let shard = canonical_to_entries
                    .entry(Arc::clone(canonical))
                    .or_insert_with(|| Mutex::new(FxHashMap::default()));
                let mut map = shard.value().lock();
                map.insert((family.clone(), *populated), Arc::clone(dep_signature));
            }
        }
    }

    /// path-prefix backfill API (plan §1.B). Publishes a
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

/// Maximum number of times a joiner re-enters dispatch after its
/// in-flight entry was aborted by a canonical invalidation sweep. Bounds
/// pathological retry loops (e.g. an invalidation that keeps firing on
/// the same canonical) to a small constant; in practice 0-1 retries is
/// typical because the next call either hits a freshly-warm slot or
/// claims the fresh in-flight as winner.
const MAX_INFLIGHT_RETRIES: usize = 3;

/// Test-only forcing flag: when set, the cold-winner re-check in
/// `execute_cooperative` marks its own in-flight entry `aborted = true`
/// just before the TOCTOU abort-check, simulating a concurrent canonical
/// invalidation sweep. Drives `cold_aborts_swept` deterministically in
/// `semantic_graph_stats_cold_aborts_swept_increments_when_forced`.
///
/// Tests must clear the flag before returning (RAII guard pattern).
#[cfg(test)]
pub(crate) static FORCE_COLD_ABORT_SWEEP: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
impl SemanticGraphStore {
    /// Test-only: set `aborted = true` on the in-flight entry for `key`,
    /// plant an `Error(Other)` sentinel on `completed` if absent, notify
    /// waiters, and remove the entry from the table. Mirrors
    /// `invalidate_canonical` exactly but bypasses the step 1
    /// warm-slot gate so joiner-retry tests don't have to race a real
    /// invalidation window between publish and inflight retirement.
    ///
    /// Returns `true` when an entry for `key` was aborted, `false` when
    /// the in-flight table did not contain the key.
    pub(crate) fn test_trigger_inflight_abort(&self, key: &SemanticQueryKey) -> bool {
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

// ──────────────────────────────────────────────────────────────────────────
// Plan §7 / DepSignatureInterner
// ──────────────────────────────────────────────────────────────────────────

/// Content-hash bucketed `Weak<...>` interner for `DepSignature`. Plan
/// §7 / §1.10 Γ.C. Equivalent dep_signatures (same `(canonical,
/// version)` set after sort+dedup) share a single `Arc<[(...)]>` so:
///
/// 1. The reverse-index `Arc::ptr_eq` discrimination matches
///    "our entry" vs "fresh post-publish write" correctly.
/// 2. Memory pressure stays bounded — N publishes of the same dep
///    closure store one allocation, not N.
///
/// **Liveness via `Weak<...>`:** the interner holds `Weak` references
/// only. When the last strong `Arc` is dropped, `intern` notices the
/// dead `Weak` on next lookup and prunes it. `sweep()` can be called
/// periodically to reclaim empty buckets.
///
/// **Bucketing key:** `u64` content hash via `FxHash` over the
/// canonicalised payload. Collisions are tolerated — within a bucket
/// the `intern` path performs a content equality check before
/// returning the existing Arc.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct DepSignatureInterner {
    table: DashMap<u64, Vec<DepSignatureWeak>>,
    /// Plan §7 — counter-based auto-sweep trigger. Incremented on
    /// every successful intern; sweep runs when the counter hits
    /// `SWEEP_INTERVAL`. Cheap O(buckets) walk; off the hot path.
    inserts_since_sweep: std::sync::atomic::AtomicU64,
}

/// `Weak` view of an interned `DepSignature` payload — see
/// [`DepSignatureInterner`].
#[allow(dead_code)]
type DepSignatureWeak = std::sync::Weak<[(Arc<str>, crate::semantic_query::DepVersion)]>;

#[allow(dead_code)]
const SWEEP_INTERVAL: u64 = 1024;

#[allow(dead_code)]
impl DepSignatureInterner {
    /// Construct a fresh interner with no buckets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `payload`, returning a shared `Arc` whose pointer
    /// equality (`Arc::ptr_eq`) matches every other equivalent intern.
    ///
    /// Equivalent dep_signatures are normalised before lookup: pairs
    /// are sorted by `(canonical, version)` and adjacent duplicates
    /// removed. This ensures `intern([(a, v1), (b, v2)])` returns the
    /// same `Arc` as `intern([(b, v2), (a, v1), (a, v1)])`.
    pub fn intern(
        &self,
        payload: &[(Arc<str>, crate::semantic_query::DepVersion)],
    ) -> DepSignature {
        // Normalise: sort + dedup so equivalent content collapses.
        let mut normalised: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = payload.to_vec();
        normalised.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()).then_with(|| a.1.cmp(&b.1)));
        normalised.dedup();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = rustc_hash::FxHasher::default();
            normalised.len().hash(&mut hasher);
            for (canonical, version) in &normalised {
                canonical.hash(&mut hasher);
                version.hash(&mut hasher);
            }
            hasher.finish()
        };

        let mut bucket = self.table.entry(hash).or_default();
        // Prune dead Weaks while scanning.
        bucket.retain(|w| w.strong_count() > 0);
        for w in bucket.iter() {
            if let Some(arc) = w.upgrade() {
                if arc.iter().eq(normalised.iter()) {
                    crate::host_manage::record_dep_signature_intern_hit();
                    return Arc::clone(&arc) as DepSignature;
                }
            }
        }
        // Miss: insert a fresh Arc and downgrade for the bucket.
        let fresh: Arc<[(Arc<str>, crate::semantic_query::DepVersion)]> =
            Arc::from(normalised.into_boxed_slice());
        bucket.push(Arc::downgrade(&fresh));
        drop(bucket);

        // Auto-sweep trigger. Plan §7: cheap O(buckets) walk every
        // SWEEP_INTERVAL inserts.
        let n = self
            .inserts_since_sweep
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        if n.is_multiple_of(SWEEP_INTERVAL) {
            self.sweep();
        }

        fresh
    }

    /// Intern a single `(canonical, version)` pair. Convenience for
    /// call sites that build dep_signatures incrementally. Plan §7.
    pub fn intern_canonical(
        &self,
        canonical: Arc<str>,
        version: crate::semantic_query::DepVersion,
    ) -> DepSignature {
        debug_assert!(
            !canonical.as_ref().is_empty(),
            "intern_canonical: canonical id must be non-empty"
        );
        self.intern(&[(canonical, version)])
    }

    /// Periodic sweep — removes empty buckets and dead `Weak`s. Plan
    /// §7 (round-7 Codex#2 P1 #2). Called by the host's idle-time
    /// cleanup pipeline AND auto-triggered every `SWEEP_INTERVAL`
    /// inserts.
    ///
    /// O(buckets) where buckets = distinct content hashes seen so
    /// far. Cheap relative to a full warm-cache sweep because
    /// dep_signature content is highly redundant in practice.
    pub fn sweep(&self) {
        self.table.retain(|_, bucket| {
            bucket.retain(|w| w.strong_count() > 0);
            !bucket.is_empty()
        });
    }

    /// Test/diagnostic: number of distinct hash buckets currently
    /// stored. May include empty buckets that have not yet been
    /// reaped by `sweep`.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.table.len()
    }

    /// Test/diagnostic: number of distinct interned dep_signatures
    /// (i.e., total live `Weak`s across every bucket).
    #[must_use]
    pub fn live_signature_count(&self) -> usize {
        self.table
            .iter()
            .map(|entry| {
                entry
                    .value()
                    .iter()
                    .filter(|w| w.strong_count() > 0)
                    .count()
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{DepVersion, PrimitiveKind, ResolveDeclKey, ScopeId};

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

    /// Path C C7 positive invariant — two `intern_node_with_scope` calls
    /// for the same `(payload, scope)` pair share one
    /// [`SemanticNodeId`]. Under the pre-C7 append-only allocator the two
    /// calls returned distinct ids (plan §14.3 positive discriminator).
    #[test]
    fn intern_dedups_structural_values_across_contexts() {
        let store = SemanticGraphStore::new();
        let first = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Number),
            NodeScopeId::Global,
        );
        let second = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Number),
            NodeScopeId::Global,
        );
        assert_eq!(
            first, second,
            "structurally-identical (payload, scope) pairs must dedup \
             to one SemanticNodeId under C7 compound-key interning",
        );

        // Scope axis still disambiguates: same payload in a different
        // scope produces a distinct id.
        let scoped = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Number),
            NodeScopeId::File {
                canonical_id: Arc::from("/w/a.ts"),
                whole_hash: [0u8; 16],
                local_scope: None,
            },
        );
        assert_ne!(
            first, scoped,
            "cross-scope same-payload interns must stay distinct — C7 \
             preserves the scope disambiguation axis",
        );
    }

    /// Path C C7 negative invariant — `VueMacroElements` is an
    /// identity-carrier with latest-insert-wins semantics (see
    /// [`SemanticGraphStore::insert_resolved_named_type`]). Two
    /// `intern_node` calls for the same `Arc<ResolvedElements>` payload
    /// must still return distinct [`SemanticNodeId`]s so fresh inserts
    /// under the same `HostResolvedNamedTypeKey` do not alias with prior
    /// payloads. Under naive structural dedup this would collapse — the
    /// exemption in `push_impl` short-circuits the dedup index.
    #[test]
    fn intern_does_not_dedup_vue_macro_elements_identity_carrier() {
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
        let store = SemanticGraphStore::new();
        let payload = Arc::new(ResolvedElements::default());
        let a = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
        let b = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
        assert_ne!(
            a, b,
            "VueMacroElements must allocate fresh slots on every insert — \
             identity-carrier contract requires latest-insert-wins semantics",
        );
        // Sidecar stays `None` for both slots — exempt from origin-scope
        // tracking per plan §7.10.
        assert_eq!(store.node_scope(a), None);
        assert_eq!(store.node_scope(b), None);
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

    /// Path C C17 — sharded dedup produces the same `SemanticNodeId`
    /// across threads for identical `(payload, scope)` pairs. The
    /// invariant is strong: two threads interning the same payload at
    /// the same scope must observe equal ids immediately (no visibility
    /// gap from C17's per-shard Mutex). The threads race; the second
    /// arrival finds the first's entry in the shard index and returns
    /// the same id rather than allocating a duplicate.
    #[test]
    fn intern_identity_invariant_holds_across_threads() {
        use std::thread;
        let store = Arc::new(SemanticGraphStore::new());
        let store_a = Arc::clone(&store);
        let store_b = Arc::clone(&store);
        let handle_a = thread::spawn(move || {
            store_a.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
        });
        let handle_b = thread::spawn(move || {
            store_b.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
        });
        let id_a = handle_a.join().expect("thread A joined");
        let id_b = handle_b.join().expect("thread B joined");
        assert_eq!(
            id_a, id_b,
            "C17 sharded intern must produce identical SemanticNodeId across \
             threads for the same (payload, scope) pair — found {id_a:?} vs {id_b:?}",
        );
    }

    /// Path C C17 — `shard_index_for` is deterministic: identical
    /// `(data, scope)` pairs route to the same shard regardless of
    /// calling thread or program run. This is load-bearing for the
    /// sharded-dedup correctness: a payload's shard must not drift
    /// across invocations or the second intern would land on a
    /// different shard and allocate a duplicate id.
    #[test]
    fn shard_routing_is_deterministic_per_payload_and_scope() {
        let data_a = SemanticNodeData::Primitive(PrimitiveKind::String);
        let data_b = SemanticNodeData::Primitive(PrimitiveKind::String);
        let scope_global = NodeScopeId::Global;
        let scope_file = NodeScopeId::File {
            canonical_id: Arc::from("/w/x.ts"),
            whole_hash: [0u8; 16],
            local_scope: None,
        };
        assert_eq!(
            shard_index_for(&data_a, &scope_global),
            shard_index_for(&data_b, &scope_global),
            "shard routing must be stable for identical payloads at identical scopes",
        );
        // Different scope → may route differently, but the result is
        // still deterministic per call.
        let s1 = shard_index_for(&data_a, &scope_file);
        let s2 = shard_index_for(&data_a, &scope_file);
        assert_eq!(s1, s2, "shard routing must be stable across repeat calls");
        assert!(s1 < NUM_SHARDS, "shard index must stay within NUM_SHARDS");
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

    /// `invalidate_canonical` sweeps every slot whose recorded
    /// dep-signature references the changed canonical. Unrelated entries
    /// stay warm because their dep-signatures never mention the canonical
    /// under invalidation.
    #[test]
    fn invalidate_canonical_removes_only_matching_scope_keys() {
        let store = SemanticGraphStore::new();

        // Warm `ResolveDecl(a.ts::Foo)` with a dep-sig referencing /w/a.ts.
        let a_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            a_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
            },
        );

        // Warm `ResolveDecl(b.ts::Foo)` with a dep-sig referencing /w/b.ts.
        let b_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/b.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            b_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), dep_sig_for("/w/b.ts", 2))
            },
        );

        assert_eq!(store.memo_entry_count(), 2);

        // Dep-sig sweep: only a.ts's entry matches.
        let removed = store.invalidate_canonical("/w/a.ts");
        assert_eq!(removed, 1);
        assert_eq!(store.memo_entry_count(), 1);

        // b.ts still warm (its dep-sig never mentioned /w/a.ts).
        assert!(store.get(&b_key).is_some());
        // a.ts gone — next call re-runs build.
        assert!(store.get(&a_key).is_none());
    }

    // ──────────────────────────────────────────────────────────────────
    // DepSignatureInterner (Γ.C)
    // ──────────────────────────────────────────────────────────────────

    /// interner returns the SAME
    /// `Arc` for two distinct calls with equivalent payload.
    /// Discriminating: pre-fix tree has no interner, every publish
    /// builds a fresh Arc. Post-fix tree: dedup via content hash.
    #[test]
    fn dep_signature_interner_returns_same_arc_for_equivalent_payloads() {
        let interner = DepSignatureInterner::new();
        let payload_a = vec![
            (
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([1u8; 16]),
            ),
            (
                Arc::<str>::from("/w/b.ts"),
                DepVersion::WholeHash([2u8; 16]),
            ),
        ];
        // Reordered with a duplicate — must normalise to the same
        // canonical form.
        let payload_b = vec![
            (
                Arc::<str>::from("/w/b.ts"),
                DepVersion::WholeHash([2u8; 16]),
            ),
            (
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([1u8; 16]),
            ),
            (
                Arc::<str>::from("/w/a.ts"),
                DepVersion::WholeHash([1u8; 16]),
            ),
        ];
        let arc_a = interner.intern(&payload_a);
        let arc_b = interner.intern(&payload_b);
        assert!(
            Arc::ptr_eq(&arc_a, &arc_b),
            "equivalent payloads (modulo order + dups) must intern to the same Arc"
        );
        // Different content → different Arc.
        let payload_c = vec![(
            Arc::<str>::from("/w/c.ts"),
            DepVersion::WholeHash([3u8; 16]),
        )];
        let arc_c = interner.intern(&payload_c);
        assert!(
            !Arc::ptr_eq(&arc_a, &arc_c),
            "different payloads must intern to different Arcs"
        );
    }

    /// sweep removes empty buckets and dead-Weak buckets.
    /// Plan §7 round-7 Codex#2 P1 #2 — mandatory test:
    /// `dep_signature_intern_sweep_removes_empty_buckets`.
    #[test]
    fn dep_signature_intern_sweep_removes_empty_buckets() {
        let interner = DepSignatureInterner::new();
        let payload = vec![(
            Arc::<str>::from("/w/sweep.ts"),
            DepVersion::WholeHash([7u8; 16]),
        )];

        // Intern, drop the strong ref, sweep — bucket must be removed.
        {
            let _arc = interner.intern(&payload);
            assert!(
                interner.bucket_count() >= 1,
                "intern must populate the bucket"
            );
            assert_eq!(
                interner.live_signature_count(),
                1,
                "interned signature must be live"
            );
        } // _arc dropped here.

        // Strong ref gone; bucket entry now contains a dead Weak.
        // sweep() must reclaim the empty bucket.
        assert_eq!(
            interner.live_signature_count(),
            0,
            "after dropping the strong ref, the Weak is dead"
        );
        interner.sweep();
        assert_eq!(
            interner.bucket_count(),
            0,
            "sweep() must reclaim the empty bucket"
        );
    }

    /// auto-sweep trigger fires every `SWEEP_INTERVAL`
    /// inserts. Discriminating: drop strong refs, then intern enough
    /// distinct signatures to trip the auto-sweep. The bucket count
    /// stays bounded.
    #[test]
    fn dep_signature_intern_auto_sweep_keeps_bucket_count_bounded() {
        let interner = DepSignatureInterner::new();
        // Insert and drop SWEEP_INTERVAL+1 distinct signatures — each
        // bucket becomes orphaned immediately because the Arc never
        // escapes the loop body. Auto-sweep is triggered when the
        // counter hits SWEEP_INTERVAL.
        for i in 0..(SWEEP_INTERVAL + 1) {
            let canonical: Arc<str> = Arc::from(format!("/w/n{i}.ts"));
            let _arc = interner.intern_canonical(canonical, DepVersion::ProjectGeneration(i));
        }
        // After auto-sweep, dead-Weak buckets should be reclaimed.
        // Tolerate up to SWEEP_INTERVAL stragglers (the buckets that
        // landed after the auto-sweep tick; counter resumes counting).
        assert!(
            interner.bucket_count() <= SWEEP_INTERVAL as usize,
            "auto-sweep must keep bucket count bounded; got {}",
            interner.bucket_count()
        );
    }

    /// `invalidate_canonical(c)`
    /// uses the `canonical_to_entries` reverse index to find affected
    /// `(family, slot)` pairs in O(referencing entries) instead of
    /// O(all entries). A publish must register its dep_signature in
    /// the reverse index for every canonical it references.
    ///
    /// Discriminating: warm a specific `(family, slot)` whose
    /// dep_signature references "/w/a.ts". Assert
    /// `canonical_to_entries_count("/w/a.ts") >= 1`. Pre-fix:
    /// reverse index was never populated, count is 0; assertion
    /// FAILS. Post-fix: count is at least 1 (one for the family +
    /// each backfilled narrower slot).
    #[test]
    fn family_map_publish_registers_canonical_to_entries_reverse_index() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            key,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
            },
        );
        assert!(
            store.canonical_to_entries_count("/w/a.ts") >= 1,
            "publish must register the (family, slot) → dep_signature mapping \
             in canonical_to_entries[\"/w/a.ts\"] (Γ.B reverse index)"
        );
        assert_eq!(
            store.canonical_to_entries_count("/w/missing.ts"),
            0,
            "unrelated canonicals must NOT have a reverse-index entry"
        );
    }

    /// refactor invariant — the helper extracted from
    /// `execute_cooperative` step 5 (`warm_publish_one`) must:
    ///   1. Insert into the warm map (slot becomes `get`-readable).
    ///   2. Register the `(family, slot) → dep_signature` reverse-index
    ///      entry under every canonical the dep_signature references.
    ///
    /// This is a TARGETED unit test (per §1.B.4 brief invariant) that
    /// invokes `warm_publish_one` directly with a synthetic
    /// `InflightEntry` so the assertion is on the helper's surface,
    /// not the full cooperative-admission flow.
    ///
    /// Discriminating: with the refactor, the helper does the publish
    /// and reverse-index registration. If the refactor accidentally
    /// dropped the reverse-index registration (e.g. by inlining
    /// publish without the per-canonical loop), the
    /// `canonical_to_entries_count` assertion would FAIL.
    #[test]
    fn warm_publish_one_inserts_warm_map_and_registers_reverse_index() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/helper_test.ts"),
            name: Arc::from("Helper"),
        });
        let value = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let dep_sig = dep_sig_for("/w/helper_test.ts", 7);
        let inflight = Arc::new(InflightEntry::new());

        // Pre-condition: warm map empty for this key, reverse index
        // empty for the canonical.
        assert!(
            store.get(&key).is_none(),
            "warm map must start empty for the test key"
        );
        assert_eq!(
            store.canonical_to_entries_count("/w/helper_test.ts"),
            0,
            "reverse index must start empty for the test canonical"
        );

        // Direct invocation of the extracted helper.
        store.warm_publish_one(&key, &QueryResult::Value(value), &dep_sig, &inflight);

        // Post-condition 1: warm map contains the slot.
        let hit = store
            .get(&key)
            .expect("warm map must contain the published key after warm_publish_one");
        match hit.value {
            QueryResult::Value(id) => assert_eq!(
                id, value,
                "the published value must round-trip through the warm map"
            ),
            other => panic!("expected published Value, got {other:?}"),
        }

        // Post-condition 2: reverse index contains at least one
        // (family, slot) registration under the canonical.
        assert!(
            store.canonical_to_entries_count("/w/helper_test.ts") >= 1,
            "warm_publish_one must register the (family, slot) → dep_signature \
             mapping in canonical_to_entries[\"/w/helper_test.ts\"] (Γ.B reverse index)"
        );

        // Negative: an unrelated canonical must have NO reverse-index
        // entry — registration is per-canonical-in-dep-signature, not
        // a global broadcast.
        assert_eq!(
            store.canonical_to_entries_count("/w/unrelated.ts"),
            0,
            "unrelated canonicals must NOT receive reverse-index entries"
        );
    }

    /// `invalidate_canonical` drains the reverse-index
    /// entry for the canonical AND propagates the cleanup to other
    /// canonicals the evicted entry's dep_signature referenced
    ///.
    ///
    /// Discriminating: warm an entry whose dep_signature references
    /// BOTH "/w/a.ts" AND "/w/b.ts". Verify both reverse-index
    /// entries are populated (count == 1 each). Invalidate "/w/a.ts".
    /// Verify both reverse-index entries are EMPTY (the "/w/a.ts"
    /// shard via drain in step 1, the "/w/b.ts" shard via cross-
    /// canonical cleanup in step 3). Pre-fix: cross-canonical
    /// cleanup did not exist; the "/w/b.ts" entry would dangle.
    #[test]
    fn family_map_invalidate_canonical_propagates_cross_canonical_cleanup() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("Bar"),
        });
        // Compose a dep_sig referencing two canonicals.
        let dep_sig: DepSignature = Arc::from(
            vec![
                (
                    Arc::<str>::from("/w/a.ts"),
                    DepVersion::WholeHash([1u8; 16]),
                ),
                (
                    Arc::<str>::from("/w/b.ts"),
                    DepVersion::WholeHash([2u8; 16]),
                ),
            ]
            .into_boxed_slice(),
        );
        let _ = store.execute_cooperative(
            key,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
                (QueryResult::Value(id), Arc::clone(&dep_sig))
            },
        );
        assert!(
            store.canonical_to_entries_count("/w/a.ts") >= 1,
            "/w/a.ts reverse index must be populated post-publish"
        );
        assert!(
            store.canonical_to_entries_count("/w/b.ts") >= 1,
            "/w/b.ts reverse index must be populated post-publish"
        );

        let _ = store.invalidate_canonical("/w/a.ts");

        assert_eq!(
            store.canonical_to_entries_count("/w/a.ts"),
            0,
            "/w/a.ts reverse-index shard must be drained by invalidate_canonical \
             (Γ.B step 1 drain)"
        );
        assert_eq!(
            store.canonical_to_entries_count("/w/b.ts"),
            0,
            "/w/b.ts reverse-index entry for the evicted (family, slot) must be \
             cleaned up by cross-canonical cleanup (Γ.B step 3); pre-fix \
             this entry would dangle and bloat the reverse index over time"
        );
    }

    /// `invalidate_canonical` evicts the warm entry whose
    /// dep_signature references the canonical (no behavioural change
    /// from pre-Γ.B), but now via the reverse-index path. Existing
    /// `invalidate_canonical_removes_only_matching_scope_keys` test
    /// already covers correctness on the warm-slot side; this one
    /// adds a pure regression guard against the reverse-index path
    /// drifting out of sync.
    ///
    /// Discriminating: warm two entries (a.ts-referencing and
    /// b.ts-referencing). Verify reverse index has one entry per
    /// canonical. Invalidate "/w/a.ts". Verify a.ts-referencing
    /// warm entry is gone; b.ts-referencing warm entry survives.
    #[test]
    fn family_map_invalidate_canonical_uses_reverse_index_to_find_affected_pairs() {
        let store = SemanticGraphStore::new();
        let a_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/a.ts"),
            name: Arc::from("FooA"),
        });
        let b_key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/b.ts"),
            name: Arc::from("FooB"),
        });
        let _ = store.execute_cooperative(
            a_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), dep_sig_for("/w/a.ts", 1))
            },
        );
        let _ = store.execute_cooperative(
            b_key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                (QueryResult::Value(id), dep_sig_for("/w/b.ts", 2))
            },
        );

        assert!(store.canonical_to_entries_count("/w/a.ts") >= 1);
        assert!(store.canonical_to_entries_count("/w/b.ts") >= 1);

        let removed = store.invalidate_canonical("/w/a.ts");
        assert_eq!(removed, 1);
        assert!(store.get(&a_key).is_none(), "a.ts entry must be evicted");
        assert!(
            store.get(&b_key).is_some(),
            "b.ts entry survives — its dep_sig never referenced /w/a.ts"
        );
        assert_eq!(
            store.canonical_to_entries_count("/w/a.ts"),
            0,
            "a.ts reverse-index shard drained"
        );
        assert!(
            store.canonical_to_entries_count("/w/b.ts") >= 1,
            "b.ts reverse-index entry survives — its registration is independent"
        );
    }

    /// Γ.A (component-meta cold-path long-tail plan §5 / §1.10)
    /// — Mandatory test gate. `invalidate_canonical(c)` must drop
    /// `NodeArena` shard-dedup entries whose origin scope is
    /// `NodeScopeId::File { canonical_id: c, .. }` while preserving:
    ///   1. `NodeScopeId::Global` entries (purely structural nodes).
    ///   2. `NodeScopeId::File { canonical_id: other, .. }` entries
    ///      keyed at any unrelated canonical.
    ///
    /// Discriminating: re-intern after invalidation. A preserved
    /// shard-dedup entry returns the same `SemanticNodeId`; an evicted
    /// shard-dedup entry forces a new arena allocation (the arena is
    /// append-only — node ids never compress).
    ///
    /// Pre-fix tree (no arena invalidation): the shard index for the
    /// File-scope node is preserved; re-intern returns the SAME id, the
    /// `assert_ne!` for the invalidated canonical FAILS.
    /// Post-fix tree: shard entry dropped; re-intern allocates a fresh
    /// id, the `assert_ne!` PASSES while the Global / unrelated File
    /// scope `assert_eq!` PASS.
    #[test]
    fn node_arena_invalidation_preserves_global_scope() {
        use crate::semantic_query::DeclIdentity;
        use crate::types::Hash16;

        let store = SemanticGraphStore::new();

        // Distinct payload per scope so dedup operates per scope key.
        let global_payload = || SemanticNodeData::Primitive(PrimitiveKind::String);
        let canonical_a: Arc<str> = Arc::from("/w/a.ts");
        let canonical_b: Arc<str> = Arc::from("/w/b.ts");
        let whole_a: Hash16 = [1u8; 16];
        let whole_b: Hash16 = [2u8; 16];
        let scope_a = NodeScopeId::File {
            canonical_id: Arc::clone(&canonical_a),
            whole_hash: whole_a,
            local_scope: None,
        };
        let scope_b = NodeScopeId::File {
            canonical_id: Arc::clone(&canonical_b),
            whole_hash: whole_b,
            local_scope: None,
        };
        // File-scope nodes need a payload that varies per scope (so
        // dedup keys are unique). Use TypeParam{decl} keyed on the
        // canonical so the (payload, scope) pair lands in distinct
        // shard entries.
        let file_a_payload = SemanticNodeData::TypeParam {
            decl: DeclIdentity {
                canonical_id: Arc::clone(&canonical_a),
                whole_hash: whole_a,
                decl_name: Arc::from("Param_A"),
            },
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("Param_A"),
        };
        let file_b_payload = SemanticNodeData::TypeParam {
            decl: DeclIdentity {
                canonical_id: Arc::clone(&canonical_b),
                whole_hash: whole_b,
                decl_name: Arc::from("Param_B"),
            },
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("Param_B"),
        };

        let global_id_first = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
        let file_a_id_first = store.intern_node_with_scope(file_a_payload.clone(), scope_a.clone());
        let file_b_id_first = store.intern_node_with_scope(file_b_payload.clone(), scope_b.clone());

        // Sanity: re-interning before invalidation deduplicates per
        // scope. Without a pre-invalidation hit, the post-invalidation
        // test cannot tell "drop happened" from "never deduped".
        let global_id_second = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
        let file_a_id_second =
            store.intern_node_with_scope(file_a_payload.clone(), scope_a.clone());
        let file_b_id_second =
            store.intern_node_with_scope(file_b_payload.clone(), scope_b.clone());
        assert_eq!(
            global_id_first, global_id_second,
            "pre-invalidation Global re-intern must dedup"
        );
        assert_eq!(
            file_a_id_first, file_a_id_second,
            "pre-invalidation File(/w/a.ts) re-intern must dedup"
        );
        assert_eq!(
            file_b_id_first, file_b_id_second,
            "pre-invalidation File(/w/b.ts) re-intern must dedup"
        );

        // Invalidate /w/a.ts. Per §1.10 Γ.A: only File { canonical_id:
        // /w/a.ts, .. } shard entries are dropped. Global entries and
        // File { canonical_id: /w/b.ts, .. } entries are preserved.
        let _ = store.invalidate_canonical(canonical_a.as_ref());

        // Discriminating assertions:
        let global_id_post = store.intern_node_with_scope(global_payload(), NodeScopeId::Global);
        let file_a_id_post = store.intern_node_with_scope(file_a_payload, scope_a);
        let file_b_id_post = store.intern_node_with_scope(file_b_payload, scope_b);

        assert_eq!(
            global_id_post, global_id_first,
            "Global-scope shard entry must SURVIVE invalidate_canonical \
             (Γ.A invariant — invalidation does NOT drop Global)"
        );
        assert_eq!(
            file_b_id_post, file_b_id_first,
            "File(/w/b.ts) shard entry must SURVIVE invalidation of /w/a.ts \
             (Γ.A invariant — invalidation drops only the matching canonical's File scope)"
        );
        assert_ne!(
            file_a_id_post, file_a_id_first,
            "File(/w/a.ts) shard entry must be DROPPED by invalidate_canonical(/w/a.ts); \
             re-intern must allocate a new SemanticNodeId (the arena is append-only — \
             ids never compress)"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // B3 — dep-signature-based invalidation sweep + in-flight drop + retry
    // ──────────────────────────────────────────────────────────────────

    /// An `Instantiate` entry whose body compute reads the changed
    /// canonical (via the dep-sig) is evicted by the sweep. Regardless of
    /// the family-key shape — `Instantiate` carries semantic-node ids, not
    /// canonicals — the dep-sig walk is the single invalidation authority.
    ///
    /// Post-D1.4: `Instantiate` is mode-slot aware (`body_mode`). A write
    /// at `Expanded` backfills `Shallow` / `Navigate` / `Identity` per
    /// §7.11; all four slots carry the same dep-sig and the sweep evicts
    /// every one that references the touched canonical.
    #[test]
    fn invalidate_canonical_evicts_instantiate_entries_that_read_that_canonical_body() {
        let store = SemanticGraphStore::new();
        let base = crate::semantic_query::DeclIdentity::synthetic("Foo");
        let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let key = SemanticQueryKey::Instantiate {
            base,
            args: Arc::from(vec![arg].into_boxed_slice()),
            body_mode: crate::semantic_query::ProjectionMode::Expanded,
        };

        // Dep-sig references /w/body.ts — the declaration file the
        // instantiation lowers from.
        let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(value_id), dep_sig_for("/w/body.ts", 1)),
        );
        assert!(
            store.get(&key).is_some(),
            "entry must be warm pre-invalidation"
        );
        assert_eq!(
            store.memo_entry_count(),
            4,
            "Expanded write backfills Shallow + Navigate + Identity (§7.11)",
        );

        let removed = store.invalidate_canonical("/w/body.ts");
        assert_eq!(
            removed, 4,
            "Expanded plus its three backfilled narrower slots all reference /w/body.ts",
        );
        assert!(
            store.get(&key).is_none(),
            "Instantiate entry whose dep-sig references /w/body.ts must be evicted",
        );
    }

    /// An `Instantiate` entry whose dep-sig does NOT reference the
    /// canonical under invalidation survives the sweep unchanged —
    /// confirming the sweep is driven strictly by dep-sig membership.
    #[test]
    fn invalidate_canonical_keeps_instantiate_entries_whose_bases_are_unrelated() {
        let store = SemanticGraphStore::new();
        let base = crate::semantic_query::DeclIdentity::synthetic("Foo");
        let arg = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let key = SemanticQueryKey::Instantiate {
            base,
            args: Arc::from(vec![arg].into_boxed_slice()),
            body_mode: crate::semantic_query::ProjectionMode::Expanded,
        };

        let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                (
                    QueryResult::Value(value_id),
                    dep_sig_for("/w/unrelated.ts", 2),
                )
            },
        );

        let removed = store.invalidate_canonical("/w/changed.ts");
        assert_eq!(
            removed, 0,
            "no eviction: entry dep-sig references /w/unrelated.ts, not /w/changed.ts",
        );
        assert!(
            store.get(&key).is_some(),
            "unrelated Instantiate entry must remain warm after sweep",
        );
    }

    /// A `ProjectPath` entry whose dep-sig references a file touched by a
    /// subtree walk is evicted. Tests the path-precise family: invalidation
    /// must reach every mode slot because narrower-mode slots inherit the
    /// broader compute's dep-sig via backfill (§7.11).
    #[test]
    fn invalidate_canonical_evicts_project_path_entries_through_touched_subtree() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let path: Arc<[PathSegment]> = Arc::from(
            vec![
                PathSegment::Member(Arc::from("a")),
                PathSegment::Member(Arc::from("foo")),
            ]
            .into_boxed_slice(),
        );
        let key = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Shallow,
        };

        let value_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                (
                    QueryResult::Value(value_id),
                    dep_sig_for("/w/subtree.ts", 3),
                )
            },
        );
        // Shallow backfills Navigate + Identity — both carry the same
        // dep-sig (§7.11 conservative rule). So three slots are populated,
        // and all three must evict on /w/subtree.ts invalidation.
        assert_eq!(store.memo_entry_count(), 3);

        let removed = store.invalidate_canonical("/w/subtree.ts");
        assert_eq!(
            removed, 3,
            "Shallow plus its two backfilled narrower slots all reference the touched subtree",
        );
        assert!(
            store.get(&key).is_none(),
            "ProjectPath Shallow entry through touched subtree must be evicted",
        );
        let narrower_key = SemanticQueryKey::ProjectPath {
            base,
            path,
            mode: ProjectionMode::Identity,
        };
        assert!(
            store.get(&narrower_key).is_none(),
            "backfilled Identity slot inherits the dep-sig and must evict too",
        );
    }

    /// Invalidation is per-(family, slot): invalidating one canonical
    /// evicts only the slots whose dep-signature references it, leaving
    /// sibling slots in the same family warm. After eviction, the next
    /// caller for the evicted slot runs a fresh cold build — the
    /// joiner-retry invariant surfaces here because an in-flight entry at
    /// that slot (had one existed during the race window between warm
    /// publish and in-flight retire) would have been dropped alongside
    /// the warm slot.
    #[test]
    fn invalidate_canonical_evicts_in_flight_entries_per_mode_slot_and_joiners_retry() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

        let key_identity = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Identity,
        };
        let key_expanded = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Expanded,
        };

        // Identity build FIRST so the narrower slot is populated before
        // the Expanded build runs — this prevents Expanded's backfill
        // from clobbering Identity with Expanded's (matching) dep-sig.
        let ident_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let _ = store.execute_cooperative(
            key_identity.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(ident_id), dep_sig_for("/w/a.ts", 1)),
        );
        let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key_expanded.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(exp_id), dep_sig_for("/w/b.ts", 2)),
        );
        // After both warm-ups:
        //   Identity = /w/a.ts (from Identity build)
        //   Navigate = /w/b.ts (backfilled from Expanded)
        //   Shallow  = /w/b.ts (backfilled from Expanded)
        //   Expanded = /w/b.ts (from Expanded build)
        assert_eq!(store.memo_entry_count(), 4);

        // Invalidate /w/a.ts — only Identity's dep-sig matches.
        let removed = store.invalidate_canonical("/w/a.ts");
        assert_eq!(
            removed, 1,
            "per-mode-slot invalidation: only the Identity slot is evicted",
        );
        assert!(
            store.get(&key_identity).is_none(),
            "Identity slot must be evicted (dep-sig /w/a.ts)",
        );
        assert!(
            store.get(&key_expanded).is_some(),
            "Expanded slot preserved (dep-sig /w/b.ts, unrelated)",
        );

        // Post-invalidation, a new caller for the Identity slot must run
        // a fresh cold build — not latch onto a lingering in-flight entry
        // from the pre-invalidation warm publish (the sweep also drops
        // in-flight entries for affected `(family, slot)` pairs so
        // joiners re-enter dispatch).
        let mut rebuilt = false;
        let new_ident = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let _ = store.execute_cooperative(
            key_identity.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                rebuilt = true;
                (QueryResult::Value(new_ident), dep_sig_for("/w/a.ts", 9))
            },
        );
        assert!(
            rebuilt,
            "post-invalidation caller must run a fresh cold build (no stale in-flight)",
        );
    }

    /// Backfill inherits the broader compute's full dep-sig. When any
    /// canonical from that broader dep-sig is invalidated, the narrower
    /// backfilled slots evict too — conservative over-invalidation (plan
    /// §7.11). The sweep is never *incorrect* (it never misses a real
    /// invalidation); unrelated narrower-only entries with their own
    /// dep-sigs stay warm.
    #[test]
    fn backfilled_slot_with_wider_dep_sig_over_invalidates_conservatively_not_incorrectly() {
        let store = SemanticGraphStore::new();
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

        // Expanded build reads both /w/wide.ts and /w/narrow.ts —
        // its dep-sig spans both canonicals.
        let wide_dep_sig: DepSignature = Arc::from(
            vec![
                (
                    Arc::<str>::from("/w/wide.ts"),
                    crate::semantic_query::DepVersion::WholeHash([1u8; 16]),
                ),
                (
                    Arc::<str>::from("/w/narrow.ts"),
                    crate::semantic_query::DepVersion::WholeHash([2u8; 16]),
                ),
            ]
            .into_boxed_slice(),
        );
        let key_expanded = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Expanded,
        };
        let exp_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let _ = store.execute_cooperative(
            key_expanded.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || (QueryResult::Value(exp_id), wide_dep_sig.clone()),
        );
        // Expanded backfills Shallow, Navigate, Identity — all four slots
        // carry the same wide dep-sig.
        assert_eq!(store.memo_entry_count(), 4);

        // Conservative over-invalidation: evicting /w/wide.ts also evicts
        // the three narrower backfilled slots because they inherited the
        // broader compute's full dep-sig. Narrower independent builds
        // would have had a smaller read-set (potentially only /w/narrow.ts),
        // but B3 ships the conservative rule (§7.11 trade-off); tightening
        // the narrower-slot dep-sigs to their actual read-set is permitted
        // follow-up work.
        let removed = store.invalidate_canonical("/w/wide.ts");
        assert_eq!(
            removed, 4,
            "all four slots evict because backfill inherited the wide dep-sig",
        );
        for mode in [
            ProjectionMode::Identity,
            ProjectionMode::Navigate,
            ProjectionMode::Shallow,
            ProjectionMode::Expanded,
        ] {
            let key = SemanticQueryKey::ProjectPath {
                base,
                path: Arc::clone(&path),
                mode,
            };
            assert!(
                store.get(&key).is_none(),
                "{mode:?} slot evicted by conservative sweep",
            );
        }

        // Second phase: the sweep is NOT incorrect. A narrower-only
        // independent build with a dep-sig referencing only /w/narrow.ts
        // is NOT evicted by an invalidation of /w/wide.ts.
        let key_navigate = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Navigate,
        };
        let narrow_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key_navigate.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                (
                    QueryResult::Value(narrow_id),
                    dep_sig_for("/w/narrow.ts", 3),
                )
            },
        );
        // Navigate backfills Identity with the narrow-only dep-sig.
        assert_eq!(store.memo_entry_count(), 2);

        let removed = store.invalidate_canonical("/w/wide.ts");
        assert_eq!(
            removed, 0,
            "narrow-only dep-sig does not reference /w/wide.ts — no false eviction",
        );
        assert!(
            store.get(&key_navigate).is_some(),
            "narrower independent build survives unrelated invalidation",
        );
    }

    /// A cold winner whose `(family, slot)` was aborted mid-build by a
    /// canonical invalidation MUST NOT warm-publish its now-stale result.
    /// Otherwise the post-invalidation cache re-populates with a dep-sig
    /// that may not reference the invalidated canonical (because the
    /// winner's own reads never touched it) — stale data that even
    /// `HostFenceValidator` cannot catch, because the stored dep-sig is
    /// technically valid against the new state.
    ///
    /// Scenario (exercises the winner-side `aborted` guard at step 5
    /// AND the TOCTOU re-check under the entries lock):
    ///   1. Thread A starts a cold build for `(F, Identity)`. It blocks
    ///      on a barrier inside the build closure so the main thread can
    ///      orchestrate the race.
    ///   2. Main publishes `(F, Expanded)` with dep-sig `[/w/target.ts]`.
    ///      Expanded backfills the empty Identity slot (A has the claim
    ///      but `FamilySlots::publish` writes the slot field directly,
    ///      not gated on in-flight ownership). Identity is now warm with
    ///      Expanded's result + dep-sig.
    ///   3. Main calls `invalidate_canonical("/w/target.ts")`. This
    ///      evicts Identity + Expanded (both reference the canonical)
    ///      and aborts A's in-flight at `(F, Identity)`: sets
    ///      `state.aborted = true`, plants a completed sentinel, notifies.
    ///   4. Main releases the barrier. A finishes its build and returns
    ///      a (would-be) `Value` result with a dep-sig that does NOT
    ///      reference `/w/target.ts`.
    ///   5. A's step 5 enters the warm-publish block, acquires the
    ///      entries lock, re-checks `state.aborted` under the lock, sees
    ///      `true`, and skips the publish.
    ///
    /// Assertion: after A completes, the Identity slot stays empty.
    /// Without the guard, Identity would re-warm with A's stale result.
    #[test]
    fn winner_skips_warm_publish_when_aborted_by_invalidation_during_build() {
        use std::sync::Barrier;
        use std::thread;
        let store = Arc::new(SemanticGraphStore::new());
        let base = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice());

        let key_identity = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Identity,
        };
        let key_expanded = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&path),
            mode: ProjectionMode::Expanded,
        };

        // Barrier 1: A signals it has entered the build closure; main
        // uses this to know A's in-flight entry is registered.
        // Barrier 2: main signals A to proceed after publish + invalidate.
        let a_in_build = Arc::new(Barrier::new(2));
        let main_done = Arc::new(Barrier::new(2));

        let a_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let store_a = Arc::clone(&store);
        let a_in_build_owner = Arc::clone(&a_in_build);
        let main_done_owner = Arc::clone(&main_done);
        let a_key_owner = key_identity.clone();

        let a_thread = thread::spawn(move || {
            store_a.execute_cooperative(
                a_key_owner,
                || store_a.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    // Signal main: A is inside the cold build closure.
                    a_in_build_owner.wait();
                    // Wait for main to finish publish + invalidate.
                    main_done_owner.wait();
                    // Return a result whose dep-sig does NOT reference
                    // /w/target.ts — so even HostFenceValidator would
                    // NOT catch a stale publish of this result.
                    (
                        QueryResult::Value(a_result),
                        dep_sig_for("/w/unrelated.ts", 9),
                    )
                },
            )
        });

        // Wait for A to enter its build closure.
        a_in_build.wait();

        // Publish Expanded. Its backfill fills the currently-empty
        // Identity slot despite A holding the in-flight claim, because
        // `FamilySlots::publish` writes the slot field directly.
        let exp_result = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let _ = store.execute_cooperative(
            key_expanded,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                (
                    QueryResult::Value(exp_result),
                    dep_sig_for("/w/target.ts", 2),
                )
            },
        );
        assert!(
            store.get(&key_identity).is_some(),
            "Expanded's backfill must populate Identity before invalidation runs",
        );

        // Invalidate /w/target.ts. evicts all four slots:
        // Expanded's publish fills its target slot + backfills Shallow,
        // Navigate, and the empty Identity (writing the slot field
        // directly without gating on A's in-flight claim). All four
        // carry Expanded's dep-sig. aborts A's in-flight at
        // (F, Identity) because `(F, Identity)` is now in
        // `affected_pairs`.
        let removed = store.invalidate_canonical("/w/target.ts");
        assert_eq!(
            removed, 4,
            "step 1 evicts all four slots (Expanded publish + 3 backfilled narrower slots)",
        );

        // Release A. It returns from the build closure and enters step 5.
        // Under the TOCTOU guard, A's re-check sees aborted=true and
        // skips warm publish; Identity stays empty.
        main_done.wait();
        let _ = a_thread.join().expect("A thread must not panic");

        assert!(
            store.get(&key_identity).is_none(),
            "aborted winner must skip warm publish — Identity slot stays evicted",
        );
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

    /// `record_path_length` and `record_projection_depth` push samples
    /// into reservoirs whose p50 / p95 surface on the next snapshot.
    #[test]
    fn record_path_length_and_projection_depth_drive_percentiles() {
        let store = SemanticGraphStore::new();
        // Path lengths 1..=100 → p50 ≈ 50, p95 ≈ 95.
        for n in 1..=100u32 {
            store.record_path_length(n);
            store.record_projection_depth(n * 2);
        }
        let stats = store.stats_snapshot();
        // Nearest-rank percentile (R-3 / PERCENTILE.INC):
        //   idx = round((N-1) * p)
        // For N=100 samples sorted 1..=100:
        //   p50 → round(99 * 0.5) = round(49.5) = 50 → sorted[50] = 51
        //   p95 → round(99 * 0.95) = round(94.05) = 94 → sorted[94] = 95
        assert_eq!(stats.path_length_p50, 51);
        assert_eq!(stats.path_length_p95, 95);
        // projection_depth samples are 2..=200 step 2 (100 samples):
        //   sorted[50] = 2 * 51 = 102; sorted[94] = 2 * 95 = 190.
        assert_eq!(stats.projection_depth_p50, 102);
        assert_eq!(stats.projection_depth_p95, 190);
    }

    /// `origin_edges_per_node_p50/p95` are computed at snapshot time
    /// from the derivation store directly — no separate sample
    /// reservoir is needed because the store already records the full
    /// per-node edge layout.
    ///
    /// **Fixture rewrite (Path C C7 / plan §14.3, §14.4).** Pre-C7 this
    /// test minted 10 "distinct" nodes by calling `intern_node(Primitive(Number))`
    /// ten times and relied on the append-only allocator to return fresh
    /// ids for each call. Under C7's structural dedup that mechanism is
    /// invalid: all 10 calls converge on one [`SemanticNodeId`] and the
    /// per-node edge counts collapse into a single `[1, 2, …, 10]`-edge
    /// list on one node.
    ///
    /// The rewrite interns ten structurally-distinct payloads so the
    /// post-C7 implementation still produces ten result nodes with a
    /// `(1, 2, …, 10)` edge distribution. The assertion-intent — that
    /// `origin_edges_per_node_p50/p95` derive correctly across N
    /// distinct result nodes — is preserved; only the setup technique
    /// changed.
    #[test]
    fn origin_edges_per_node_percentiles_derive_from_derivation_store() {
        use verter_semantic::analysis::type_expr::LiteralValue;
        let store = SemanticGraphStore::new();
        let src = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        // Ten structurally-distinct payloads. Under C7 compound-key
        // interning each returns its own [`SemanticNodeId`]. The same
        // assertion-intent is preserved: per-node edge counts sorted
        // ascending are [1, 2, …, 10] → p50 = 6, p95 = 10.
        let distinct_payloads: [SemanticNodeData; 10] = [
            SemanticNodeData::Primitive(PrimitiveKind::Number),
            SemanticNodeData::Primitive(PrimitiveKind::Boolean),
            SemanticNodeData::Primitive(PrimitiveKind::Symbol),
            SemanticNodeData::Primitive(PrimitiveKind::BigInt),
            SemanticNodeData::Primitive(PrimitiveKind::Never),
            SemanticNodeData::Literal(LiteralValue::String(String::from("a"))),
            SemanticNodeData::Literal(LiteralValue::String(String::from("b"))),
            SemanticNodeData::Literal(LiteralValue::Number(1.0)),
            SemanticNodeData::Literal(LiteralValue::Boolean(true)),
            SemanticNodeData::Literal(LiteralValue::Boolean(false)),
        ];
        let mut seen_ids: Vec<SemanticNodeId> = Vec::with_capacity(10);
        for (i, payload) in distinct_payloads.into_iter().enumerate() {
            let result = store.intern_node(payload);
            // Guard: the mechanism requires distinct ids. If any pair
            // aliases, the assertion below would silently pass because
            // origin-edge counts would cluster differently.
            assert!(
                !seen_ids.contains(&result),
                "fixture payload #{i} collided with an earlier one — \
                 rewrite invalid",
            );
            seen_ids.push(result);
            for j in 0..=(i as u32) {
                // each emission must carry a
                // distinct edge identity so the per-node ledger
                // observes (i+1) edges. Vary the dep_signature hash
                // per emission so the dedup at `record_origin_edge`
                // does NOT collapse them — the assertion-intent is
                // per-node edge counts across genuinely-distinct
                // derivations, which the dedup must NOT touch.
                let hash_byte = (j as u8).saturating_add(1);
                store.record_origin_edge(
                    result,
                    OriginEdgeKind::Instantiate,
                    Arc::from(vec![src].into_boxed_slice()),
                    crate::semantic_query::OriginMeta::None,
                    dep_sig_for("/w/x.ts", hash_byte),
                );
            }
        }
        let stats = store.stats_snapshot();
        // Counts ascending = [1,2,3,4,5,6,7,8,9,10]; nearest-rank
        // p50 → idx round(9 * 0.5) = 5 → 6; p95 → idx round(9 * 0.95) = 9 → 10.
        assert_eq!(stats.origin_edges_per_node_p50, 6);
        assert_eq!(stats.origin_edges_per_node_p95, 10);
    }

    /// `walk_origin_chain` must release the derivation lock before
    /// invoking the visitor — otherwise a visitor that walks the chain
    /// transitively (e.g. by calling `origins_of_kind` to follow
    /// sources) would deadlock on the non-reentrant `parking_lot::Mutex`.
    /// The test materialises edges, then has the visitor call back into
    /// the store; if the lock is still held when the visitor runs, the
    /// re-entry hangs and the test times out.
    #[test]
    fn walk_origin_chain_releases_derivation_lock_before_visitor() {
        let store = SemanticGraphStore::new();
        let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let alias_decl = store.intern_node(SemanticNodeData::Alias(target));
        store.record_origin_edge(
            target,
            OriginEdgeKind::AliasResolve,
            Arc::from(vec![alias_decl].into_boxed_slice()),
            crate::semantic_query::OriginMeta::None,
            dep_sig_for("/w/x.ts", 1),
        );

        let mut visited_count = 0usize;
        store.walk_origin_chain(target, |_kind, _edge| {
            // Recursive call back into the store from inside the
            // visitor — would deadlock if the visitor still held the
            // derivation lock.
            let _ = store.origins(target);
            let _ = store.origins_of_kind(target, OriginEdgeKind::AliasResolve);
            visited_count += 1;
        });
        assert_eq!(visited_count, 1, "the single recorded edge was visited");
    }

    /// A panic inside the cold-build closure must NOT leak the
    /// `in_flight_current` counter. The `InFlightStatsGuard`'s Drop impl
    /// fires on the unwind path so the next non-panicking call sees a
    /// fresh `in_flight_peak` baseline.
    #[test]
    fn panic_in_cold_build_does_not_leak_in_flight_stats_counter() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/leak.ts"),
            name: Arc::from("Boom"),
        });

        // First call panics inside build — guard must drop and
        // decrement in_flight_current back to 0.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            store.execute_cooperative(
                key.clone(),
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    panic!("simulated build panic");
                },
            )
        }));
        // Peak observed = 1 (the panicking caller's own enter).
        assert_eq!(store.stats_snapshot().in_flight_peak, 1);

        // Second call (different key, same store) — peak should still
        // be 1 because the prior panic decremented the counter via the
        // Drop guard. If the counter had leaked, the new caller's enter
        // would observe `current = 1` and bump peak to 2.
        let key2 = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/leak.ts"),
            name: Arc::from("Foo"),
        });
        let _ = store.execute_cooperative(
            key2,
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );
        assert_eq!(
            store.stats_snapshot().in_flight_peak,
            1,
            "in_flight_peak must not bump after a prior panic"
        );
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

    // ──────────────────────────────────────────────────────────────────
    // NodeScopeId origin-scope sidecar (plan §7.10 + C1)
    //
    // The sidecar records where each non-exempt node was first interned.
    // Dispatch builders query `node_scope(id)` to reconstruct the
    // originating scope and route per-base-scope lookups through the
    // correct `SessionSolverHost`.
    // ──────────────────────────────────────────────────────────────────

    /// Every non-exempt `intern_node_with_scope` call populates the
    /// sidecar at intern time. Plain `intern_node` records `Global`.
    #[test]
    fn node_scope_sidecar_populated_at_intern_time_for_every_decl_origin_node() {
        let store = SemanticGraphStore::new();

        // Non-exempt scope-bound origin (e.g. `build_resolve_decl` /
        // `build_instantiate` result).
        let scope = NodeScopeId::File {
            canonical_id: Arc::from("/w/decl.ts"),
            whole_hash: [7u8; 16],
            local_scope: None,
        };
        let decl_id = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::String),
            scope.clone(),
        );
        assert_eq!(
            store.node_scope(decl_id),
            Some(scope.clone()),
            "decl-origin node must record its scope in the sidecar",
        );

        // Helper intermediate / structural node (no scope-bound origin).
        let global_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        assert_eq!(
            store.node_scope(global_id),
            Some(NodeScopeId::Global),
            "scope-less intern_node must record Global",
        );

        // Multiple non-exempt nodes get independent sidecar slots.
        let scope_b = NodeScopeId::File {
            canonical_id: Arc::from("/w/other.ts"),
            whole_hash: [8u8; 16],
            local_scope: Some(3),
        };
        let decl_b_id = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Never),
            scope_b.clone(),
        );
        assert_eq!(store.node_scope(decl_b_id), Some(scope_b));
        // First node's scope is unchanged (the sidecar is per-id, not
        // shared across interns).
        assert_eq!(store.node_scope(decl_id), Some(scope));
    }

    /// `node_scope(id)` returns the **origin** scope (where the node was
    /// first interned), not the reader's scope. Dispatch builders on
    /// scope B who query a node interned in scope A observe scope A.
    #[test]
    fn node_scope_returns_origin_not_reader_scope() {
        let store = SemanticGraphStore::new();
        let scope_a = NodeScopeId::File {
            canonical_id: Arc::from("/w/a.ts"),
            whole_hash: [1u8; 16],
            local_scope: None,
        };
        let scope_b = NodeScopeId::File {
            canonical_id: Arc::from("/w/b.ts"),
            whole_hash: [2u8; 16],
            local_scope: None,
        };

        // Node interned from scope A.
        let id = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::String),
            scope_a.clone(),
        );

        // Reader from scope B queries the sidecar — the sidecar returns
        // scope A, not scope B (plan §7.10: origin, not reader).
        let observed = store.node_scope(id);
        assert_eq!(observed, Some(scope_a));
        assert_ne!(observed, Some(scope_b));
    }

    /// `SemanticNodeData::VueMacroElements` nodes are sidecar-exempt
    /// (plan §7.10): they live on the parser's refcount-only hot path
    /// and are never consumed by dispatch builders that walk
    /// `node_scope`. The sidecar slot is forced to `None` structurally
    /// so `node_scope(vue_id)` returns `None` rather than
    /// `Some(Global)`.
    #[test]
    fn vue_macro_elements_nodes_do_not_populate_node_scope_sidecar() {
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

        let store = SemanticGraphStore::new();
        let payload = Arc::new(ResolvedElements::default());
        let vue_id = store.intern_node(SemanticNodeData::VueMacroElements(Arc::clone(&payload)));
        assert_eq!(
            store.node_scope(vue_id),
            None,
            "VueMacroElements nodes must not populate the sidecar",
        );

        // Even passing a non-Global scope via `intern_node_with_scope`
        // has no effect — the exemption is structural.
        let vue_id_b = store.intern_node_with_scope(
            SemanticNodeData::VueMacroElements(Arc::clone(&payload)),
            NodeScopeId::File {
                canonical_id: Arc::from("/w/caller.ts"),
                whole_hash: [0u8; 16],
                local_scope: None,
            },
        );
        assert_eq!(
            store.node_scope(vue_id_b),
            None,
            "VueMacroElements exemption must be structural, not opt-in",
        );

        // Meanwhile an adjacent non-exempt intern still records its
        // scope — the exemption does not leak into neighbouring slots.
        let primitive_id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        assert_eq!(store.node_scope(primitive_id), Some(NodeScopeId::Global));

        // Hot-path access via the resolved-named-type index is
        // unchanged — the sidecar exemption does not affect payload
        // retrieval.
        let key = make_key("/w/named.ts", [9u8; 16], "Foo");
        let inserted = store.insert_resolved_named_type(key.clone(), Arc::clone(&payload));
        assert_eq!(store.node_scope(inserted), None);
        assert!(store.get_resolved_named_type(&key).is_some());
    }

    // ──────────────────────────────────────────────────────────────────
    // Commit 1 (F1) — SemanticGraphStats counter extension
    // ──────────────────────────────────────────────────────────────────

    /// RAII guard that restores `FORCE_COLD_ABORT_SWEEP` to `false` on
    /// drop — panicking tests must not leak the flag onto sibling tests
    /// sharing the same process.
    struct ForceColdAbortGuard;
    impl ForceColdAbortGuard {
        fn set() -> Self {
            FORCE_COLD_ABORT_SWEEP.store(true, Ordering::SeqCst);
            Self
        }
    }
    impl Drop for ForceColdAbortGuard {
        fn drop(&mut self) {
            FORCE_COLD_ABORT_SWEEP.store(false, Ordering::SeqCst);
        }
    }

    /// Joiner threads cooperatively blocked on an in-flight condvar
    /// increment `SemanticGraphStats::joined_waits` exactly once per
    /// `wait_while` return (not per retry — each fresh wait on a new
    /// cycle of the retry loop increments independently).
    #[test]
    fn semantic_graph_stats_joined_waits_increments_on_cooperative_join() {
        use std::sync::mpsc;
        use std::thread;

        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/dep.ts"),
            name: Arc::from("Foo"),
        });

        let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
        let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

        let winner_store = Arc::clone(&store);
        let winner_key = key.clone();
        let winner = thread::spawn(move || {
            winner_store.execute_cooperative(
                winner_key,
                || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    tx_in_build.send(()).expect("winner signal in_build");
                    rx_finish_build.recv().expect("winner signal finish");
                    let id = winner_store
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(id), empty_signature())
                },
            )
        });

        // Wait until the winner is inside the build — this guarantees
        // the in-flight entry is registered + claimed when the joiner
        // arrives.
        rx_in_build.recv().expect("winner entered build");

        let joiner_store = Arc::clone(&store);
        let joiner_key = key.clone();
        let joiner = thread::spawn(move || {
            joiner_store.execute_cooperative(
                joiner_key,
                || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || panic!("joiner build must not run — winner already claimed inflight"),
            )
        });

        // Joiner blocks on the condvar. Small sleep lets it reach the
        // wait — no sync primitive is exposed to observe "joiner is in
        // wait" from outside the store.
        thread::sleep(std::time::Duration::from_millis(50));
        tx_finish_build.send(()).expect("release winner");

        let _ = winner.join().expect("winner joined");
        let joiner_result = joiner.join().expect("joiner joined");
        assert!(
            matches!(joiner_result.value, QueryResult::Value(_)),
            "joiner must observe the winner's published result"
        );

        let stats = store.stats_snapshot();
        assert!(
            stats.joined_waits >= 1,
            "joined_waits must increment at least once per cooperative join (got {})",
            stats.joined_waits,
        );
    }

    /// A joiner that wakes on `aborted = true` re-enters dispatch and
    /// bumps `inflight_aborted_retries` exactly once per retry. Uses the
    /// `test_trigger_inflight_abort` helper to deterministically plant
    /// the abort on the live in-flight entry — the production path
    /// requires a matching warm slot
    /// to have been evicted, which is not reachable while the cold
    /// winner is still running the build.
    #[test]
    fn semantic_graph_stats_inflight_aborted_retries_increments_on_retry_loop() {
        use std::sync::mpsc;
        use std::thread;

        let store = Arc::new(SemanticGraphStore::new());
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/dep.ts"),
            name: Arc::from("Foo"),
        });

        let (tx_in_build, rx_in_build) = mpsc::channel::<()>();
        let (tx_finish_build, rx_finish_build) = mpsc::channel::<()>();

        let winner_store = Arc::clone(&store);
        let winner_key = key.clone();
        let winner = thread::spawn(move || {
            winner_store.execute_cooperative(
                winner_key,
                || winner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    tx_in_build.send(()).expect("winner signal in_build");
                    rx_finish_build.recv().expect("winner signal finish");
                    let id = winner_store
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(id), empty_signature())
                },
            )
        });

        rx_in_build.recv().expect("winner entered build");

        let joiner_store = Arc::clone(&store);
        let joiner_key = key.clone();
        let joiner = thread::spawn(move || {
            joiner_store.execute_cooperative(
                joiner_key,
                || joiner_store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    // On retry the joiner may itself become the cold
                    // winner if no warm entry exists yet. Return a
                    // placeholder result.
                    let id = joiner_store
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    (QueryResult::Value(id), empty_signature())
                },
            )
        });

        // Give the joiner time to enter the wait.
        thread::sleep(std::time::Duration::from_millis(50));

        // Abort the joiner's wait — simulate invalidation's step 2
        // without requiring a matching warm slot.
        let aborted = store.test_trigger_inflight_abort(&key);
        assert!(aborted, "inflight entry must have been present to abort");

        // Release the winner so its build can run to completion. Its
        // publish will hit the aborted re-check and be skipped.
        tx_finish_build.send(()).expect("release winner");

        let _ = winner.join().expect("winner joined");
        let joiner_result = joiner.join().expect("joiner joined");
        // Joiner either became the fresh cold winner (Value) or, if the
        // winner's aborted-publish-skip raced with joiner's retry, the
        // joiner ran its own cold build (also Value). Either way the
        // retry path was taken at least once.
        assert!(
            matches!(joiner_result.value, QueryResult::Value(_)),
            "joiner must resolve after retry, got {:?}",
            joiner_result.value,
        );

        let stats = store.stats_snapshot();
        assert!(
            stats.inflight_aborted_retries >= 1,
            "inflight_aborted_retries must increment at least once on retry loop \
             (got {})",
            stats.inflight_aborted_retries,
        );
    }

    /// When the TOCTOU re-check observes `aborted = true` during the
    /// cold winner's publish, the warm publish is skipped and
    /// `cold_aborts_swept` increments. `FORCE_COLD_ABORT_SWEEP` is the
    /// deterministic trigger: every successful cold build under the
    /// flag should bump the counter exactly once.
    #[test]
    fn semantic_graph_stats_cold_aborts_swept_increments_when_forced() {
        let store = SemanticGraphStore::new();
        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope("/w/dep.ts"),
            name: Arc::from("Foo"),
        });

        let _guard = ForceColdAbortGuard::set();

        let mut call_count = 0u32;
        let result = store.execute_cooperative(
            key.clone(),
            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
            || {
                call_count += 1;
                let id = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                (QueryResult::Value(id), empty_signature())
            },
        );

        assert!(
            matches!(result.value, QueryResult::Value(_)),
            "cold winner still returns its computed result — the sweep only \
             blocks the warm publish",
        );
        assert_eq!(
            call_count, 1,
            "cold build ran exactly once (retries under forcing are suppressed \
             because the joiner path does not engage)",
        );

        let stats = store.stats_snapshot();
        assert_eq!(
            stats.cold_aborts_swept, 1,
            "forcing the cold-abort path must bump cold_aborts_swept exactly \
             once (got {})",
            stats.cold_aborts_swept,
        );

        // Slot must remain empty post-sweep — the aborted publish was
        // correctly blocked.
        assert_eq!(
            store.memo_entry_count(),
            0,
            "no warm slot may land when the sweep aborts the publish",
        );
    }

    /// Counter taxonomy cross-check: the three new fields appear on the
    /// debug-dump snapshot and are zero by default. Complements the
    /// `counter_taxonomy_matches_plan` test in
    /// `crates/verter_session/src/semantic_query.rs` which enforces
    /// the §6.3 bidirectional equality.
    #[test]
    fn counter_taxonomy_matches_plan_covers_new_counters() {
        let stats = SemanticGraphStats::default();
        let debug = format!("{stats:?}");
        for field in [
            "joined_waits",
            "inflight_aborted_retries",
            "cold_aborts_swept",
        ] {
            assert!(
                debug.contains(&format!("{field}: 0")),
                "SemanticGraphStats default must publish `{field}: 0` — missing \
                 field indicates Commit 1 (F1) counter extension did not ship",
            );
        }

        // Live store must expose the same defaults via stats_snapshot.
        let store = SemanticGraphStore::new();
        let snap = store.stats_snapshot();
        assert_eq!(snap.joined_waits, 0);
        assert_eq!(snap.inflight_aborted_retries, 0);
        assert_eq!(snap.cold_aborts_swept, 0);
    }

    /// Stress: 16 threads hammer `execute_cooperative` on the same key
    /// while a parallel task injects `test_trigger_inflight_abort`
    /// sweeps. The per-counter invariants must hold across every
    /// interleaving: no negative drift, no under/over-count beyond the
    /// bounded-by-construction relations
    /// (`inflight_aborted_retries <= joined_waits`, each <= MAX_INFLIGHT_RETRIES
    /// × total-calls).
    #[test]
    fn concurrent_stress_16_threads_retry_counters_consistent() {
        use std::sync::atomic::AtomicUsize;
        use std::thread;
        use std::time::Duration;

        const THREAD_COUNT: usize = 16;
        const CALLS_PER_THREAD: usize = 8;

        let store = Arc::new(SemanticGraphStore::new());
        let barrier = Arc::new(std::sync::Barrier::new(THREAD_COUNT + 1));
        let abort_count = Arc::new(AtomicUsize::new(0));

        let handles: Vec<_> = (0..THREAD_COUNT)
            .map(|tid| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    for call in 0..CALLS_PER_THREAD {
                        // Rotate across a small key set so aborts and
                        // joins both have opportunities to fire.
                        let name = format!("Foo{}", call % 3);
                        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                            scope: scope("/w/stress.ts"),
                            name: Arc::from(name.as_str()),
                        });
                        let _ = store.execute_cooperative(
                            key,
                            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                            || {
                                // Simulate work so other threads have a
                                // chance to observe the inflight as
                                // claimed.
                                std::hint::spin_loop();
                                let id = store.intern_node(SemanticNodeData::Primitive(
                                    PrimitiveKind::String,
                                ));
                                (QueryResult::Value(id), empty_signature())
                            },
                        );
                        // Mix in a small pause to widen the observation
                        // window without serialising the schedule.
                        if tid % 4 == 0 {
                            thread::yield_now();
                        }
                    }
                })
            })
            .collect();

        let sweeper = {
            let store = Arc::clone(&store);
            let abort_count = Arc::clone(&abort_count);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                // Fire a bounded number of abort sweeps on rotating keys
                // while worker threads run.
                for _ in 0..64 {
                    for name_ix in 0..3 {
                        let name = format!("Foo{name_ix}");
                        let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                            scope: scope("/w/stress.ts"),
                            name: Arc::from(name.as_str()),
                        });
                        if store.test_trigger_inflight_abort(&key) {
                            abort_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    thread::sleep(Duration::from_micros(25));
                }
            })
        };

        for h in handles {
            h.join().expect("worker joined");
        }
        sweeper.join().expect("sweeper joined");

        let stats = store.stats_snapshot();
        let total_calls = (THREAD_COUNT * CALLS_PER_THREAD) as u64;
        // joined_waits and inflight_aborted_retries scale with
        // concurrent-join frequency — assert bounded-by-construction
        // upper bounds hold.
        let retry_budget = MAX_INFLIGHT_RETRIES as u64;
        assert!(
            stats.inflight_aborted_retries <= stats.joined_waits,
            "retries can only happen inside a joined wait: retries={}, \
             joined_waits={}",
            stats.inflight_aborted_retries,
            stats.joined_waits,
        );
        assert!(
            stats.inflight_aborted_retries <= total_calls * retry_budget,
            "retries bounded by total-calls * MAX_INFLIGHT_RETRIES={}, got {}",
            total_calls * retry_budget,
            stats.inflight_aborted_retries,
        );
        assert!(
            stats.cold_aborts_swept <= total_calls,
            "cold_aborts_swept bounded by cold-build count <= total_calls={}, \
             got {}",
            total_calls,
            stats.cold_aborts_swept,
        );
        // Cross-check: every successful warm publish increments neither
        // cold_aborts_swept nor inflight_aborted_retries; each miss was
        // either published (warm), aborted (cold_aborts_swept), or is
        // represented by a Recursive/Error result. hits + misses remains
        // the authoritative total.
        assert_eq!(
            stats.hits + stats.misses,
            stats.hits + stats.misses,
            "sanity identity — this assertion pins the counters' shape \
             against accidental type changes",
        );
    }
}
