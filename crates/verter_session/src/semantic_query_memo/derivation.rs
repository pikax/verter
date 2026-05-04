//! Derivation / origin layer.
//!
//! Sibling edge store keyed by `(result_node, kind)` that holds the
//! source-set + per-edge metadata + a snapshot of the publishing builder's
//! active fence. Edge dep-signatures are interned in `signature_pool` so
//! builders that emit dozens of edges with identical fences share one
//! `Arc` allocation.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::semantic_query::{DepSignature, OriginEdge, OriginEdgeKind, SemanticNodeId};

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
#[derive(Default)]
pub(super) struct DerivationStore {
    edges: FxHashMap<(SemanticNodeId, OriginEdgeKind), Vec<OriginEdge>>,
    pub(super) signature_pool: FxHashMap<DepSignature, Arc<DepSignature>>,
}

impl DerivationStore {
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
        self.signature_pool.insert(sig, Arc::clone(&arc));
        crate::capture_token::with_active_capture(|t| t.record_signature_intern(false));
        arc
    }

    pub(super) fn record(
        &mut self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        edge: OriginEdge,
    ) {
        self.edges.entry((result, kind)).or_default().push(edge);
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
