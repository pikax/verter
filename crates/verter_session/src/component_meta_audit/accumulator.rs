#![deny(missing_docs)]
//! Request-scoped footprint accumulator.
//!
//! One `RequestFootprintAccumulator` is built per audited
//! `get_component_meta_with_resolution` call, attached to the
//! `RequestContext`, and installed into TLS via
//! `CURRENT_ACCUMULATOR`. The `record_origin_edge` hook and the
//! structured-event macro both push into this accumulator under the
//! per-request lock.
//!
//! The accumulator is a `parking_lot::Mutex<AccumulatorState>`. Every
//! push is an append — short critical sections. Draining into the
//! footprint miner happens exactly once per request, at the end of
//! `get_component_meta_with_resolution`.

use std::sync::Arc;

use parking_lot::Mutex;

use super::{
    AliasResolveRecord, ConditionalRecord, IndexedReadyBuildRecord, InstantiationRecord,
    MaterializationRecord, ProjectionRecord, SharedLoadReuseRecord, StructuredAuditEvent,
    SubstitutionRecord, VfsReadRecord,
};
use crate::semantic_query::{OriginEdge, OriginEdgeKind, SemanticNodeId};
use verter_audit::{AuditCaps, TruncationCounters};

/// Raw derivation-edge entry captured during a request. The miner
/// canonicalises these into the final `DerivationSubgraph.edges` with
/// deterministic `NodeId` / `EdgeId` assignment.
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
    /// Per-file timing ledger pushed by the session-side audit sink in
    /// parallel with `vfs_reads`. Carries optional `read_ns` so the
    /// `FileAudit` builder can populate read-once-aware `read_ms`
    /// timings without retaining the substrate's `VfsReadRecord` shape.
    pub file_read_timings: Vec<FileReadTiming>,
    /// Per-file parse/lower timing ledger pushed by the executor's
    /// source stage when timing capture is on. Used by the `FileAudit`
    /// builder to populate `parse_ms` / `lower_ms` for files this
    /// request triggered an `IndexedReady` build for.
    pub file_parse_timings: Vec<FileParseTiming>,
    /// Records where this request joined a winner's in-flight slot
    /// instead of starting cold.
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
    pub structured_events: Vec<StructuredAuditEvent>,
    /// Raw derivation edges captured by the `record_origin_edge` hook
    /// before the miner canonicalises them.
    pub derivation_edges_raw: Vec<DerivationEdgeRaw>,
    /// Per-category truncation counters. Each `push_*` method on
    /// [`RequestFootprintAccumulator`] checks the matching cap on the
    /// owning [`AuditCaps`]; once the cap is reached, the item is
    /// dropped and the matching counter is incremented. The miner
    /// surfaces this struct on
    /// [`verter_audit::RequestFootprintAudit::truncation_counters`].
    pub truncation_counters: TruncationCounters,
}

/// Per-file timing ledger entry — pushed by the session-side VFS sink
/// alongside `VfsReadRecord`. Carries the optional `read_ns` from the
/// workspace's `VfsReadEvent` so `FileAudit::read_ms` can be populated
/// without changing the public `VfsReadRecord` wire shape.
#[derive(Debug, Clone)]
pub struct FileReadTiming {
    /// Canonical id of the file the timing entry attributes.
    pub canonical_id: Arc<str>,
    /// Which VFS layer served the read.
    pub layer: super::VfsLayer,
    /// `true` when the read resolved from an in-memory cache.
    pub cache_hit: bool,
    /// Number of bytes returned (0 for negative / missing reads).
    pub bytes_read: u64,
    /// Wall-clock nanoseconds spent inside the workspace `read_file`
    /// path. `Some(value)` only when timing capture was on at event
    /// time.
    pub read_ns: Option<u64>,
}

/// Per-file parse/lower timing ledger entry — pushed by the executor's
/// source stage when timing capture is on. The `FileAudit` builder
/// uses this to populate `parse_ms` / `lower_ms` for files the request
/// triggered an `IndexedReady` build for. Files served from the
/// existing cache do not get an entry — the read-once invariant.
#[derive(Debug, Clone)]
pub struct FileParseTiming {
    /// Canonical id of the file whose source stage was executed.
    pub canonical_id: Arc<str>,
    /// Wall-clock nanoseconds spent parsing the source.
    pub parse_ns: u64,
    /// Wall-clock nanoseconds spent lowering the parsed AST. May be
    /// `0` when the executor combines parse and lower phases.
    pub lower_ns: u64,
}

/// Request-scoped accumulator. Thin wrapper over the locked state so
/// the push API is easy to call from any context that holds an `Arc`.
///
/// Each push method checks the matching cap on
/// [`Self::caps`]; once a category's `Vec` reaches the resolved cap,
/// subsequent pushes drop the item and increment the matching
/// counter on `state.truncation_counters`. The caps protect against
/// the unbounded-growth OOM observed on pathological fixtures.
#[derive(Debug, Default)]
pub struct RequestFootprintAccumulator {
    state: Mutex<AccumulatorState>,
    caps: AuditCaps,
}

impl RequestFootprintAccumulator {
    /// Construct an accumulator using the default
    /// [`AuditCaps`] (every category capped at its `DEFAULT_*`
    /// constant — 10_000 by default). One per audited request.
    pub fn new() -> Self {
        Self::with_caps(AuditCaps::default())
    }

    /// Construct an accumulator with explicit caps. Production
    /// callers pass `host.config.audit_caps.clone()`; tests pass a
    /// custom [`AuditCaps`] to exercise the cap behaviour.
    pub fn with_caps(caps: AuditCaps) -> Self {
        Self {
            state: Mutex::new(AccumulatorState::default()),
            caps,
        }
    }

    /// Borrow the caps this accumulator was constructed with.
    pub fn caps(&self) -> &AuditCaps {
        &self.caps
    }

    /// Drain the accumulator into a cloned state snapshot. Used by the
    /// footprint miner after the request completes.
    pub fn drain(&self) -> AccumulatorState {
        let mut st = self.state.lock();
        std::mem::take(&mut *st)
    }

    /// Append a fresh-`IndexedReady` build record.
    pub fn push_indexed_ready_build(&self, record: IndexedReadyBuildRecord) {
        let cap = self.caps.indexed_ready_builds();
        let mut st = self.state.lock();
        if st.indexed_ready_builds.len() >= cap {
            st.truncation_counters.indexed_ready_builds_truncated += 1;
            return;
        }
        st.indexed_ready_builds.push(record);
    }

    /// Append a VFS-read event (usually fanned out from the
    /// workspace's `VfsAuditSink`).
    pub fn push_vfs_read(&self, record: VfsReadRecord) {
        let cap = self.caps.vfs_reads();
        let mut st = self.state.lock();
        if st.vfs_reads.len() >= cap {
            st.truncation_counters.vfs_reads_truncated += 1;
            return;
        }
        st.vfs_reads.push(record);
    }

    /// Append a per-file timing ledger entry. Pushed by the session-side
    /// VFS sink alongside the `VfsReadRecord`. The `read_ns` field is
    /// `Some` only when the host's `audit_timing_capture` flag was on
    /// at event time.
    ///
    /// Bounded by the same `vfs_reads` cap as `push_vfs_read` — the
    /// two lanes are pushed in lockstep, so they must truncate
    /// together. Counter is `vfs_reads_truncated`.
    pub fn push_file_read_timing(&self, record: FileReadTiming) {
        let cap = self.caps.vfs_reads();
        let mut st = self.state.lock();
        if st.file_read_timings.len() >= cap {
            // Do not double-count: the matching `push_vfs_read` call
            // (same event) already incremented the counter. Drop
            // silently here so the truncation count stays equal to
            // the number of distinct dropped VFS events.
            return;
        }
        st.file_read_timings.push(record);
    }

    /// Append a per-file parse/lower timing ledger entry. Pushed by
    /// the executor's source stage when timing capture is on. The
    /// caller is responsible for ensuring the entry corresponds to a
    /// build this request triggered (read-once invariant).
    ///
    /// Bounded by the `indexed_ready_builds` cap — the parse-timing
    /// lane is keyed by canonical and only populated for builds the
    /// request triggered. Drop silently above the cap so the counter
    /// stays equal to distinct dropped builds.
    pub fn push_file_parse_timing(&self, record: FileParseTiming) {
        let cap = self.caps.indexed_ready_builds();
        let mut st = self.state.lock();
        if st.file_parse_timings.len() >= cap {
            return;
        }
        st.file_parse_timings.push(record);
    }

    /// Append a shared-load reuse record from the scheduler's
    /// dedup hook (`on_dedup_joiner`).
    pub fn push_shared_load_reuse(
        &self,
        canonical_id: Arc<str>,
        winner_request_id: u64,
        winner_audited: bool,
    ) {
        let cap = self.caps.shared_load_reuses();
        let mut st = self.state.lock();
        if st.shared_load_reuses.len() >= cap {
            st.truncation_counters.shared_load_reuses_truncated += 1;
            return;
        }
        st.shared_load_reuses.push(SharedLoadReuseRecord {
            canonical_id,
            winner_request_id,
            winner_audited,
        });
    }

    /// Append an instantiation step.
    pub fn push_instantiation(&self, record: InstantiationRecord) {
        let cap = self.caps.instantiations();
        let mut st = self.state.lock();
        if st.instantiations.len() >= cap {
            st.truncation_counters.instantiations_truncated += 1;
            return;
        }
        st.instantiations.push(record);
    }

    /// Append a projection step.
    pub fn push_projection(&self, record: ProjectionRecord) {
        let cap = self.caps.projections();
        let mut st = self.state.lock();
        if st.projections.len() >= cap {
            st.truncation_counters.projections_truncated += 1;
            return;
        }
        st.projections.push(record);
    }

    /// Append a conditional-branch decision.
    pub fn push_conditional(&self, record: ConditionalRecord) {
        let cap = self.caps.conditional_decisions();
        let mut st = self.state.lock();
        if st.conditional_decisions.len() >= cap {
            st.truncation_counters.conditional_decisions_truncated += 1;
            return;
        }
        st.conditional_decisions.push(record);
    }

    /// Append a type-parameter substitution step.
    pub fn push_substitution(&self, record: SubstitutionRecord) {
        let cap = self.caps.substitutions();
        let mut st = self.state.lock();
        if st.substitutions.len() >= cap {
            st.truncation_counters.substitutions_truncated += 1;
            return;
        }
        st.substitutions.push(record);
    }

    /// Append an alias-resolve step.
    pub fn push_alias_resolution(&self, record: AliasResolveRecord) {
        let cap = self.caps.alias_resolutions();
        let mut st = self.state.lock();
        if st.alias_resolutions.len() >= cap {
            st.truncation_counters.alias_resolutions_truncated += 1;
            return;
        }
        st.alias_resolutions.push(record);
    }

    /// Append a materialization envelope.
    pub fn push_materialization(&self, record: MaterializationRecord) {
        let cap = self.caps.materializations();
        let mut st = self.state.lock();
        if st.materializations.len() >= cap {
            st.truncation_counters.materializations_truncated += 1;
            return;
        }
        st.materializations.push(record);
    }

    /// Append a structured event emitted by
    /// `component_meta_trace_structured!`.
    pub fn push_structured_event(&self, event: StructuredAuditEvent) {
        let cap = self.caps.structured_events();
        let mut st = self.state.lock();
        if st.structured_events.len() >= cap {
            st.truncation_counters.structured_events_truncated += 1;
            return;
        }
        st.structured_events.push(event);
    }

    /// Append a raw derivation edge captured by the
    /// `record_origin_edge` hook.
    pub fn push_derivation_edge(
        &self,
        result: SemanticNodeId,
        kind: OriginEdgeKind,
        edge: OriginEdge,
    ) {
        let cap = self.caps.derivation_edges();
        let mut st = self.state.lock();
        if st.derivation_edges_raw.len() >= cap {
            st.truncation_counters.derivation_edges_raw_truncated += 1;
            return;
        }
        st.derivation_edges_raw
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
        acc.push_structured_event(StructuredAuditEvent::RequestStart {
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
