//! Derivation / origin layer.
//!
//! Sibling edge store keyed by `(result_node, kind)` that holds the
//! source-set + per-edge metadata + a snapshot of the publishing builder's
//! active fence. Edge dep-signatures are interned in `signature_pool` so
//! builders that emit dozens of edges with identical fences share one
//! `Arc` allocation.
//!
//! ## Bounded retention — origin edges are best-effort provenance
//!
//! `edges` is keyed on content-derived state — `(result, kind)` pairs of
//! [`SemanticNodeId`]s, where a fresh content version produces fresh ids
//! and thus fresh keys. Without a routine reclamation path it would grow
//! append-only with the content-edit count in a long-lived session — the
//! same unbounded-growth class the bounded query-identity retention
//! substrate exists to cap. Growth is bounded on TWO axes, both enforced
//! write-side in [`DerivationStore::record`] under the store's exclusive
//! `&mut self` borrow:
//!
//! - **bucket count** — a [`GlobalRetentionBudget`] caps the number of
//!   distinct `(result, kind)` buckets. A newly-keyed bucket records an
//!   admission, and the oldest buckets past `DERIVATION_EDGE_BUCKET_CAP`
//!   are FIFO-evicted.
//! - **per-bucket edge count** — a single `(result, kind)` bucket holds
//!   one [`OriginEdge`] per distinct derivation of that result, and
//!   distinct derivations carry distinct fences so the identity-tuple
//!   dedup at [`SemanticGraphStore::record_origin_edge`] never collapses
//!   them. A per-bucket FIFO cap (`DERIVATION_EDGES_PER_BUCKET_CAP`)
//!   evicts the oldest edge of a bucket on EVERY append past the cap —
//!   including an append to an already-existing bucket — so one bucket
//!   re-derived many times in a long-lived session cannot grow without
//!   bound.
//!
//! **Origin edges are bounded best-effort provenance, NOT an
//! invalidation source.** A FIFO-evicted bucket loses cached *provenance*
//! only — a later origin walk simply finds no edge for that node, never
//! a wrong edge. An origin edge's `edge_dep_signature` is a snapshot of
//! the publishing builder's fence kept purely for the audit origin-graph
//! trace ([`crate::meta_resolve::build_origin_graph`]); it is **not** a
//! dependency-propagation route and nothing reconstructs a completion
//! fence (`CompletionFence`) from it. The load-bearing invalidation
//! record for a cached result is that result's own memo entry — its
//! `ReadSetSignature` carrier, revalidated against the live `StoreView`
//! on every warm read.
//! Because origin edges carry no invalidation weight, FIFO-evicting a
//! bucket is sound: it degrades only the audit trace, never correctness.
//!
//! `signature_pool` is the dep-signature interner. It is NOT bounded by an
//! independent FIFO cap: `record_origin_edge`'s duplicate-edge dedup probe
//! compares the interned `Arc<DepSignature>` by pointer
//! ([`std::sync::Arc::ptr_eq`]), so the interner MUST guarantee that an
//! identical signature value always shares ONE `Arc` for as long as a live
//! edge references it. An independent cap that evicted a pooled signature
//! a live edge still held would break that guarantee: the next intern of
//! the same value would allocate a fresh `Arc`, the pointer probe would
//! miss, and the edge would be recorded a second time. The pool's
//! reclamation is therefore tied to edge lifetime — it holds
//! [`std::sync::Weak`] values. `intern_signature` upgrades the stored
//! `Weak`: a successful upgrade means a live edge still holds the `Arc`,
//! so it is reused; a failed upgrade means every edge that referenced it
//! has been evicted from `edges`, so a fresh `Arc` is allocated. A pooled
//! signature can never hand back a stale `Arc`, and a signature still
//! reachable from a live edge can never be missed. The `edges` map is
//! bounded on both axes — bucket count by `edge_budget` and per-bucket
//! edge count by `DERIVATION_EDGES_PER_BUCKET_CAP` — so the count of
//! distinct live-edge signatures is bounded above by the total live-edge
//! count ([`DerivationStore::edge_count`]); the pool's map of `Weak`s is
//! compacted of dead entries write-side once it grows past `2 ×` that
//! live edge count plus slack, so it does not accumulate the dead `Weak`s
//! that bucket-level or per-bucket FIFO eviction leaves behind.

use std::sync::{Arc, Weak};

use rustc_hash::FxHashMap;

use crate::bounded_query_retention::{next_retention_seq, GlobalRetentionBudget};
use crate::semantic_query::{DepSignature, EdgeFence, OriginEdge, OriginEdgeKind, SemanticNodeId};

/// Total cap on the number of distinct `(result, kind)` edge buckets the
/// derivation store retains. A long-lived session that edits many owners
/// caps here before FIFO eviction reclaims the oldest buckets.
pub const DERIVATION_EDGE_BUCKET_CAP: usize = 4096;

/// Cap on the number of [`OriginEdge`]s a single `(result, kind)` bucket
/// retains. Multiple derivations of the same structural result for the
/// same kind append parallel edges to one bucket — distinct derivations
/// carry distinct fences so the identity-tuple dedup at
/// [`super::SemanticGraphStore::record_origin_edge`] never collapses
/// them. A long-lived session that re-derives one result many times
/// would grow that single bucket without bound; the per-bucket FIFO cap
/// evicts the oldest edge on every append past this cap. Sized for the
/// legitimate `{instantiate, substitute, conditional-select, project,
/// normalize, alias-resolve}` derivation fan-in of one result with
/// generous headroom; an evicted edge loses only best-effort provenance
/// (origin edges are not an invalidation source — see the module docs).
pub const DERIVATION_EDGES_PER_BUCKET_CAP: usize = 64;

/// Granule the edge store's own storage is charged in.
const STORAGE_GRANULE: usize = 4 * 1024;

/// Bytes one bucket of the edge table holds: its key, its edge list's
/// header and its control byte.
const EDGE_TABLE_BUCKET_BYTES: usize =
    std::mem::size_of::<((SemanticNodeId, OriginEdgeKind), EdgeBucket)>() + 1;

/// Bytes one bucket of the fence pool's table holds.
const POOL_TABLE_BUCKET_BYTES: usize = std::mem::size_of::<(u64, Vec<Weak<EdgeFence>>)>() + 1;

/// The block one pooled fence keeps allocated: its pool `Weak` keeps the
/// block, whose value the last edge drops, until the pool compacts it.
fn fence_block_bytes() -> usize {
    verter_session_query::retention::allocation::arc_block_bytes(std::mem::size_of::<EdgeFence>())
}

/// Bytes one edge bucket keeps: its edge and identity lists' backing and
/// every edge's source list.
fn bucket_bytes(bucket: &EdgeBucket) -> usize {
    use verter_session_query::retention::allocation::{arc_block_bytes, vec_bytes};
    vec_bytes(&bucket.edges)
        + vec_bytes(&bucket.identities)
        + bucket
            .edges
            .iter()
            .map(|edge| arc_block_bytes(std::mem::size_of_val::<[SemanticNodeId]>(&edge.sources)))
            .sum::<usize>()
}

/// Bytes one pool bucket keeps: its `Weak` list's backing and every pooled
/// fence's block.
fn pool_bucket_bytes(bucket: &Vec<Weak<EdgeFence>>) -> usize {
    verter_session_query::retention::allocation::vec_bytes(bucket)
        + bucket.len() * fence_block_bytes()
}

/// Sibling edge store for the derivation/origin layer. Co-owned by
/// [`super::SemanticGraphStore`] but conceptually a separate graph: edges
/// are keyed by `(result_node, kind)` and hold the source-set + per-edge
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
/// derivations carry different dep-sigs). That per-key `Vec` is itself
/// FIFO-capped at `DERIVATION_EDGES_PER_BUCKET_CAP` so one repeatedly
/// re-derived result cannot grow a single bucket without bound.
///
/// `edges` is bounded on both axes — the bucket count by `edge_budget`
/// and each bucket's edge `Vec` by `DERIVATION_EDGES_PER_BUCKET_CAP`;
/// `signature_pool` holds [`Weak`] values so its reclamation is tied to
/// edge lifetime — see the module docs. Both maps are dropped wholesale
/// by [`DerivationStore::clear`] on a project-generation reset.
pub(crate) struct DerivationStore {
    edges: FxHashMap<(SemanticNodeId, OriginEdgeKind), EdgeBucket>,
    /// Dep-signature interner. Maps a signature's content hash to
    /// [`Weak`] references to the canonical `Arc<EdgeFence>`s handed to
    /// edges. `intern_signature` upgrades the `Weak`s: a live upgrade of an
    /// equal fence reuses that `Arc` (an edge still holds it); a dead
    /// upgrade means every edge referencing it was evicted, so a fresh
    /// `Arc` is allocated. This ties pool reclamation to edge lifetime — an
    /// entry can never hand back an `Arc` no live edge holds, so
    /// `record_origin_edge`'s `Arc::ptr_eq` dedup probe stays sound (see
    /// module docs) — and the pool itself keeps no rail alive: only edges
    /// do, through the fence's hold on the rail's own charge.
    pub(super) signature_pool: FxHashMap<u64, Vec<Weak<EdgeFence>>>,
    /// `Weak`s across every pool bucket.
    pool_entries: usize,
    /// FIFO total-size budget bounding the `edges` bucket count.
    edge_budget: GlobalRetentionBudget<(SemanticNodeId, OriginEdgeKind)>,
    /// Running count of [`OriginEdge`]s resident across every bucket —
    /// the sum of all bucket `Vec` lengths. Maintained incrementally so
    /// [`Self::edge_count`] and the `signature_pool` compaction
    /// threshold are O(1) rather than an O(buckets) walk on the
    /// signature-intern hot path. Incremented on every `record` append
    /// and decremented by exactly the number of edges each eviction
    /// (per-bucket FIFO + bucket-level FIFO) drops; reset to 0 by
    /// [`Self::clear`].
    total_edges: usize,
    /// The store's own tables, edge lists, source lists and pooled fence
    /// blocks, pinned and settled on every mutation; the rails the fences
    /// hold are charged by their own cells.
    storage: Storage,
}

/// The edge store's pinned storage charge.
struct Storage {
    account: Arc<verter_session_query::retention::SemanticRetentionAccount>,
    pin: Option<verter_session_query::retention::granular::GranularPin>,
    /// The live total last settled.
    charged_live: usize,
}

impl Storage {
    fn settle(&mut self, added: usize, freed: usize) {
        self.charged_live = self.charged_live + added - freed;
        if added == freed {
            return;
        }
        let account = &self.account;
        let pin = self.pin.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(account, STORAGE_GRANULE)
        });
        if added > freed {
            pin.add(added - freed);
        } else {
            pin.sub(freed - added);
        }
    }

    fn charged(&self) -> usize {
        self.pin.as_ref().map_or(
            0,
            verter_session_query::retention::granular::GranularPin::charged,
        )
    }
}

impl Default for DerivationStore {
    fn default() -> Self {
        Self::charged(&verter_session_query::retention::SemanticRetentionAccount::process_local())
    }
}

impl DerivationStore {
    /// A derivation store whose own storage is charged to `account`, with
    /// the edge-bucket retention budget set to its default cap.
    pub(super) fn charged(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            edges: FxHashMap::default(),
            signature_pool: FxHashMap::default(),
            pool_entries: 0,
            edge_budget: GlobalRetentionBudget::new(DERIVATION_EDGE_BUCKET_CAP),
            total_edges: 0,
            storage: Storage {
                account: Arc::clone(account),
                pin: None,
                charged_live: 0,
            },
        }
    }

    /// The table bytes of both maps: their capacity.
    fn table_bytes(&self) -> usize {
        self.edges.capacity() * EDGE_TABLE_BUCKET_BYTES
            + self.signature_pool.capacity() * POOL_TABLE_BUCKET_BYTES
    }

    /// Everything the store's storage holds, walked in full.
    fn storage_bytes(&self) -> usize {
        self.table_bytes()
            + self.edges.values().map(bucket_bytes).sum::<usize>()
            + self
                .signature_pool
                .values()
                .map(pool_bucket_bytes)
                .sum::<usize>()
    }

    /// Re-measure the whole store and settle its charge to it.
    fn settle_in_full(&mut self) {
        let live = self.storage_bytes();
        let charged = self.storage.charged_live;
        self.storage.settle(live, charged);
    }

    /// Bytes the store's own storage is charged.
    pub(super) fn storage_charge_bytes(&self) -> usize {
        self.storage.charged()
    }

    /// The pooled fence for `sig`, sharing a live edge's when one holds an
    /// equal fence. A fresh fence takes the store's hold on the rail's own
    /// charge; `None` when the account refuses it, and the edge is then not
    /// recorded — origin edges are best-effort provenance.
    pub(super) fn intern_signature(
        &mut self,
        sig: DepSignature,
        ctx: verter_session_query::retention::owned::ChargeCtx<'_>,
    ) -> Option<Arc<EdgeFence>> {
        // Diagnosis: record one signature-intern call per
        // invocation, classified into `returned_existing` vs.
        // `allocated`. The capture-token hook is a no-op when no
        // token is bound (zero-overhead production path).
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = rustc_hash::FxHasher::default();
            sig.hash(&mut hasher);
            hasher.finish()
        };
        if let Some(existing) = self.signature_pool.get(&hash).and_then(|bucket| {
            bucket
                .iter()
                .filter_map(Weak::upgrade)
                .find(|fence| ***fence == sig)
        }) {
            #[cfg(any(test, feature = "test-support"))]
            crate::capture_token::with_active_capture(|t| t.record_signature_intern(true));
            return Some(existing);
        }
        let hold = match sig.owned() {
            Some(rail) => Some(rail.hold(ctx).ok()?),
            None => None,
        };
        let arc = Arc::new(EdgeFence::retained(sig, hold));
        let table_before = self.table_bytes();
        let bucket_before = self.signature_pool.get(&hash).map_or(0, pool_bucket_bytes);
        let bucket = self.signature_pool.entry(hash).or_default();
        bucket.push(Arc::downgrade(&arc));
        let bucket_after = pool_bucket_bytes(bucket);
        self.pool_entries += 1;
        let table_after = self.table_bytes();
        self.storage
            .settle(table_after + bucket_after, table_before + bucket_before);
        #[cfg(any(test, feature = "test-support"))]
        crate::capture_token::with_active_capture(|t| t.record_signature_intern(false));
        // Compact dead `Weak`s write-side. The pool's reclamation is
        // tied to edge lifetime, so an evicted edge leaves a dead `Weak`
        // behind. Each LIVE edge holds exactly one fence, so the count of
        // distinct live-edge fences is bounded above by `total_edges`,
        // itself bounded by `edge_budget` and the per-bucket cap; compacting
        // once the pool grows past that live bound keeps the pool bounded
        // too, without an independent FIFO cap.
        if self.pool_entries > 2 * self.total_edges + DERIVATION_EDGE_BUCKET_CAP {
            self.signature_pool.retain(|_, bucket| {
                bucket.retain(|weak| weak.strong_count() > 0);
                !bucket.is_empty()
            });
            self.pool_entries = self.signature_pool.values().map(Vec::len).sum();
            self.settle_in_full();
        }
        Some(arc)
    }

    pub(super) fn record(
        &mut self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        identity: u64,
        edge: OriginEdge,
    ) {
        let bucket_key = (result, kind);
        let is_new_bucket = !self.edges.contains_key(&bucket_key);
        let table_before = self.table_bytes();
        let bucket_before = self.edges.get(&bucket_key).map_or(0, bucket_bytes);
        // Append the edge, then apply the PER-BUCKET FIFO cap inside the
        // SAME `&mut self` step. The bucket `Vec` is insertion-ordered
        // (push at the back), so the oldest edge is at the front and a
        // `remove(0)` is the FIFO eviction. The cap is applied on EVERY
        // append — including an append to an EXISTING bucket — so a
        // single `(result, kind)` bucket can never grow past the cap.
        // Distinct derivations of the same result carry distinct fences
        // and so are never collapsed by the identity-tuple dedup at
        // `record_origin_edge`; without this per-edge cap one bucket
        // would grow append-only with the re-derivation count.
        //
        // An evicted `OriginEdge` is dropped here, which drops its
        // `edge_dep_signature: Arc<EdgeFence>`. When the last live
        // edge holding a pooled signature is evicted, that fence's
        // `signature_pool` `Weak` becomes non-upgradeable and is reaped
        // by the compaction in `intern_signature`. Per-bucket FIFO
        // eviction loses only best-effort provenance — origin edges are
        // not an invalidation source (see the module docs), so dropping
        // the oldest edge degrades only the audit trace, never
        // correctness.
        let bucket = self.edges.entry(bucket_key).or_default();
        bucket.push(identity, edge);
        self.total_edges += 1;
        while bucket.len() > DERIVATION_EDGES_PER_BUCKET_CAP {
            // Oldest = front (insertion order). Dropping it releases its
            // dep-signature `Arc`.
            bucket.evict_oldest();
            self.total_edges -= 1;
        }
        let bucket_after = bucket_bytes(bucket);
        let mut evicted = 0;
        // Bounded retention: a newly-keyed `(result, kind)` bucket
        // records an admission; the oldest buckets past the cap are
        // FIFO-evicted. Fresh content versions intern fresh
        // `SemanticNodeId`s, so each content version contributes new
        // bucket keys — this budget is what stops the `edges` map
        // growing append-only with the edit count.
        //
        // `record_admission` hands back `(seq, bucket_key)` victims. The
        // removal here is by `bucket_key` alone, NOT by admission seq —
        // sound because `DerivationStore::record` takes `&mut self`:
        // `edges` and `edge_budget` are mutated under that exclusive
        // borrow (the `SemanticGraphStore` holds the store behind a
        // `Mutex<DerivationStore>`), so no concurrent writer can
        // re-admit a FIFO victim's `bucket_key` between `record_admission`
        // and this drain. A key-based removal therefore cannot evict a
        // fresh same-key re-admission. The `GlobalRetentionBudget`
        // victim-identity contract permits a key-based removal for
        // exactly this exclusive-`&mut self`-serialised case.
        if is_new_bucket {
            let seq = next_retention_seq();
            for (_victim_seq, victim) in self.edge_budget.record_admission(seq, bucket_key) {
                if let Some(dropped) = self.edges.remove(&victim) {
                    self.total_edges -= dropped.len();
                    evicted += bucket_bytes(&dropped);
                }
            }
        }
        let table_after = self.table_bytes();
        self.storage.settle(
            table_after + bucket_after,
            table_before + bucket_before + evicted,
        );
    }

    /// Drop every edge bucket and every pooled signature, and clear the
    /// edge-bucket retention ledger. Called from
    /// [`super::SemanticGraphStore::invalidate_all`] on a
    /// project-generation reset: the stale [`SemanticNodeId`]-keyed edge
    /// buckets are dropped so a future origin walk does not return a
    /// judgement carried over from the superseded project generation. The
    /// node arena itself is append-only and is not reset — `clear` only
    /// drops this store's own cached provenance.
    pub(super) fn clear(&mut self) {
        self.edges.clear();
        self.signature_pool.clear();
        self.pool_entries = 0;
        self.edge_budget.clear();
        self.total_edges = 0;
        self.settle_in_full();
    }

    /// Document-close release: drop every bucket keyed by a released
    /// result node and every edge whose sources name a released node
    /// (a bucket emptied that way is dropped too), keeping the retention
    /// ledger and `total_edges` exact. Origin edges are best-effort
    /// provenance, never an invalidation source, so dropping them
    /// degrades only the audit trace of a document that is gone. Returns
    /// the number of buckets dropped.
    pub(super) fn release_nodes(&mut self, is_released: &dyn Fn(SemanticNodeId) -> bool) -> usize {
        let mut victims: Vec<(SemanticNodeId, OriginEdgeKind)> = Vec::new();
        let mut dropped_edges = 0usize;
        for (key, bucket) in self.edges.iter_mut() {
            if is_released(key.0) {
                dropped_edges += bucket.len();
                victims.push(*key);
                continue;
            }
            let before = bucket.len();
            bucket.retain(|edge| !edge.sources.iter().any(|source| is_released(*source)));
            dropped_edges += before - bucket.len();
            if bucket.is_empty() {
                victims.push(*key);
            }
        }
        for key in &victims {
            self.edges.remove(key);
            // Sound under the exclusive `&mut self` borrow — see `record`.
            self.edge_budget.forget_key_under_exclusive_lock(key);
        }
        self.total_edges -= dropped_edges;
        self.settle_in_full();
        victims.len()
    }

    /// Number of distinct `(result, kind)` edge buckets currently
    /// retained. The bounded-retention proof asserts on this.
    pub(super) fn bucket_count(&self) -> usize {
        self.edges.len()
    }

    /// Lookup the bucket for `(result, kind)` for an identity-tuple
    /// dedup probe (`record_origin_edge` callers compare `Arc::ptr_eq`
    /// on `edge_dep_signature` plus content equality on `sources` and
    /// `meta`). Returns `None` when no bucket exists for the key.
    pub(super) fn bucket_for(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
    ) -> Option<&EdgeBucket> {
        self.edges.get(&(result, kind))
    }

    /// Test-only filtered origin walk — yields only `kind` edges for
    /// `result`. The sole caller is the test-only
    /// `SemanticGraphStore::origins_of_kind` enumeration accessor;
    /// production origin consumption walks every kind via
    /// [`Self::origins`] (driven by `walk_origin_chain`).
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn origins_of_kind(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
    ) -> impl Iterator<Item = &OriginEdge> {
        self.edges
            .get(&(result, kind))
            .into_iter()
            .flat_map(|bucket| bucket.edges.iter())
    }

    pub(super) fn origins(
        &self,
        result: SemanticNodeId,
    ) -> impl Iterator<Item = (OriginEdgeKind, &OriginEdge)> {
        self.edges
            .iter()
            .filter_map(move |((r, kind), edges)| {
                if *r == result {
                    Some(edges.edges.iter().map(move |e| (*kind, e)))
                } else {
                    None
                }
            })
            .flatten()
    }

    /// Total [`OriginEdge`] count across every bucket. Backed by the
    /// O(1) `total_edges` running counter — maintained by `record`
    /// (append + per-bucket / bucket-level eviction) and reset by
    /// `clear` — so callers do not pay an O(buckets) walk.
    pub(super) fn edge_count(&self) -> usize {
        verter_debug_assert_eq!(
            self.total_edges,
            self.edges.values().map(EdgeBucket::len).sum::<usize>(),
            "total_edges counter desynced from the actual bucket edge sum",
        );
        self.total_edges
    }

    /// Iterate over all `(node, kind, edges)` entries in the store. Used
    /// by `stats_snapshot` to compute the origin-edges-per-node
    /// percentiles without exposing the underlying map shape.
    pub(super) fn iter_edges(
        &self,
    ) -> impl Iterator<Item = (&SemanticNodeId, &OriginEdgeKind, &[OriginEdge])> {
        self.edges
            .iter()
            .map(|((node, kind), bucket)| (node, kind, bucket.edges.as_slice()))
    }

    pub(super) fn all_edges(&self) -> Vec<(SemanticNodeId, OriginEdgeKind, OriginEdge)> {
        self.edges
            .iter()
            .flat_map(|((node, kind), bucket)| {
                bucket
                    .edges
                    .iter()
                    .map(move |edge| (*node, *kind, edge.clone()))
            })
            .collect()
    }
}

/// One `(result, kind)` bucket: its edges in insertion order, beside each
/// edge's identity key ([`edge_identity_key`]). The duplicate-edge probe
/// compares the keys and confirms only a matching key on the whole edge.
#[derive(Default)]
pub(super) struct EdgeBucket {
    edges: Vec<OriginEdge>,
    identities: Vec<u64>,
}

impl EdgeBucket {
    fn push(&mut self, identity: u64, edge: OriginEdge) {
        self.edges.push(edge);
        self.identities.push(identity);
    }

    fn evict_oldest(&mut self) {
        self.edges.remove(0);
        self.identities.remove(0);
    }

    fn len(&self) -> usize {
        self.edges.len()
    }

    fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    fn retain(&mut self, mut keep: impl FnMut(&OriginEdge) -> bool) {
        let mut index = 0;
        while index < self.edges.len() {
            if keep(&self.edges[index]) {
                index += 1;
            } else {
                self.edges.remove(index);
                self.identities.remove(index);
            }
        }
    }

    /// Whether the bucket already holds `edge`, whose identity key is
    /// `identity`: the same interned signature, sources and metadata.
    pub(super) fn holds(&self, identity: u64, edge: &OriginEdge) -> bool {
        for index in 0..self.identities.len() {
            if self.identities[index] == identity {
                let held = &self.edges[index];
                if Arc::ptr_eq(&held.edge_dep_signature, &edge.edge_dep_signature)
                    && held.sources == edge.sources
                    && held.meta == edge.meta
                {
                    return true;
                }
            }
        }
        false
    }
}

/// The identity key of an edge with interned signature `signature` over
/// `sources`: equal edges have equal keys. It narrows the duplicate-edge
/// probe; a key match is confirmed on the whole edge.
pub(super) fn edge_identity_key(signature: &Arc<EdgeFence>, sources: &[SemanticNodeId]) -> u64 {
    const MIX: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut key = (Arc::as_ptr(signature) as *const u8 as usize as u64).wrapping_mul(MIX);
    key = (key.rotate_left(5) ^ sources.len() as u64).wrapping_mul(MIX);
    for source in sources {
        key = (key.rotate_left(5) ^ source.0).wrapping_mul(MIX);
    }
    key
}

/// Pick the percentile-`p` value out of an already-sorted ascending
/// slice. Returns 0 for an empty slice. Index rounding matches the
/// nearest-rank method (R-3 / SAS / Excel `PERCENTILE.INC`).
pub(super) fn sorted_percentile(sorted_ascending: &[u32], p: f64) -> u32 {
    if sorted_ascending.is_empty() {
        return 0;
    }
    let last = sorted_ascending.len() - 1;
    let idx = ((last as f64) * p).round() as usize;
    sorted_ascending[idx.min(last)]
}
