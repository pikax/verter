#![deny(missing_docs)]
//! Request-scoped footprint accumulator.
//!
//! Plan §2.5. One `RequestFootprintAccumulator` is built per audited
//! `get_component_meta_with_resolution` call, attached to the
//! `RequestContext`, and installed into TLS via
//! `CURRENT_ACCUMULATOR`. The `record_origin_edge` hook (Commit 4) and
//! the structured-event macro (Commit 5) both push into this
//! accumulator under the per-request lock.
//!
//! The accumulator is a `parking_lot::Mutex<AccumulatorState>`. Every
//! push is an append — short critical sections. Draining into the
//! footprint miner happens exactly once per request, at the end of
//! `get_component_meta_with_resolution`.

use std::sync::Arc;

use parking_lot::Mutex;

use super::{
    AliasResolveRecord, ConditionalRecord, IndexedReadyBuildRecord, InstantiationRecord,
    MaterializationRecord, ProjectionRecord, SharedLoadReuseRecord, StructuredComponentMetaEvent,
    SubstitutionRecord, VfsReadRecord,
};
use crate::semantic_query::{OriginEdge, OriginEdgeKind, SemanticNodeId};

/// Raw derivation-edge entry captured during a request. The miner
/// (Commit 4) canonicalises these into the final
/// `DerivationSubgraph.edges` with deterministic `NodeId` / `EdgeId`
/// assignment.
#[derive(Debug, Clone)]
pub struct DerivationEdgeRaw {
    /// Live semantic-node id of the node the edge produces.
    pub result: SemanticNodeId,
    /// Semantic-graph edge kind (union, alias-resolve, etc.).
    pub kind: OriginEdgeKind,
    /// The live `OriginEdge` — sources + meta preserved verbatim for
    /// the miner's canonicalisation pass.
    pub edge: OriginEdge,
}

/// Locked accumulator state. One mutex covers the full push surface
/// so the footprint-miner sees a consistent snapshot when it drains.
#[derive(Debug, Default)]
pub struct AccumulatorState {
    /// Fresh `IndexedReady` build events observed during the request.
    pub indexed_ready_builds: Vec<IndexedReadyBuildRecord>,
    /// VFS reads pushed by the session-side audit sink (fan-out from
    /// the workspace's `VfsAuditSink`).
    pub vfs_reads: Vec<VfsReadRecord>,
    /// Records where this request joined a winner's in-flight slot
    /// instead of starting cold (plan §2.7).
    pub shared_load_reuses: Vec<SharedLoadReuseRecord>,
    /// Type-instantiation steps observed during solver / materialization.
    pub instantiations: Vec<InstantiationRecord>,
    /// Projection steps observed.
    pub projections: Vec<ProjectionRecord>,
    /// Conditional-type branch decisions.
    pub conditional_decisions: Vec<ConditionalRecord>,
    /// Type-parameter substitutions.
    pub substitutions: Vec<SubstitutionRecord>,
    /// Alias-resolve hops.
    pub alias_resolutions: Vec<AliasResolveRecord>,
    /// Materialization envelopes with captured durations.
    pub materializations: Vec<MaterializationRecord>,
    /// Structured events emitted via `component_meta_trace_structured!`
    /// — retained verbatim for snapshot-exact assertions.
    pub structured_events: Vec<StructuredComponentMetaEvent>,
    /// Raw derivation edges captured by the `record_origin_edge` hook
    /// before the miner canonicalises them.
    pub derivation_edges_raw: Vec<DerivationEdgeRaw>,
}

/// Request-scoped accumulator. Thin wrapper over the locked state so
/// the push API is easy to call from any context that holds an `Arc`.
#[derive(Debug, Default)]
pub struct RequestFootprintAccumulator {
    state: Mutex<AccumulatorState>,
}

impl RequestFootprintAccumulator {
    /// Construct an empty accumulator. One per audited request.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain the accumulator into a cloned state snapshot. Used by the
    /// footprint miner after the request completes.
    pub fn drain(&self) -> AccumulatorState {
        let mut st = self.state.lock();
        std::mem::take(&mut *st)
    }

    /// Append a fresh-`IndexedReady` build record.
    pub fn push_indexed_ready_build(&self, record: IndexedReadyBuildRecord) {
        self.state.lock().indexed_ready_builds.push(record);
    }

    /// Append a VFS-read event (usually fanned out from the
    /// workspace's `VfsAuditSink`).
    pub fn push_vfs_read(&self, record: VfsReadRecord) {
        self.state.lock().vfs_reads.push(record);
    }

    /// Append a shared-load reuse record from the scheduler's
    /// dedup hook (`on_dedup_joiner`).
    pub fn push_shared_load_reuse(
        &self,
        canonical_id: Arc<str>,
        winner_request_id: u64,
        winner_audited: bool,
    ) {
        self.state
            .lock()
            .shared_load_reuses
            .push(SharedLoadReuseRecord {
                canonical_id,
                winner_request_id,
                winner_audited,
            });
    }

    /// Append an instantiation step.
    pub fn push_instantiation(&self, record: InstantiationRecord) {
        self.state.lock().instantiations.push(record);
    }

    /// Append a projection step.
    pub fn push_projection(&self, record: ProjectionRecord) {
        self.state.lock().projections.push(record);
    }

    /// Append a conditional-branch decision.
    pub fn push_conditional(&self, record: ConditionalRecord) {
        self.state.lock().conditional_decisions.push(record);
    }

    /// Append a type-parameter substitution step.
    pub fn push_substitution(&self, record: SubstitutionRecord) {
        self.state.lock().substitutions.push(record);
    }

    /// Append an alias-resolve step.
    pub fn push_alias_resolution(&self, record: AliasResolveRecord) {
        self.state.lock().alias_resolutions.push(record);
    }

    /// Append a materialization envelope.
    pub fn push_materialization(&self, record: MaterializationRecord) {
        self.state.lock().materializations.push(record);
    }

    /// Append a structured event emitted by
    /// `component_meta_trace_structured!`.
    pub fn push_structured_event(&self, event: StructuredComponentMetaEvent) {
        self.state.lock().structured_events.push(event);
    }

    /// Append a raw derivation edge captured by the
    /// `record_origin_edge` hook (plan §3 Commit 4).
    pub fn push_derivation_edge(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        edge: OriginEdge,
    ) {
        self.state
            .lock()
            .derivation_edges_raw
            .push(DerivationEdgeRaw { result, kind, edge });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_meta_audit::{VfsLayer, VfsReadRecord};

    #[test]
    fn accumulator_push_indexed_ready_build_appends() {
        let acc = RequestFootprintAccumulator::new();
        acc.push_indexed_ready_build(IndexedReadyBuildRecord {
            canonical_id: Arc::from("/a.ts"),
            whole_hash: [1u8; 16],
        });
        let st = acc.drain();
        assert_eq!(st.indexed_ready_builds.len(), 1);
        assert_eq!(st.indexed_ready_builds[0].canonical_id.as_ref(), "/a.ts");
    }

    #[test]
    fn accumulator_push_vfs_read_appends() {
        let acc = RequestFootprintAccumulator::new();
        acc.push_vfs_read(VfsReadRecord {
            canonical_id: Arc::from("/b.ts"),
            layer: VfsLayer::Overlay,
            cache_hit: true,
            bytes_read: 42,
            request_id: 7,
        });
        let st = acc.drain();
        assert_eq!(st.vfs_reads.len(), 1);
        assert_eq!(st.vfs_reads[0].bytes_read, 42);
        assert_eq!(st.vfs_reads[0].request_id, 7);
    }

    #[test]
    fn accumulator_push_shared_load_reuse_appends() {
        let acc = RequestFootprintAccumulator::new();
        acc.push_shared_load_reuse(Arc::from("/c.ts"), 123, true);
        let st = acc.drain();
        assert_eq!(st.shared_load_reuses.len(), 1);
        assert_eq!(st.shared_load_reuses[0].winner_request_id, 123);
    }

    #[test]
    fn accumulator_push_structured_event_appends() {
        let acc = RequestFootprintAccumulator::new();
        acc.push_structured_event(StructuredComponentMetaEvent::RequestStart {
            canonical_id: Arc::from("/d.vue"),
            request_id: 7,
        });
        let st = acc.drain();
        assert_eq!(st.structured_events.len(), 1);
    }

    #[test]
    fn accumulator_drain_yields_empty_state_for_subsequent_calls() {
        let acc = RequestFootprintAccumulator::new();
        acc.push_shared_load_reuse(Arc::from("/e.ts"), 1, false);
        let first = acc.drain();
        assert_eq!(first.shared_load_reuses.len(), 1);
        let second = acc.drain();
        assert_eq!(second.shared_load_reuses.len(), 0);
    }

    #[test]
    fn accumulator_concurrent_pushes_from_16_threads_consistent() {
        use std::thread;
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let handles: Vec<_> = (0..16)
            .map(|tid| {
                let acc = Arc::clone(&acc);
                thread::spawn(move || {
                    for i in 0..32 {
                        acc.push_shared_load_reuse(
                            Arc::from(format!("/t{tid}_{i}.vue").as_str()),
                            (tid * 100 + i) as u64,
                            i % 2 == 0,
                        );
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let st = acc.drain();
        assert_eq!(st.shared_load_reuses.len(), 16 * 32);
    }
}
