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
//! `fact_dep_signature` / `ReadSetSignature` carrier, revalidated by
//! `HostFenceValidator`.
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
use crate::semantic_query::{DepSignature, OriginEdge, OriginEdgeKind, SemanticNodeId};

/// Total cap on the number of distinct `(result, kind)` edge buckets the
/// derivation store retains. A long-lived session that edits many owners
/// caps here before FIFO eviction reclaims the oldest buckets.
pub(super) const DERIVATION_EDGE_BUCKET_CAP: usize = 4096;

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
pub(super) const DERIVATION_EDGES_PER_BUCKET_CAP: usize = 64;

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
pub(super) struct DerivationStore {
    edges: FxHashMap<(SemanticNodeId, OriginEdgeKind), Vec<OriginEdge>>,
    /// Dep-signature interner. Maps a signature value to a [`Weak`]
    /// reference to the canonical `Arc<DepSignature>` handed to edges.
    /// `intern_signature` upgrades the `Weak`: a live upgrade reuses the
    /// `Arc` (an edge still holds it); a dead upgrade means every edge
    /// referencing it was evicted, so a fresh `Arc` is allocated. This
    /// ties pool reclamation to edge lifetime — an entry can never hand
    /// back an `Arc` no live edge holds, so `record_origin_edge`'s
    /// `Arc::ptr_eq` dedup probe stays sound (see module docs).
    pub(super) signature_pool: FxHashMap<DepSignature, Weak<DepSignature>>,
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
}

impl Default for DerivationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DerivationStore {
    /// Construct a derivation store with the edge-bucket retention budget
    /// set to its default cap.
    pub(super) fn new() -> Self {
        Self {
            edges: FxHashMap::default(),
            signature_pool: FxHashMap::default(),
            edge_budget: GlobalRetentionBudget::new(DERIVATION_EDGE_BUCKET_CAP),
            total_edges: 0,
        }
    }

    pub(super) fn intern_signature(&mut self, sig: DepSignature) -> Arc<DepSignature> {
        // Diagnosis: record one signature-intern call per
        // invocation, classified into `returned_existing` vs.
        // `allocated`. The capture-token hook is a no-op when no
        // token is bound (zero-overhead production path). The
        // `with_active_capture` body never panics so it is safe to
        // run inside the `&mut self` borrow.
        //
        // The pool stores `Weak`s: an upgrade succeeds only while a
        // live edge still holds the canonical `Arc`. A successful
        // upgrade reuses that `Arc` so identical signatures keep
        // sharing one allocation — which is what makes the
        // `Arc::ptr_eq` dedup probe in `record_origin_edge` sound. A
        // failed upgrade means every edge that referenced this
        // signature has been evicted from `edges`, so there is no
        // live `Arc` to dedup against and a fresh one is allocated.
        if let Some(existing) = self.signature_pool.get(&sig) {
            if let Some(arc) = existing.upgrade() {
                crate::capture_token::with_active_capture(|t| t.record_signature_intern(true));
                return arc;
            }
        }
        let arc = Arc::new(sig.clone());
        self.signature_pool.insert(sig, Arc::downgrade(&arc));
        crate::capture_token::with_active_capture(|t| t.record_signature_intern(false));
        // Compact dead `Weak`s write-side. The pool's reclamation is
        // tied to edge lifetime, so an evicted edge — whether a whole
        // bucket FIFO-evicted past `edge_budget` or the oldest edge of a
        // bucket evicted past the per-bucket cap — leaves a dead `Weak`
        // behind. Each LIVE edge holds exactly one dep-signature `Arc`,
        // so the count of distinct live-edge signatures is bounded above
        // by `total_edges` (distinct fences may be shared across edges,
        // so the true live-signature count is `<= total_edges`).
        // `total_edges` is itself bounded — bucket count by `edge_budget`
        // and each bucket by `DERIVATION_EDGES_PER_BUCKET_CAP` — so
        // compacting once the pool grows past that live bound keeps the
        // pool's map bounded too, without an independent FIFO cap. The
        // threshold keys on `total_edges` (an O(1) running counter), not
        // the bucket count: the bucket count would under-estimate the
        // live-signature bound by the per-bucket fan-out factor and make
        // the pool compact far more eagerly than its "grown past the
        // live count" rationale intends.
        if self.signature_pool.len() > 2 * self.total_edges + DERIVATION_EDGE_BUCKET_CAP {
            self.signature_pool
                .retain(|_, weak| weak.strong_count() > 0);
        }
        arc
    }

    pub(super) fn record(
        &mut self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        edge: OriginEdge,
    ) {
        let bucket_key = (result, kind);
        let is_new_bucket = !self.edges.contains_key(&bucket_key);
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
        // `edge_dep_signature: Arc<DepSignature>`. When the last live
        // edge holding a pooled signature is evicted, that fence's
        // `signature_pool` `Weak` becomes non-upgradeable and is reaped
        // by the compaction in `intern_signature`. Per-bucket FIFO
        // eviction loses only best-effort provenance — origin edges are
        // not an invalidation source (see the module docs), so dropping
        // the oldest edge degrades only the audit trace, never
        // correctness.
        let bucket = self.edges.entry(bucket_key).or_default();
        bucket.push(edge);
        self.total_edges += 1;
        while bucket.len() > DERIVATION_EDGES_PER_BUCKET_CAP {
            // Oldest = front (insertion order). Dropping it releases its
            // dep-signature `Arc`.
            bucket.remove(0);
            self.total_edges -= 1;
        }
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
                }
            }
        }
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
        self.edge_budget.clear();
        self.total_edges = 0;
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
    ) -> Option<&Vec<OriginEdge>> {
        self.edges.get(&(result, kind))
    }

    /// Test-only filtered origin walk — yields only `kind` edges for
    /// `result`. The sole caller is the test-only
    /// `SemanticGraphStore::origins_of_kind` enumeration accessor;
    /// production origin consumption walks every kind via
    /// [`Self::origins`] (driven by `walk_origin_chain`).
    #[cfg(test)]
    pub(super) fn origins_of_kind(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
    ) -> impl Iterator<Item = &OriginEdge> {
        self.edges
            .get(&(result, kind))
            .into_iter()
            .flat_map(|v| v.iter())
    }

    pub(super) fn origins(
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

    /// Total [`OriginEdge`] count across every bucket. Backed by the
    /// O(1) `total_edges` running counter — maintained by `record`
    /// (append + per-bucket / bucket-level eviction) and reset by
    /// `clear` — so callers do not pay an O(buckets) walk.
    pub(super) fn edge_count(&self) -> usize {
        debug_assert_eq!(
            self.total_edges,
            self.edges.values().map(Vec::len).sum::<usize>(),
            "total_edges counter desynced from the actual bucket edge sum",
        );
        self.total_edges
    }

    /// Iterate over all `(node, kind, edges)` entries in the store. Used
    /// by `stats_snapshot` to compute the origin-edges-per-node
    /// percentiles without exposing the underlying map shape.
    pub(super) fn iter_edges(
        &self,
    ) -> impl Iterator<Item = (&SemanticNodeId, &OriginEdgeKind, &Vec<OriginEdge>)> {
        self.edges
            .iter()
            .map(|((node, kind), edges)| (node, kind, edges))
    }

    pub(super) fn all_edges(&self) -> Vec<(SemanticNodeId, OriginEdgeKind, OriginEdge)> {
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
pub(super) fn sorted_percentile(sorted_ascending: &[u32], p: f64) -> u32 {
    if sorted_ascending.is_empty() {
        return 0;
    }
    let last = sorted_ascending.len() - 1;
    let idx = ((last as f64) * p).round() as usize;
    sorted_ascending[idx.min(last)]
}
