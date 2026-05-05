#![deny(missing_docs)]
//! `VerterHost::resolve_type_with_audit` — public audited entry-point
//! for type-resolution requests.
//!
//! Wires the shared
//! [`crate::audited_request::AuditedRequestBuilder::run`] generic
//! harness to the host-bound
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`] so
//! consumers (component-meta, LSP hover, MCP) can drive a query
//! through `SemanticQueryApi` and receive the matching
//! [`verter_audit::RequestAuditRecord`] in the same call.
//!
//! Boundary contract:
//!
//! 1. Construct an
//!    [`crate::host_audit_runtime::AuditRequestRegistration`] with
//!    `RequestKind::TypeResolution`.
//! 2. Branch on the registration's `Active` / `Noop` arm to install
//!    the matching observer in TLS (real
//!    [`crate::request_context::RequestContextGuard`] or
//!    [`verter_audit::install_noop_observer`]).
//! 3. Run the resolver query through `ProjectSemanticDispatch::execute`.
//! 4. Snapshot the per-request type-resolution counters from the
//!    active [`crate::request_context::RequestContext`].
//! 5. Build the [`verter_audit::RequestAuditRecord`] with
//!    [`verter_audit::RequestKindPayload::TypeResolution`].
//! 6. Finalise through the registration. `Noop` registrations return
//!    `(Some(node), None)`; active registrations return
//!    `(Some(node), Some(record))`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::{
    ProjectionModeTag, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit, TypeResolutionPayload, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{ProjectionMode, SemanticNodeId, SemanticQueryApi, SemanticQueryKey};
use crate::VerterHost;

/// Outcome of a type-resolution query — the resolved
/// [`SemanticNodeId`] (or `None` if dispatch returned an error /
/// miss). The same shape any consumer would build for a single
/// `SemanticQueryApi::execute` call, lifted here for the public
/// `*_with_audit` API.
pub type TypeResolutionResult = SemanticNodeId;

impl VerterHost {
    /// Run a single semantic query through the shared
    /// [`ProjectSemanticDispatch`] and return the resolved node
    /// alongside the per-request [`RequestAuditRecord`].
    ///
    /// Mirrors
    /// [`Self::get_component_meta_with_resolution`]'s audit
    /// lifecycle: registration constructed BEFORE the TLS observer
    /// install, query executed inside the `RequestContext` window,
    /// counters snapshotted from the context, record finalised
    /// through the registration.
    ///
    /// Returns:
    /// - `(Some(node), Some(record))` when the audit-config consumer
    ///   filter accepts `RequestKind::TypeResolution` AND the
    ///   resolver produced a non-error result.
    /// - `(Some(node), None)` when the filter rejected the kind
    ///   (`AuditRequestRegistration::Noop`); the query still ran.
    /// - `(None, _)` when dispatch returned a [`QueryResult::Error`]
    ///   variant the caller cannot consume; the audit record is
    ///   still produced for active registrations so consumers can
    ///   observe the failed shape.
    #[must_use]
    pub fn resolve_type_with_audit(
        &self,
        query: SemanticQueryKey,
        canonical_hint: &str,
    ) -> (Option<TypeResolutionResult>, Option<RequestAuditRecord>) {
        // Stamp a fresh request id and bookkeeping for the harness'
        // multi-request guard. Mirrors the component-meta entry-point.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // Determine the projection mode from the query so the audit
        // payload reports the caller's intent (Identity for queries
        // that do not carry a mode field — Conditional, KeyOf, …).
        let query_mode = query_projection_mode(&query);

        // Construct a per-request context. Footprint accumulator is
        // disabled for type-resolution requests — they do not collect
        // semantic-footprint events the way component-meta does.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_hint),
            RequestKind::TypeResolution,
            footprint_capture,
            timing_capture,
            None,
        );

        // BEFORE installing the TLS guard: construct the registration.
        // The `Noop` arm short-circuits when the consumer filter
        // rejects `TypeResolution`; the `Active` arm captures a slot
        // in the host's active-request registry.
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        debug_assert!(
            ctx.audit_registration.get().is_none(),
            "freshly-constructed RequestContext must have no audit_registration",
        );
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        // Install the matching TLS observer at the audit boundary.
        // Active registrations install the real
        // `RequestContextGuard`; Noop installs
        // `verter_audit::NoOpObserver` so emit sites still see
        // `Some(observer)` without paying downstream cost.
        let request_start = std::time::Instant::now();
        let result = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                let dispatch = ProjectSemanticDispatch::new(self);
                dispatch.execute(query)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                let dispatch = ProjectSemanticDispatch::new(self);
                dispatch.execute(query)
            }
        };
        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;

        // Decode the dispatch result into the consumer-visible shape.
        let resolved = match &result {
            crate::semantic_query::QueryResult::Value(node) => Some(*node),
            crate::semantic_query::QueryResult::Recursive(node) => Some(*node),
            crate::semantic_query::QueryResult::Error(_) => None,
        };

        // Filtered kinds: skip record construction entirely.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            return (resolved, None);
        }

        // Build the audit record. Only the `Active` arm reaches here.
        // Snapshot per-request counters from the active context AFTER
        // dispatch completes (the context is still alive — the guard
        // dropped above only un-installed it from TLS, not the
        // owning Arc on this stack frame).
        let payload = TypeResolutionPayload {
            query_mode: ProjectionModeTag::from(query_mode),
            hops: u32::try_from(ctx.type_resolution_hops.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            navigations: u32::try_from(ctx.type_resolution_navigations.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            expansions: u32::try_from(ctx.type_resolution_expansions.load(Ordering::Relaxed))
                .unwrap_or(u32::MAX),
            conditional_decisions: u32::try_from(
                ctx.type_resolution_conditional_decisions
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            ref_root_cycle_hits: u32::try_from(
                ctx.type_resolution_ref_root_cycle_hits
                    .load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            projection_ops_executed: u32::try_from(
                ctx.type_resolution_projection_ops.load(Ordering::Relaxed),
            )
            .unwrap_or(u32::MAX),
            depth_high_water: ctx.type_resolution_depth_high_water.load(Ordering::Relaxed),
            recursion_limit_reached: ctx
                .type_resolution_recursion_limit_reached
                .load(Ordering::Relaxed),
        };

        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };
        let store = RequestStoreAudit {
            cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
            ..RequestStoreAudit::default()
        };
        let memory = RequestMemoryAudit {
            process_rss_peak_bytes: ctx.process_rss_peak_bytes.load(Ordering::Relaxed),
            ..RequestMemoryAudit::default()
        };

        let waits = if ctx.timing_capture {
            Some(WaitAudit {
                lock_wait_ns: ctx.lock_wait_ns.load(Ordering::Relaxed),
                queue_wait_ns: ctx.queue_wait_ns.load(Ordering::Relaxed),
                lock_acquisitions: ctx.lock_acquisitions.load(Ordering::Relaxed),
            })
        } else {
            None
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_hint.to_string(),
            kind: RequestKind::TypeResolution,
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::TypeResolution(payload),
        };

        let cloned = record.clone();
        registration.finalize(record);
        (resolved, Some(cloned))
    }
}

/// Project the caller-visible projection mode from a
/// [`SemanticQueryKey`]. Variants without a `mode` field map to
/// [`ProjectionMode::Identity`] — they do not consume a projection
/// budget.
fn query_projection_mode(key: &SemanticQueryKey) -> ProjectionMode {
    match key {
        SemanticQueryKey::ProjectPath { mode, .. }
        | SemanticQueryKey::ProjectMember { mode, .. }
        | SemanticQueryKey::IndexedAccess { mode, .. }
        | SemanticQueryKey::ResolveMacroPayload { mode, .. } => *mode,
        SemanticQueryKey::Instantiate { body_mode, .. } => *body_mode,
        SemanticQueryKey::ResolveDecl(_)
        | SemanticQueryKey::KeyOf { .. }
        | SemanticQueryKey::MappedType { .. }
        | SemanticQueryKey::Conditional { .. }
        | SemanticQueryKey::TypeOf { .. }
        | SemanticQueryKey::NormalizeUnion { .. }
        | SemanticQueryKey::NormalizeIntersection { .. }
        | SemanticQueryKey::ResolvedNamedType { .. }
        | SemanticQueryKey::Relate { .. } => ProjectionMode::Identity,
    }
}
