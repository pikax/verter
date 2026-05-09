#![deny(missing_docs)]
//! `VerterHost::audit_mcp_tool_call` — public audited entry-point for
//! MCP tool invocations.
//!
//! Wraps a single MCP tool invocation in the same audit-registration /
//! TLS-observer machinery the component-meta and compile entry-points
//! use. The caller closure performs the tool's actual work; this
//! wrapper stamps a request id, constructs a
//! [`crate::request_context::RequestContext`] keyed by
//! [`verter_audit::RequestKind::Mcp`], installs the matching
//! TLS observer, runs the closure, and finalises a
//! [`verter_audit::McpToolPayload`] through the registration.
//!
//! Downstream correlation: any audited sub-request the closure
//! initiates (`get_component_meta_with_resolution`,
//! `compile_with_audit`, `resolve_type_with_audit`, …) sniffs the
//! installed TLS slot at construction time and records the MCP
//! request's id as its `parent_request_id`. The shared scheduler-side
//! TLS mechanism (`verter_scheduler::request_context::current_request_id`)
//! is the propagation channel; this wrapper does not need to thread
//! the id explicitly.
//!
//! Returns `(T, Option<RequestAuditRecord>)`. The record is `None`
//! when the audit-config consumer filter rejects the
//! `RequestKind::Mcp` kind (`AuditRequestRegistration::Noop`); the
//! tool's work always runs.

use std::sync::Arc;

use verter_audit::{
    McpToolPayload, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::VerterHost;

/// Outcome a caller closure produces. Carries the closure's value
/// alongside two audit-payload facts the wrapper cannot infer on its
/// own: the result size in bytes (caller measures the response body
/// it intends to ship to the MCP client) and an optional error
/// message. The wrapper assembles these into the
/// [`McpToolPayload`] without inspecting the closure's value.
pub struct McpToolOutcome<T> {
    /// The value the tool produced. Returned to the caller verbatim.
    pub value: T,
    /// Approximate result size in bytes — the caller measures the
    /// response body it will ship to the MCP client.
    pub result_size_bytes: u32,
    /// Optional error message. Tools that succeed leave this `None`;
    /// tools that fail set it to a short human-readable summary.
    pub error: Option<String>,
}

impl VerterHost {
    /// Drive an MCP tool invocation under audit.
    ///
    /// Lifecycle:
    /// 1. Stamps a fresh request id from the host's monotonic counter
    ///    and bumps the per-thread audited-run counter so the harness
    ///    multi-request guard observes the call.
    /// 2. Constructs a [`RequestContext`] with
    ///    [`RequestKind::Mcp`] tagged by `tool_name`. The constructor
    ///    sniffs the scheduler-side TLS slot; tools nested under an
    ///    enclosing audited request inherit `parent_request_id`
    ///    automatically.
    /// 3. Constructs an [`AuditRequestRegistration`] BEFORE installing
    ///    the TLS guard. The `Active` arm enters the host's
    ///    active-request registry; the `Noop` arm short-circuits when
    ///    the audit-config consumer filter rejects the kind.
    /// 4. Installs [`RequestContextGuard`] (active arm) or the
    ///    no-op observer (filtered arm) so any sub-requests the
    ///    closure spawns (`get_component_meta_with_resolution`,
    ///    `compile_with_audit`, …) record the MCP request as their
    ///    parent.
    /// 5. Runs the closure. The closure returns a
    ///    [`McpToolOutcome`] carrying the value, the result size in
    ///    bytes (caller-measured), and an optional error message.
    /// 6. Assembles a [`McpToolPayload`] from the outcome plus the
    ///    caller-supplied `tool_name` / `args_size_bytes`, builds the
    ///    [`RequestAuditRecord`], and finalises through the
    ///    registration.
    /// 7. Returns the closure's value paired with the audit record
    ///    (or `None` when the consumer filter rejected the kind).
    pub fn audit_mcp_tool_call<T, F>(
        self: &Arc<Self>,
        tool_name: &str,
        canonical_id: &str,
        args_size_bytes: u32,
        f: F,
    ) -> (T, Option<RequestAuditRecord>)
    where
        F: FnOnce(&Arc<Self>) -> McpToolOutcome<T>,
    {
        // Audit-disabled fast path: drive the closure with NO
        // RequestContextGuard installed. Producer-side
        // current_observer() returns None and the instrumentation
        // short-circuits.
        if !self.config.audit_enabled {
            let outcome = f(self);
            return (outcome.value, None);
        }

        // 1. Stamp request id and bump the harness multi-request guard.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        // 2. Build the per-request context. Footprint capture is
        //    disabled — MCP tools do not collect semantic-footprint
        //    events the way component-meta does. Timing capture
        //    follows the host config.
        let footprint_capture = false;
        let timing_capture = self.config.audit_timing_capture;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::Mcp {
                tool: tool_name.to_string(),
            },
            footprint_capture,
            timing_capture,
            None,
        );

        // 3. Construct the registration BEFORE installing the TLS
        //    guard so the active-request registry slot precedes the
        //    closure body. The `Noop` arm returns when the consumer
        //    filter rejects `RequestKind::Mcp`.
        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
        let _ = ctx.install_audit_registration(Arc::clone(&registration));

        // 4. Install the matching TLS observer. The active arm uses
        //    the real RequestContextGuard so sub-requests spawned by
        //    the closure inherit `parent_request_id` from this MCP
        //    request via the scheduler-side TLS slot. The filtered
        //    arm installs a no-op observer; downstream emit sites
        //    short-circuit but child requests can still observe the
        //    parent id.
        let total_start = Instant::now();
        let outcome = match registration.as_ref() {
            AuditRequestRegistration::Active(_) => {
                let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
                f(self)
            }
            AuditRequestRegistration::Noop => {
                let _noop_guard = verter_audit::install_noop_observer();
                f(self)
            }
        };
        let total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

        // 5. Filtered kinds: skip record construction.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            return (outcome.value, None);
        }

        // 6. Assemble the payload and the envelope.
        let payload = McpToolPayload {
            tool_name: tool_name.to_string(),
            args_size_bytes,
            result_size_bytes: outcome.result_size_bytes,
            error: outcome.error,
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
            process_rss_peak_bytes: ctx
                .process_rss_peak_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            ..RequestMemoryAudit::default()
        };
        let waits = if ctx.timing_capture {
            Some(WaitAudit {
                lock_wait_ns: ctx.lock_wait_ns.load(std::sync::atomic::Ordering::Relaxed),
                queue_wait_ns: ctx.queue_wait_ns.load(std::sync::atomic::Ordering::Relaxed),
                lock_acquisitions: ctx
                    .lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
            })
        } else {
            None
        };

        let record = RequestAuditRecord {
            request_id,
            canonical_id: canonical_id.to_string(),
            kind: RequestKind::Mcp {
                tool: tool_name.to_string(),
            },
            parent_request_id: ctx.parent_request_id.map(|id| id.to_string()),
            from_cache: false,
            timings,
            memory,
            store,
            footprint: None,
            scheduler: ctx.scheduler_audit.lock().clone(),
            files: Vec::new(),
            waits,
            kind_payload: RequestKindPayload::Mcp(payload),
            trace_id: String::new(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        (outcome.value, Some(cloned))
    }
}
