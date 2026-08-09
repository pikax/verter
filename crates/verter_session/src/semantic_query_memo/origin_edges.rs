//! Derivation / origin-edge layer of the semantic graph store.
//!
//! The store-facing half of the `derivation` module: recording a
//! provenance edge for a produced result, and the read-only walks the
//! audit origin-graph builder and the derivation tests use. Origin edges
//! are bounded best-effort provenance, never an invalidation source.

use super::*;

impl SemanticGraphStore {
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
    /// this; the walker walks all edges. The per-`(result, kind)` edge
    /// list is FIFO-capped (`DERIVATION_EDGES_PER_BUCKET_CAP`): a result
    /// re-derived more times than the cap retains only its most recent
    /// edges, so one bucket cannot grow without bound in a long-lived
    /// session. An evicted edge loses only best-effort provenance —
    /// origin edges are not an invalidation source.
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
        // bound (test/debug instrumentation only); the diagnosis
        // benchmark is the only consumer; production-path behaviour is
        // unchanged when no token is bound. The timestamp read and the
        // recording site below both gate on the instrumentation module so
        // release does not pay for them.
        #[cfg(any(test, feature = "test-support"))]
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
        #[cfg(any(test, feature = "test-support"))]
        let elapsed_ns = start.elapsed().as_nanos();
        #[cfg(any(test, feature = "test-support"))]
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
                // `record_origin_edge_total_ns` reflects only the
                // cold-path wall-clock.
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
}
