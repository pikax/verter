//! Derivation / origin layer.
//!
//! Sibling edge store keyed by `(result_node, kind)` that holds the
//! source-set + per-edge metadata + a snapshot of the publishing builder's
//! active fence. Edge dep-signatures are interned in `signature_pool` so
//! builders that emit dozens of edges with identical fences share one
//! `Arc` allocation.
//!
//! ## Bounded retention
//!
//! Both maps are keyed on content-derived state — `edges` on
//! [`SemanticNodeId`]s (a fresh content version produces fresh ids and
//! thus fresh `(result, kind)` keys), `signature_pool` on the distinct
//! [`DepSignature`] fences builders emit. Without a routine reclamation
//! path each map would grow append-only with the content-edit count in a
//! long-lived session — the same unbounded-growth class the bounded
//! query-identity retention substrate exists to cap.
//!
//! Each map is bounded by a [`GlobalRetentionBudget`]: a newly-keyed
//! `(result, kind)` bucket / a newly-interned signature records an
//! admission, and the oldest keys past the cap are FIFO-evicted
//! write-side (from [`DerivationStore::record`] /
//! [`DerivationStore::intern_signature`]). Evicting an `edges` bucket
//! only drops cached derivation provenance — a future origin walk simply
//! finds no edge for that node, never a wrong edge. Evicting a
//! `signature_pool` entry only forgoes a dedup opportunity: any edge that
//! already holds the `Arc<DepSignature>` keeps it alive, and a later
//! identical fence re-allocates instead of dedup-hitting. Both are
//! correctness-neutral.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::bounded_query_retention::{next_retention_seq, GlobalRetentionBudget};
use crate::semantic_query::{DepSignature, OriginEdge, OriginEdgeKind, SemanticNodeId};

/// Total cap on the number of distinct `(result, kind)` edge buckets the
/// derivation store retains. A long-lived session that edits many owners
/// caps here before FIFO eviction reclaims the oldest buckets.
pub(super) const DERIVATION_EDGE_BUCKET_CAP: usize = 4096;

/// Total cap on the number of distinct interned [`DepSignature`] fences
/// the derivation store's interning pool retains.
pub(super) const DERIVATION_SIGNATURE_POOL_CAP: usize = 4096;

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
/// derivations carry different dep-sigs).
///
/// Both maps are bounded — see the module docs. The two budgets are
/// reclaimed wholesale by [`DerivationStore::clear`] on a
/// project-generation reset.
pub(super) struct DerivationStore {
    edges: FxHashMap<(SemanticNodeId, OriginEdgeKind), Vec<OriginEdge>>,
    pub(super) signature_pool: FxHashMap<DepSignature, Arc<DepSignature>>,
    /// FIFO total-size budget bounding the `edges` bucket count.
    edge_budget: GlobalRetentionBudget<(SemanticNodeId, OriginEdgeKind)>,
    /// FIFO total-size budget bounding the `signature_pool` entry count.
    signature_budget: GlobalRetentionBudget<DepSignature>,
}

impl Default for DerivationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DerivationStore {
    /// Construct a derivation store with both retention budgets set to
    /// their default caps.
    pub(super) fn new() -> Self {
        Self {
            edges: FxHashMap::default(),
            signature_pool: FxHashMap::default(),
            edge_budget: GlobalRetentionBudget::new(DERIVATION_EDGE_BUCKET_CAP),
            signature_budget: GlobalRetentionBudget::new(DERIVATION_SIGNATURE_POOL_CAP),
        }
    }

    pub(super) fn intern_signature(&mut self, sig: DepSignature) -> Arc<DepSignature> {
        // Diagnosis: record one signature-intern call per
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
        self.signature_pool.insert(sig.clone(), Arc::clone(&arc));
        crate::capture_token::with_active_capture(|t| t.record_signature_intern(false));
        // Bounded retention: a newly-interned fence records an
        // admission; the oldest pooled signatures past the cap are
        // FIFO-evicted. Eviction only forgoes future dedup — any edge
        // already holding the evicted `Arc` keeps the data alive.
        let seq = next_retention_seq();
        for victim in self.signature_budget.record_admission(seq, sig) {
            self.signature_pool.remove(&victim);
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
        self.edges.entry(bucket_key).or_default().push(edge);
        // Bounded retention: a newly-keyed `(result, kind)` bucket
        // records an admission; the oldest buckets past the cap are
        // FIFO-evicted. Fresh content versions intern fresh
        // `SemanticNodeId`s, so each content version contributes new
        // bucket keys — this budget is what stops the `edges` map
        // growing append-only with the edit count.
        if is_new_bucket {
            let seq = next_retention_seq();
            for victim in self.edge_budget.record_admission(seq, bucket_key) {
                self.edges.remove(&victim);
            }
        }
    }

    /// Drop every edge bucket and every pooled signature, and clear both
    /// retention ledgers. Called from
    /// [`super::SemanticGraphStore::invalidate_all`] on a
    /// project-generation reset — every stored [`SemanticNodeId`] becomes
    /// invalid at that boundary, so this id-keyed store MUST be cleared
    /// before the node arena reuses its id space.
    pub(super) fn clear(&mut self) {
        self.edges.clear();
        self.signature_pool.clear();
        self.edge_budget.clear();
        self.signature_budget.clear();
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

    pub(super) fn edge_count(&self) -> usize {
        self.edges.values().map(Vec::len).sum()
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
