#![deny(missing_docs)]
//! Session-side wiring for the component-meta audit surface.
//!
//! Audit DTOs (record envelope, timing/memory/store data, footprint
//! types, structured events, observer trait) live in
//! [`verter_audit`]. This module hosts the session-only orchestration
//! glue: the per-request [`AuditBuilder`], the
//! [`RequestPhaseAudit`] TLS stack, the structured-trace emit
//! helpers, and the bridges between session-owned domain types
//! (`semantic_query::ProjectionMode`,
//! `verter_workspace::audit_sink::VfsAuditLayer`) and the substrate's
//! audit-side mirrors.
//!
//! In-crate re-exports below preserve the historic
//! `verter_session::component_meta_audit::<Type>` import paths so
//! same-crate callers do not need to retarget every import.

use std::cell::RefCell;
use std::sync::Arc;

use crate::instant::Instant;

pub mod accumulator;
pub mod assertions;
pub mod audit_records_store;
#[cfg(test)]
pub(crate) mod expected_display_snapshots;
pub mod footprint_miner;
pub(crate) mod session_vfs_sink;
pub mod structured_event;

#[cfg(test)]
mod mod_tests;

pub use accumulator::{
    AccumulatorState, FileParseTiming, FileReadTiming, RequestFootprintAccumulator,
};
pub use assertions::{
    render_chain_text, AssertionDiff, ChainTermination, ProvenanceChain, ProvenanceStep,
    WALKER_DEPTH_CAP,
};
pub use audit_records_store::{AuditRecordsStore, AUDIT_RECORDS_STORE_CAPACITY};
pub use footprint_miner::mine_footprint;

// ----------------------------------------------------------------------------
// Re-exports of audit DTOs from the substrate. Preserves the historic
// `verter_session::component_meta_audit::<Type>` import paths for
// same-crate callers; consumers outside this crate import from
// `verter_audit::*` directly.
// ----------------------------------------------------------------------------

pub use verter_audit::footprint::{
    AliasResolveRecord, CacheOutcomeTally, ConditionalRecord, GraphCompletenessReport,
    IndexedReadyBuildRecord, InstantiationRecord, MaterializationRecord, ProjectionRecord,
    RequestFootprintAudit, SharedLoadReuseRecord, SubstitutionRecord, VfsReadRecord,
};
pub use verter_audit::memory::{current_process_rss, RequestMemoryAudit};
pub use verter_audit::observer::{
    current_observer, install_observer, AuditEvent, AuditObserver, ObserverGuard,
};
pub use verter_audit::origin_graph::{
    ConditionalBranch, DerivationEdgeRaw, DerivationEdgeRecord, DerivationSubgraph,
    DispatchKeyKind, EdgeId, MaterializationScopeAudit, MaterializationSubject,
    MaterializeSkipReason, NamedIdentity, NodeId, NodeRecord, NormalizeKind, OriginEdgeKind,
    OriginEdgeMetaDto, ProjectPathSegment, ProjectionModeAudit, SemanticNodeKind, VfsLayer,
};
pub use verter_audit::payloads::cache_outcomes::CacheOutcomeKind;
pub use verter_audit::payloads::ComponentMetaPayload;
pub use verter_audit::record::{IncidentalFields, RequestAuditRecord, RequestPhaseAudit};
pub use verter_audit::store::RequestStoreAudit;
pub use verter_audit::structured_event::StructuredAuditEvent;
pub use verter_audit::timing::RequestTimingAudit;

// In-crate alias so existing TLS-stack call sites continue to read.
pub use verter_audit::record::{Hash16 as AuditHash16, RequestKind, RequestKindPayload};

/// Convert a session-owned [`crate::semantic_query::ProjectionMode`]
/// into the audit-side mirror enum.
///
/// Replaces the `impl From<...> for ProjectionModeAudit` that used
/// to live in this module. Both source and target are now foreign
/// types from the perspective of `verter_session`, so the orphan
/// rule prevents an `impl From`. Producers call this helper
/// explicitly.
#[must_use]
pub fn projection_mode_audit_from(
    mode: crate::semantic_query::ProjectionMode,
) -> ProjectionModeAudit {
    use crate::semantic_query::ProjectionMode;
    match mode {
        ProjectionMode::Identity => ProjectionModeAudit::Identity,
        ProjectionMode::Navigate => ProjectionModeAudit::Navigate,
        ProjectionMode::Shallow => ProjectionModeAudit::Shallow,
        ProjectionMode::Expanded => ProjectionModeAudit::Expanded,
        ProjectionMode::Skeleton => ProjectionModeAudit::Skeleton,
    }
}

/// Convert a workspace-side `VfsAuditLayer` into the audit-side
/// mirror.
///
/// Replaces `impl From<verter_workspace::audit_sink::VfsAuditLayer>
/// for VfsLayer` for the same orphan-rule reason as
/// [`projection_mode_audit_from`].
#[must_use]
pub fn vfs_layer_from_workspace(layer: verter_workspace::audit_sink::VfsAuditLayer) -> VfsLayer {
    use verter_workspace::audit_sink::VfsAuditLayer as W;
    match layer {
        W::Overlay => VfsLayer::Overlay,
        W::Snapshot => VfsLayer::Snapshot,
        W::Disk => VfsLayer::Disk,
        W::DirIndexNegative => VfsLayer::DirIndexNegative,
        W::Missing => VfsLayer::Missing,
    }
}

/// Snapshot the per-request cache counters from the currently
/// installed [`crate::request_context::RequestContext`] into a
/// [`verter_audit::store::CacheLayerBreakdown`]. Used at request
/// finalisation to attribute hits/misses to THIS request only.
///
/// Returns a default (all-zero) breakdown when no context is
/// installed — the synthetic-record path (warm-cache fixture) lands
/// here and emits a zeroed breakdown, which is the correct semantic
/// for a request that did not exercise the cache layers.
#[must_use]
pub fn snapshot_cache_layers_from_tls() -> verter_audit::store::CacheLayerBreakdown {
    use verter_audit::store::{CacheLayerBreakdown, CacheLayerHitMiss};
    let Some(ctx) = crate::request_context::current_request_context() else {
        return CacheLayerBreakdown::default();
    };
    let snap = |hm: &crate::request_context::HitMiss| {
        let (hits, misses) = hm.snapshot();
        CacheLayerHitMiss { hits, misses }
    };
    CacheLayerBreakdown {
        indexed: snap(&ctx.cache_counters.indexed),
        analysis: snap(&ctx.cache_counters.analysis),
        owner_import: snap(&ctx.cache_counters.owner_import),
        route_owned_shallow: snap(&ctx.cache_counters.route_owned_shallow),
        component_meta: snap(&ctx.cache_counters.component_meta),
        route_db: snap(&ctx.cache_counters.route_db),
        ref_cycle: snap(&ctx.cache_counters.ref_cycle),
        intrinsic_registry: snap(&ctx.cache_counters.intrinsic_registry),
        semantic_graph: snap(&ctx.cache_counters.semantic_graph),
        materialize_structure: snap(&ctx.cache_counters.materialize_structure),
        materialize_memo: snap(&ctx.cache_counters.materialize_memo),
        prepared_surface: snap(&ctx.cache_counters.prepared_surface),
        prepared_member: snap(&ctx.cache_counters.prepared_member),
    }
}

// ----------------------------------------------------------------------------
// Audit builder — accumulates data during a request
// ----------------------------------------------------------------------------

/// Builder for accumulating audit data during a component-meta
/// request. Created only when `audit_enabled` is true.
pub struct AuditBuilder {
    request_id: u64,
    canonical_id: String,
    request_start: Instant,
    phase_start: Instant,
    timings: RequestTimingAudit,
    store: RequestStoreAudit,
    memory: RequestMemoryAudit,
    footprint: Option<RequestFootprintAudit>,
    files: Vec<verter_audit::files::FileAudit>,
    component_meta_payload: ComponentMetaPayload,
    /// Set by the cold-resolver path when the singleflight identified
    /// this request as a joiner (Follower) on an in-flight semantic
    /// computation. Joiner-accounting contract: a joiner records
    /// `from_cache=true` because semantically it received its result
    /// from the dedup-join, not by computing cold. Default `false`
    /// (the cold winner / pre-`mark_joined_inflight` path).
    from_cache: bool,
}

impl AuditBuilder {
    /// Construct a new builder stamped with `request_id` and the
    /// resolved `canonical_id`. Captures the current process RSS for
    /// the memory-delta baseline.
    pub fn new(request_id: u64, canonical_id: String) -> Self {
        let now = Instant::now();
        let rss = current_process_rss();
        Self {
            request_id,
            canonical_id,
            request_start: now,
            phase_start: now,
            timings: RequestTimingAudit::default(),
            store: RequestStoreAudit::default(),
            memory: RequestMemoryAudit {
                process_rss_before_bytes: rss,
                ..Default::default()
            },
            footprint: None,
            files: Vec::new(),
            component_meta_payload: ComponentMetaPayload::default(),
            from_cache: false,
        }
    }

    /// Mark the end of the current phase and start the next one.
    pub fn end_phase(&mut self, phase: AuditPhase) {
        let elapsed = self.phase_start.elapsed().as_secs_f64() * 1000.0;
        match phase {
            AuditPhase::CaptureInputs => self.timings.capture_inputs_ms = elapsed,
            AuditPhase::StoreRead => self.timings.store_read_ms = elapsed,
            AuditPhase::StoreMerge => self.timings.store_merge_ms = elapsed,
            AuditPhase::DirectImportProof => self.timings.direct_import_proof_ms = elapsed,
            AuditPhase::ImportedRootProof => self.timings.imported_root_proof_ms = elapsed,
            AuditPhase::Solver => self.timings.solver_ms = elapsed,
            AuditPhase::Materialize => self.timings.materialize_ms = elapsed,
            AuditPhase::Serialize => self.timings.serialize_ms = elapsed,
        }
        self.phase_start = Instant::now();
    }

    /// Record `steps` solver resolve-steps and bump the solve-count.
    /// Counters live on the [`ComponentMetaPayload`] (component-meta
    /// is the only request kind that runs the solver).
    pub fn record_solver_steps(&mut self, steps: u64) {
        self.component_meta_payload.total_resolve_steps += steps;
        self.component_meta_payload.solve_count += 1;
    }

    /// Replace the generic store-counter block. Component-meta-specific
    /// counters route through [`Self::record_component_meta_store`].
    pub fn record_store(&mut self, store: RequestStoreAudit) {
        self.store = store;
    }

    /// Replace the component-meta store + materialiser counter
    /// block. These fields live on [`ComponentMetaPayload`] rather
    /// than the generic [`RequestStoreAudit`] envelope because they
    /// are kind-specific (only component-meta requests run the
    /// materialiser).
    pub fn record_component_meta_store(
        &mut self,
        materialize_structure_calls: u64,
        materialize_structure_cache_hits: u64,
        node_arena_lock_acquisitions: u64,
        family_map_lock_acquisitions: u64,
        dep_signature_merges: u64,
        dep_signature_intern_hits: u64,
    ) {
        self.component_meta_payload.materialize_structure_calls = materialize_structure_calls;
        self.component_meta_payload.materialize_structure_cache_hits =
            materialize_structure_cache_hits;
        self.component_meta_payload.node_arena_lock_acquisitions = node_arena_lock_acquisitions;
        self.component_meta_payload.family_map_lock_acquisitions = family_map_lock_acquisitions;
        self.component_meta_payload.dep_signature_merges = dep_signature_merges;
        self.component_meta_payload.dep_signature_intern_hits = dep_signature_intern_hits;
    }

    /// Record host-cache + workspace memory snapshots (before/after).
    pub fn record_memory_snapshots(
        &mut self,
        host_cache_before_bytes: u64,
        host_cache_after_bytes: u64,
        workspace_before_bytes: u64,
        workspace_after_bytes: u64,
    ) {
        self.memory.host_cache_before_bytes = host_cache_before_bytes;
        self.memory.host_cache_after_bytes = host_cache_after_bytes;
        self.memory.workspace_before_bytes = workspace_before_bytes;
        self.memory.workspace_after_bytes = workspace_after_bytes;
    }

    /// Replace the timings block.
    pub fn record_timings(&mut self, timings: RequestTimingAudit) {
        self.timings = timings;
    }

    /// Replace the in-flight component-meta payload wholesale.
    ///
    /// Used at the end of cold-resolver execution by the
    /// host-manage path: the component-meta cache stores a
    /// pre-aggregated [`ComponentMetaPayload`] from the cold
    /// resolution, and the audit-emitting wrapper imports those
    /// counters into the builder before adding any post-cache work.
    pub fn record_component_meta_payload(&mut self, payload: ComponentMetaPayload) {
        self.component_meta_payload = payload;
    }

    /// Borrow the in-flight component-meta payload mutably so callers
    /// outside this module can update individual counters before
    /// finalising the record.
    pub fn component_meta_payload_mut(&mut self) -> &mut ComponentMetaPayload {
        &mut self.component_meta_payload
    }

    /// Borrow the in-flight component-meta payload immutably.
    #[must_use]
    pub fn component_meta_payload(&self) -> &ComponentMetaPayload {
        &self.component_meta_payload
    }

    /// Attach a fully-mined semantic footprint to this builder.
    pub fn record_footprint(&mut self, footprint: RequestFootprintAudit) {
        self.footprint = Some(footprint);
    }

    /// Record the per-file attribution vector built from the
    /// per-request file ledger. Called by the host before
    /// [`Self::finish`] so the read-once-aware `request_critical_path_ms`
    /// and `bytes_parsed` aggregates can derive from it.
    pub fn record_files(&mut self, files: Vec<verter_audit::files::FileAudit>) {
        self.files = files;
    }

    /// Mark this request as a joiner on an in-flight semantic
    /// computation. Joiner-accounting contract: when N concurrent
    /// requests dedup-join the same compute, the winner records
    /// `from_cache=false` + cache miss; each joiner records
    /// `from_cache=true` + cache hit. The cold path discriminates
    /// the winner from the joiners via the singleflight role. Joiners
    /// flip the speculative miss recorded by the warm-cache check
    /// (which observed the empty cache before the winner published)
    /// into a hit on the active TLS request context — the resulting
    /// per-request snapshot then attributes exactly one
    /// `component_meta` hit per joiner and exactly zero misses.
    pub fn mark_joined_inflight(&mut self) {
        use std::sync::atomic::Ordering;
        self.from_cache = true;
        if let Some(ctx) = crate::request_context::current_request_context() {
            if ctx.request_id == self.request_id {
                let layer = &ctx.cache_counters.component_meta;
                // Undo the speculative miss bumped by the warm-cache
                // check (`ComponentMetaResultDb::get` returned None
                // before the winner published). saturating_sub via
                // compare-exchange-style fetch_update keeps the
                // counter monotonic even if the speculative miss was
                // never bumped (defensive — the warm path always
                // bumps under audit_enabled, but the call is
                // idempotent under the floor).
                let prev = layer.misses.load(Ordering::Relaxed);
                if prev > 0 {
                    layer.misses.fetch_sub(1, Ordering::Relaxed);
                }
                layer.hits.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Finalize the builder into a [`RequestAuditRecord`] — captures
    /// the request-end RSS, computes the signed delta, fills the
    /// `total_ms` wall-clock, snapshots the per-request cache
    /// counters from the currently installed [`crate::request_context::RequestContext`]
    /// into [`RequestStoreAudit::cache_layers`], and snapshots the
    /// per-request peak RSS slot maintained by the host-owned
    /// sampler thread (or `0` when the sampler never ran). The
    /// request is single-threaded at finalisation, so the
    /// relaxed-ordering snapshot observes every prior bump on the
    /// same context.
    pub fn finish(mut self) -> RequestAuditRecord {
        self.timings.total_ms = self.request_start.elapsed().as_secs_f64() * 1000.0;
        self.memory.process_rss_after_bytes = current_process_rss();
        self.memory.process_rss_delta_bytes = self.memory.process_rss_after_bytes as i64
            - self.memory.process_rss_before_bytes as i64;
        self.store.cache_layers = snapshot_cache_layers_from_tls();

        // Read scheduler attribution + parent-request correlation
        // from the active request context. Both are populated by the
        // scheduler dispatch sites and the `RequestContext`
        // constructor respectively; an absent context (rare —
        // direct callers outside the audited entry-point) leaves both
        // fields `None`, matching the substrate's WASM behaviour.
        // The host-owned sampler ticks `fetch_max(current_rss)`
        // into the per-request peak slot on the matching
        // `RequestContext`. Read it back through TLS — the public
        // audited entry-point installs the context BEFORE the
        // request runs and KEEPS it installed until the record is
        // built. When no context is in scope (the synthetic test
        // fixture path), the peak stays at 0, matching the WASM /
        // flag-off contract.
        let (parent_request_id, scheduler, waits, trace_id) =
            match crate::request_context::current_request_context() {
                Some(ctx) if ctx.request_id == self.request_id => {
                    self.memory.process_rss_peak_bytes = ctx
                        .process_rss_peak_bytes
                        .load(std::sync::atomic::Ordering::Relaxed);
                    // Snapshot the per-request slot-binding-attribution
                    // counters into the component-meta payload. The
                    // host-global counters (process-wide
                    // `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS` and the
                    // `SemanticGraphStore::memo_size_in_test` delta)
                    // remain the canonical signals; the per-request
                    // mirrors here let attribution tests assert
                    // "synthesis-attributable count == 0 for this
                    // request" without false positives from peer
                    // dispatches in workspace-parallel runs.
                    self.component_meta_payload.expanded_instantiate_calls = ctx
                        .expanded_instantiate_calls
                        .load(std::sync::atomic::Ordering::Relaxed);
                    self.component_meta_payload.memo_insertions = ctx
                        .memo_insertions
                        .load(std::sync::atomic::Ordering::Relaxed);
                    self.component_meta_payload.memo_publish_suppressed = ctx
                        .memo_publish_suppressed
                        .load(std::sync::atomic::Ordering::Relaxed);
                    // `waits` is populated only when the host's
                    // `audit_timing_capture` flag is on. The flag is
                    // mirrored onto the context at construction
                    // (`RequestContext::with_kind_and_timing`); when off,
                    // the field stays `None` so the zero-cost path is
                    // preserved through serialisation.
                    let waits = if ctx.timing_capture {
                        Some(verter_audit::WaitAudit {
                            lock_wait_ns: ctx
                                .lock_wait_ns
                                .load(std::sync::atomic::Ordering::Relaxed),
                            queue_wait_ns: ctx
                                .queue_wait_ns
                                .load(std::sync::atomic::Ordering::Relaxed),
                            lock_acquisitions: ctx
                                .lock_acquisitions
                                .load(std::sync::atomic::Ordering::Relaxed),
                        })
                    } else {
                        None
                    };
                    (
                        ctx.parent_request_id.map(|id| id.to_string()),
                        ctx.scheduler_audit.lock().clone(),
                        waits,
                        ctx.trace_id.clone(),
                    )
                }
                _ => (None, None, None, String::new()),
            };

        // bytes_parsed (always-on under audit_enabled): sum of bytes_read
        // across non-NotLoaded entries.
        let bytes_parsed: u64 = self
            .files
            .iter()
            .filter(|f| !matches!(f.role, verter_audit::files::FileRole::NotLoaded))
            .map(|f| f.bytes_read)
            .sum();
        self.memory.bytes_parsed = bytes_parsed;

        // request_critical_path_ms: sum of read+parse+lower for files
        // this request triggered. Read-once-aware.
        let critical_path_ms: f64 = self
            .files
            .iter()
            .filter(|f| f.triggered_by_this_request)
            .map(|f| {
                f.read_ms.unwrap_or(0.0) + f.parse_ms.unwrap_or(0.0) + f.lower_ms.unwrap_or(0.0)
            })
            .sum();
        self.timings.request_critical_path_ms = critical_path_ms;

        RequestAuditRecord {
            request_id: self.request_id,
            canonical_id: self.canonical_id,
            kind: RequestKind::ComponentMeta,
            parent_request_id,
            from_cache: self.from_cache,
            timings: self.timings,
            memory: self.memory,
            store: self.store,
            footprint: self.footprint,
            scheduler,
            files: self.files,
            waits,
            kind_payload: RequestKindPayload::ComponentMeta(self.component_meta_payload),
            trace_id,
        }
    }
}

/// Named phases for timing capture.
#[derive(Debug, Clone, Copy)]
pub enum AuditPhase {
    /// Capture inputs (request args, config snapshot).
    CaptureInputs,
    /// Read the project type store.
    StoreRead,
    /// Merge store data with the overlay view.
    StoreMerge,
    /// Prove direct imports.
    DirectImportProof,
    /// Prove transitively-imported type roots.
    ImportedRootProof,
    /// Type solver invocation.
    Solver,
    /// Member-route / public-type materialization.
    Materialize,
    /// Serialize the final component-meta payload.
    Serialize,
}

thread_local! {
    static ACTIVE_REQUEST_AUDIT: RefCell<Vec<(u64, RequestPhaseAudit)>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII guard returned by [`begin_request_audit`]. Drops the
/// corresponding entry from the `ACTIVE_REQUEST_AUDIT` TLS stack on
/// scope exit.
pub struct RequestAuditGuard {
    request_id: u64,
}

impl RequestAuditGuard {
    /// Snapshot the phase-audit state for this request without
    /// removing it from the stack.
    pub fn snapshot(&self) -> RequestPhaseAudit {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            stack
                .borrow()
                .iter()
                .rev()
                .find(|(request_id, _)| *request_id == self.request_id)
                .map(|(_, audit)| audit.clone())
                .unwrap_or_default()
        })
    }
}

impl Drop for RequestAuditGuard {
    fn drop(&mut self) {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(position) = stack
                .iter()
                .rposition(|(request_id, _)| *request_id == self.request_id)
            {
                stack.remove(position);
            }
        });
    }
}

/// Compute the deduplicated per-file attribution vector for a finished
/// request from the drained accumulator's file ledger.
///
/// Inputs:
/// - `state` — drained accumulator state with `file_read_timings` /
///   `file_parse_timings` / `indexed_ready_builds` populated by the
///   workspace and executor instrumentation.
/// - `entry_canonical_id` — the request's primary subject. The matching
///   ledger entry is tagged [`verter_audit::files::FileRole::Entry`].
/// - `direct_import_canonicals` — set of canonical ids that are
///   first-level imports of the entry. Files reached through these
///   canonicals are tagged [`verter_audit::files::FileRole::DirectImport`];
///   anything else (non-Entry, non-IndexedReadyBuild) is tagged
///   [`verter_audit::files::FileRole::TransitiveImport`]. The set may be
///   empty when the entry's shallow surface is not yet available — in
///   that case all non-Entry, non-IndexedReadyBuild files fall back to
///   `DirectImport`, preserving the legacy attribution.
/// - `timing_capture_on` — when `true`, per-file `read_ms` / `parse_ms`
///   / `lower_ms` are populated from `Instant::now()` measurements.
///
/// Read-once invariant: a file appearing in `state.indexed_ready_builds`
/// is treated as triggered by THIS request (the build site only fires
/// on a fresh insert). Files served entirely from the existing
/// `IndexedReady` cache do NOT show up in `indexed_ready_builds` and
/// therefore receive `triggered_by_this_request = false` and all
/// `*_ms = None`.
pub fn build_file_audit_vec(
    state: &accumulator::AccumulatorState,
    entry_canonical_id: &str,
    direct_import_canonicals: &rustc_hash::FxHashSet<String>,
    timing_capture_on: bool,
) -> Vec<verter_audit::files::FileAudit> {
    use rustc_hash::{FxHashMap, FxHashSet};
    use verter_audit::files::{FileAudit, FileRole};

    let mut parse_timings: FxHashMap<String, (u64, u64)> = FxHashMap::default();
    parse_timings.reserve(state.file_parse_timings.len());
    for entry in &state.file_parse_timings {
        parse_timings.insert(
            entry.canonical_id.to_string(),
            (entry.parse_ns, entry.lower_ns),
        );
    }

    let mut triggered: FxHashSet<String> = FxHashSet::default();
    for build in &state.indexed_ready_builds {
        triggered.insert(build.canonical_id.to_string());
    }

    let mut by_id: FxHashMap<String, FileAudit> = FxHashMap::default();
    let mut order: Vec<String> = Vec::with_capacity(state.file_read_timings.len());
    for read in &state.file_read_timings {
        let key = read.canonical_id.to_string();
        if by_id.contains_key(&key) {
            continue;
        }
        order.push(key.clone());

        let layer = read.layer;
        let request_triggered = triggered.contains(&key);
        let role = if key.as_str() == entry_canonical_id {
            FileRole::Entry
        } else if request_triggered {
            FileRole::IndexedReadyBuild
        } else if direct_import_canonicals.is_empty()
            || direct_import_canonicals.contains(key.as_str())
        {
            // Empty set falls back to DirectImport (legacy
            // attribution when the entry's shallow surface is
            // not yet available). Otherwise: distinguish first-
            // level imports from deeper closure files.
            FileRole::DirectImport
        } else {
            FileRole::TransitiveImport
        };

        let read_ms = if timing_capture_on && request_triggered {
            read.read_ns.map(ns_to_ms)
        } else {
            None
        };
        let (parse_ms, lower_ms) = if timing_capture_on && request_triggered {
            match parse_timings.get(&key) {
                Some(&(p_ns, l_ns)) => (Some(ns_to_ms(p_ns)), Some(ns_to_ms(l_ns))),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        let audit = if request_triggered {
            FileAudit {
                canonical_id: key.clone(),
                role,
                layer,
                bytes_read: read.bytes_read,
                cache_hit: read.cache_hit,
                triggered_by_this_request: true,
                read_ms,
                parse_ms,
                lower_ms,
            }
        } else {
            FileAudit::cached(key.clone(), role, layer, read.bytes_read)
        };
        by_id.insert(key, audit);
    }

    for build in &state.indexed_ready_builds {
        let key = build.canonical_id.to_string();
        if by_id.contains_key(&key) {
            continue;
        }
        order.push(key.clone());
        // Files that ONLY appear in `indexed_ready_builds` (not in
        // `file_read_timings`) are by definition triggered by THIS
        // request — `IndexedReadyBuild` carries the read+parse cost.
        // The Entry id is short-circuited above so non-Entry files
        // here are exclusively `IndexedReadyBuild`.
        let role = if key.as_str() == entry_canonical_id {
            FileRole::Entry
        } else {
            FileRole::IndexedReadyBuild
        };
        let (parse_ms, lower_ms) = if timing_capture_on {
            match parse_timings.get(&key) {
                Some(&(p_ns, l_ns)) => (Some(ns_to_ms(p_ns)), Some(ns_to_ms(l_ns))),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        by_id.insert(
            key.clone(),
            FileAudit {
                canonical_id: key,
                role,
                layer: verter_audit::origin_graph::VfsLayer::Snapshot,
                bytes_read: 0,
                cache_hit: false,
                triggered_by_this_request: true,
                read_ms: None,
                parse_ms,
                lower_ms,
            },
        );
    }

    let mut out: Vec<FileAudit> = Vec::with_capacity(order.len() + 1);
    for key in order {
        if let Some(audit) = by_id.remove(&key) {
            out.push(audit);
        }
    }

    // Defensive cover: the entry canonical id MUST appear in the
    // file ledger even when no `read_file` event fired (e.g. the
    // host received the entry via `upsert` and served the request
    // entirely from the IndexedReady cache without re-reading
    // through the workspace). Insert at the head with the
    // appropriate role so consumers can always locate the entry.
    if !out.iter().any(|f| f.canonical_id == entry_canonical_id) {
        let bytes = state
            .indexed_ready_builds
            .iter()
            .find(|b| b.canonical_id.as_ref() == entry_canonical_id)
            .map(|_| 0u64)
            .unwrap_or(0);
        let entry_triggered = state
            .indexed_ready_builds
            .iter()
            .any(|b| b.canonical_id.as_ref() == entry_canonical_id);
        let (parse_ms, lower_ms) = if timing_capture_on && entry_triggered {
            match parse_timings.get(entry_canonical_id) {
                Some(&(p_ns, l_ns)) => (Some(ns_to_ms(p_ns)), Some(ns_to_ms(l_ns))),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        out.insert(
            0,
            FileAudit {
                canonical_id: entry_canonical_id.to_string(),
                role: FileRole::Entry,
                layer: verter_audit::origin_graph::VfsLayer::Snapshot,
                bytes_read: bytes,
                cache_hit: !entry_triggered,
                triggered_by_this_request: entry_triggered,
                read_ms: None,
                parse_ms,
                lower_ms,
            },
        );
    }

    out
}

fn ns_to_ms(ns: u64) -> f64 {
    (ns as f64) / 1_000_000.0
}

/// Push a new [`RequestPhaseAudit`] entry for `request_id` onto the
/// TLS stack and return the RAII guard that removes it on drop.
pub fn begin_request_audit(request_id: u64) -> RequestAuditGuard {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow_mut()
            .push((request_id, RequestPhaseAudit::default()));
    });
    RequestAuditGuard { request_id }
}

/// Accumulate `elapsed_ms` into the current request's imported-root
/// proof phase. Zero or negative values are dropped (defensive guard
/// against timer skew).
pub fn record_imported_root_proof_ms(elapsed_ms: f64) {
    if elapsed_ms <= 0.0 {
        return;
    }
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        if let Some((_, audit)) = stack.borrow_mut().last_mut() {
            audit.imported_root_proof_ms += elapsed_ms;
        }
    });
}

/// Snapshot the top-of-stack request's phase-audit without removing
/// it. Used by consumers that need a sidecar view without owning a
/// guard.
pub fn current_request_audit_snapshot() -> RequestPhaseAudit {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow()
            .last()
            .map(|(_, audit)| audit.clone())
            .unwrap_or_default()
    })
}

// ----------------------------------------------------------------------------
// Trace emission
// ----------------------------------------------------------------------------

/// Emit an audit record via the component-meta trace system and, when
/// `VERTER_COMPONENT_META_AUDIT_JSON_OUT` is set, also serialise the
/// record to the named path.
pub fn emit_audit_trace(record: &RequestAuditRecord) {
    let cm = record.component_meta_payload().cloned().unwrap_or_default();
    let detail = format!(
        "request_id={} canonical={} total_ms={:.2} solver_ms={:.2} solver_steps={} solve_count={} \
         capture_inputs_ms={:.2} store_read_ms={:.2} store_merge_ms={:.2} \
         direct_import_proof_ms={:.2} imported_root_proof_ms={:.2} \
         materialize_ms={:.2} serialize_ms={:.2} \
         rss_before={}B rss_after={}B rss_delta={}B \
         host_cache_before={}B host_cache_after={}B \
         workspace_before={}B workspace_after={}B \
         store_view_hits={} store_view_misses={} structural_merges={} \
         imported_dep_entries={} imported_dep_bytes={} prepared_type_decls={} prepared_value_decls={} \
         footprint_present={}",
        record.request_id,
        record.canonical_id,
        record.timings.total_ms,
        record.timings.solver_ms,
        cm.total_resolve_steps,
        cm.solve_count,
        record.timings.capture_inputs_ms,
        record.timings.store_read_ms,
        record.timings.store_merge_ms,
        record.timings.direct_import_proof_ms,
        record.timings.imported_root_proof_ms,
        record.timings.materialize_ms,
        record.timings.serialize_ms,
        record.memory.process_rss_before_bytes,
        record.memory.process_rss_after_bytes,
        record.memory.process_rss_delta_bytes,
        record.memory.host_cache_before_bytes,
        record.memory.host_cache_after_bytes,
        record.memory.workspace_before_bytes,
        record.memory.workspace_after_bytes,
        record.store.store_view_hits,
        record.store.store_view_misses,
        record.store.structural_merges,
        record.store.imported_dependency_entries,
        record.store.imported_dependency_bytes,
        record.store.prepared_type_decls,
        record.store.prepared_value_decls,
        record.footprint.is_some(),
    );
    eprintln!("[verter-rust-audit] {detail}");

    if let Ok(path) = std::env::var("VERTER_COMPONENT_META_AUDIT_JSON_OUT") {
        if !path.is_empty() {
            if let Ok(serialized) = serde_json::to_string_pretty(record) {
                let _ = std::fs::write(&path, serialized);
            }
        }
    }
}

/// Serialise an audit record to JSON.
pub fn emit_json(record: &RequestAuditRecord) -> String {
    serde_json::to_string(record).unwrap_or_default()
}

/// Merge the `dep_signature` entries from a `CacheRead` (or any
/// `&[(Arc<str>, DepVersion)]` slice) into the materialiser's
/// per-frame `local_fence` while recording audit counters in lock
/// step.
pub fn merge_dep_signature_into_local_fence(
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    incoming: &[(Arc<str>, crate::semantic_query::DepVersion)],
) {
    crate::host_manage::record_dep_signature_merge();
    let pre_existing_count = local_fence.len();
    for entry in incoming {
        let is_hit = local_fence
            .iter()
            .take(pre_existing_count)
            .any(|existing| Arc::ptr_eq(&existing.0, &entry.0) && existing.1 == entry.1);
        if is_hit {
            crate::host_manage::record_dep_signature_intern_hit();
        }
        local_fence.push(entry.clone());
    }
}

/// Record a fresh [`IndexedReady`](crate::project_type_store::IndexedReady)
/// insertion in the active request's accumulator. Pushes both a
/// typed [`IndexedReadyBuildRecord`] (direct lane used by the miner
/// on the happy path) and the equivalent [`StructuredAuditEvent`]
/// (fallback lane when the direct records vec is empty). No-op when
/// no request context is installed.
pub fn record_indexed_ready_built(canonical_id: Arc<str>, whole_hash: crate::types::Hash16) {
    if let Some(acc) = crate::request_context::current_accumulator() {
        acc.push_indexed_ready_build(IndexedReadyBuildRecord {
            canonical_id: Arc::clone(&canonical_id),
            whole_hash,
        });
        acc.push_structured_event(StructuredAuditEvent::IndexedReadyBuilt {
            canonical_id,
            whole_hash,
        });
    }
}

// ----------------------------------------------------------------------------
// Stable display key for SemanticNodeId
// ----------------------------------------------------------------------------

/// Produce a deterministic, human-readable key for a
/// [`SemanticNodeId`](crate::semantic_query::SemanticNodeId)
/// suitable for audit trace output and
/// `MaterializationSubject::Structure.node_key` field.
///
/// The key is deterministic under one project generation: identical
/// `(graph, id)` pairs produce identical strings. Returns
/// `<unknown:{id}>` when the id has not been interned in `graph`
/// (defensive: an audit lookup must not panic on a stale id from a
/// prior generation).
#[must_use]
pub fn audit_key_for_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    id: crate::semantic_query::SemanticNodeId,
) -> Arc<str> {
    use crate::semantic_query::{IndexKey, LiteralValue, SemanticNodeData};
    let Some(data) = graph.node_data(id) else {
        return Arc::from(format!("<unknown:{}>", id.0));
    };
    let label = match data.as_ref() {
        SemanticNodeData::Alias(inner) => format!("Alias({})", inner.0),
        SemanticNodeData::Object(_) => format!("Object#{}", id.0),
        SemanticNodeData::Union(arms) => format!("Union[{}]", arms.len()),
        SemanticNodeData::Intersection(arms) => format!("Intersection[{}]", arms.len()),
        SemanticNodeData::Primitive(p) => format!("Primitive({p:?})"),
        SemanticNodeData::Literal(LiteralValue::String(s)) => format!("Literal(\"{s}\")"),
        SemanticNodeData::Literal(other) => format!("Literal({other:?})"),
        SemanticNodeData::Opaque(_) => format!("Opaque#{}", id.0),
        SemanticNodeData::Array { element, readonly } => {
            format!("Array{{element={},readonly={}}}", element.0, readonly)
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            format!("Tuple[{},readonly={}]", elements.len(), readonly)
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => format!("TemplateLiteral[{}q,{}e]", quasis.len(), expressions.len()),
        SemanticNodeData::KeyOf { base } => format!("KeyOf({})", base.0),
        SemanticNodeData::IndexedAccess { object, index } => match index {
            IndexKey::String(s) => format!("IndexedAccess({}[\"{}\"])", object.0, s),
            IndexKey::Number(n) => format!("IndexedAccess({}[{}])", object.0, n),
            IndexKey::TypeNode(n) => format!("IndexedAccess({}[<type:{}>])", object.0, n.0),
        },
        SemanticNodeData::Mapped { source, .. } => format!("Mapped(source={})", source.0),
        SemanticNodeData::TypeOf { value_root, path } => format!(
            "TypeOf({}::{},path[{}])",
            value_root.scope.canonical_id,
            value_root.name,
            path.len()
        ),
        SemanticNodeData::TypeParam {
            decl,
            display_name,
            param_index,
            ..
        } => format!(
            "TypeParam({}::{}#{})",
            decl.canonical_id, display_name, param_index
        ),
        SemanticNodeData::Infer { name } => format!("Infer({name})"),
        SemanticNodeData::Conditional { distributive, .. } => {
            format!("Conditional(distributive={distributive})")
        }
        SemanticNodeData::VueMacroElements(_) => format!("VueMacroElements#{}", id.0),
        SemanticNodeData::Function { params, .. } => format!("Function[{}p]", params.len()),
        SemanticNodeData::DeclRef { identity } => {
            format!("DeclRef({}::{})", identity.canonical_id, identity.decl_name)
        }
        SemanticNodeData::InstantiationRef { base, args } => format!(
            "InstantiationRef({}::{}[{}])",
            base.canonical_id,
            base.decl_name,
            args.len()
        ),
    };
    Arc::from(label)
}
