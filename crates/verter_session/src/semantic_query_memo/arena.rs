//! Node arena — structurally interning, sharded dedup, stable ids.
//!
//! The arena pairs each interned [`SemanticNodeData`] with an **origin-scope
//! sidecar**. Both the node vec and the parallel scope
//! vec live inside one `RwLock<ArenaInner>` so reads (`node_data`,
//! `node_scope`) are concurrent while writes (intern-miss) serialize.
//!
//! **Structural interning.** Two callers that construct the same
//! `SemanticNodeData::Primitive(Number)` in the same scope share one
//! [`SemanticNodeId`] — preventing the semantic graph from growing
//! unbounded under repeated structural construction. Cross-scope
//! same-payload interns stay distinct.
//!
//! **Sharded dedup index.** The dedup index lives on
//! `[Mutex<ShardIndex>; NUM_SHARDS]` rather than inside `ArenaInner`.
//! Payload hash + scope hash route to a specific shard; intern-hits
//! (the steady-state hot path) take only that shard's Mutex — so `K`
//! threads interning payloads that route to distinct shards proceed
//! in parallel. Intern-misses acquire the shard Mutex, then briefly
//! acquire `inner.write()` to allocate the next sequential id and
//! push the node. Storage stays global and dense so `id.0 as usize`
//! indexing + `a.0 + 1 == b.0` serial-id invariant are preserved.
//!
//! Dispatch builders query the sidecar via [`super::SemanticGraphStore::node_scope`]
//! to route per-base-scope lookups through the correct
//! [`SessionSolverHost`](crate::resolver_core::solver_host::SessionSolverHost)
//! without threading scope through every call.
//!
//! **Exempt variants.** `SemanticNodeData::VueMacroElements` nodes store
//! `None` in the sidecar slot — they live on the parser's refcount-only hot
//! path and are never consumed by dispatch builders that walk `node_scope`.
//! The exemption is enforced structurally inside `push_impl` so callers
//! can't accidentally populate a sidecar entry for a vue-macro node, even
//! via [`super::SemanticGraphStore::intern_node_with_scope`]. C17 preserves C7's
//! short-circuit: `VueMacroElements` bypasses both the shard index and the
//! shard Mutex entirely, acquiring only `inner.write()` for the sequential
//! id allocation.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::instant::Instant;
use crate::semantic_query::{NodeScopeId, SemanticNodeData, SemanticNodeId};

pub(super) const NUM_SHARDS: usize = 16;
pub(super) const SHARD_MASK: u64 = (NUM_SHARDS as u64) - 1;

/// Per-shard dedup index. Routes keyed by
/// `hash(payload, scope) & SHARD_MASK`; the same payload + scope
/// always lands on the same shard, so intern-hits never race across
/// shards.
#[derive(Default)]
pub(super) struct ShardIndex {
    index: FxHashMap<(SemanticNodeData, NodeScopeId), SemanticNodeId>,
}

/// Interior state of [`NodeArena`]. Held behind an `RwLock` so reads of
/// `(nodes, scopes)` (non-hot-path) are concurrent while the allocating
/// intern-miss path serializes on the writer.
#[derive(Default)]
pub(super) struct ArenaInner {
    nodes: Vec<Arc<SemanticNodeData>>,
    /// Origin-scope sidecar. Index-aligned with `nodes`.
    /// `None` marks an exempt slot (`VueMacroElements`); `Some(scope)`
    /// records the scope the node was first interned in (`Global` for
    /// scope-less structural nodes, `File { .. }` for declaration-origin
    /// nodes).
    scopes: Vec<Option<NodeScopeId>>,
}

/// Deterministic shard routing — `hash((data, scope)) & mask`. The
/// same `(data, scope)` pair always routes to the same shard,
/// regardless of the calling thread. FxHash picked for speed; the
/// dedup key's own `Eq` implementation disambiguates collisions
/// within a shard.
pub(super) fn shard_index_for(data: &SemanticNodeData, scope: &NodeScopeId) -> usize {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    data.hash(&mut hasher);
    scope.hash(&mut hasher);
    (hasher.finish() & SHARD_MASK) as usize
}

pub(super) struct NodeArena {
    /// Global dense storage for node data + sidecar. `RwLock` so readers
    /// (`get`, `scope`) are concurrent and writers (intern-miss) briefly
    /// serialize to push a fresh slot.
    inner: parking_lot::RwLock<ArenaInner>,
    /// Sharded dedup indexes. Each shard owns the key-range whose
    /// `hash(payload, scope) & mask` lands on it.
    shards: [parking_lot::Mutex<ShardIndex>; NUM_SHARDS],
    /// Optional contention instrumentation. When present,
    /// `push_impl` records per-call counters and `inner.write()`
    /// wait time so downstream passes have evidence-grade contention
    /// data. `None` for test-default arenas constructed via
    /// `Default::default()`.
    pub(super) provenance: Option<Arc<crate::types::MetaProvenance>>,
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
    pub(super) fn push(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.push_impl(data, NodeScopeId::Global)
    }

    /// Intern `data` and record `scope` in the origin sidecar. Called by
    /// builders that know the declaration origin — `build_resolve_decl`,
    /// `build_typeof`, C1's forthcoming `build_instantiate`, etc.
    pub(super) fn push_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.push_impl(data, scope)
    }

    fn push_impl(&self, data: SemanticNodeData, scope: NodeScopeId) -> SemanticNodeId {
        // `VueMacroElements` is exempt — record `None` so
        // `node_scope` returns `None` rather than `Some(Global)` for
        // those nodes. The exemption is structural so even
        // `intern_node_with_scope(VueMacroElements, Some(scope))`
        // yields `None` in the sidecar slot. The sharded dedup is
        // also short-circuited — identity-carriers must allocate a
        // fresh slot on every insert so the `NamedTypeCache`
        // latest-insert-wins contract stays observable.
        let is_vue_macro = matches!(data, SemanticNodeData::VueMacroElements(_));

        // Capture the discriminant before moving `data` so the
        // contention instrumentation can bucket per-variant pushes.
        let discriminant = data.discriminant_index();

        // Sharded dedup hot path. `VueMacroElements` bypasses the
        // shard index entirely (fresh slot every call). Other
        // variants route to their shard and check for an existing
        // id; the miss path acquires `inner.write()` briefly to push
        // the new slot.
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
                let timing_on = verter_scheduler::request_context::current_timing_enabled();
                let lock_start = if timing_on {
                    Some(Instant::now())
                } else {
                    None
                };
                let shard = self.shards[shard_idx].lock();
                let lock_wait = lock_start
                    .map(|t| t.elapsed())
                    .unwrap_or(std::time::Duration::ZERO);
                crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
                if let Some(&existing) = shard.index.get(&key) {
                    (existing, false, 0u64)
                } else {
                    drop(shard);
                    // Miss: re-acquire the shard (to serialize concurrent
                    // misses for the same key on this shard) and then
                    // briefly acquire inner.write() to allocate.
                    let lock_start = if timing_on {
                        Some(Instant::now())
                    } else {
                        None
                    };
                    let mut shard = self.shards[shard_idx].lock();
                    let lock_wait = lock_start
                        .map(|t| t.elapsed())
                        .unwrap_or(std::time::Duration::ZERO);
                    crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
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

    pub(super) fn get(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        let inner = self.inner.read();
        inner.nodes.get(id.0 as usize).cloned()
    }

    /// Return the recorded origin scope for `id` — `None` for exempt nodes
    /// (or invalid ids), `Some(scope)` for everything else. Exempt slots
    /// are `VueMacroElements` nodes.
    pub(super) fn scope(&self, id: SemanticNodeId) -> Option<NodeScopeId> {
        let inner = self.inner.read();
        inner.scopes.get(id.0 as usize).cloned().flatten()
    }

    pub(super) fn len(&self) -> usize {
        self.inner.read().nodes.len()
    }

    /// Drop shard-dedup entries for the given canonical id.
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
    /// generation. The arena's dense node / scope storage is never
    /// shrunk: `SemanticNodeId` is a raw `u64` index with no generation
    /// tag, so reclaiming the id space would require a generational
    /// `SemanticNodeId` redesign.
    ///
    /// Touches every shard mutex once. Each shard's retain walk is
    /// O(shard size). When `node_arena_lock_acquisitions` is wired
    /// into the audit context, each shard lock acquisition is recorded.
    pub(super) fn invalidate_for_canonical(&self, canonical_id: &str) {
        let timing_on = verter_scheduler::request_context::current_timing_enabled();
        for shard in self.shards.iter() {
            let lock_start = if timing_on {
                Some(Instant::now())
            } else {
                None
            };
            let mut shard = shard.lock();
            let lock_wait = lock_start
                .map(|t| t.elapsed())
                .unwrap_or(std::time::Duration::ZERO);
            crate::host_manage::record_node_arena_lock_acquisition(lock_wait);
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
