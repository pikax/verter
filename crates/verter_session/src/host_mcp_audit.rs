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
//! Returns an [`verter_audit::AuditedResult<T, E>`] carrier pairing the
//! closure's outcome (`Ok(value)` / `Err(error)`) with the audit
//! record. The carrier's `audit` field is mandatory: the full-capture
//! path returns an [`verter_audit::AuditCaptureState::ActiveStored`]
//! record, while the filtered / disabled paths return the cheap
//! default-filled record marked
//! [`verter_audit::AuditCaptureState::FilteredNoop`] /
//! [`verter_audit::AuditCaptureState::AuditDisabled`].

use std::fmt::Debug;
use std::sync::Arc;

use verter_audit::{
    AuditedResult, McpToolPayload, RequestAuditRecord, RequestKind, RequestKindPayload,
    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, WaitAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::instant::Instant;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::VerterHost;

/// Success payload a caller closure produces. Carries the tool's value
/// alongside the one audit-payload fact the wrapper cannot infer on
/// its own: the result size in bytes (the caller measures the response
/// body it intends to ship to the MCP client).
///
/// The error half of the outcome rides the closure's `Err(E)` arm —
/// the wrapper folds it into [`McpToolPayload::error`] via its `Debug`
/// rendering and routes it to the carrier's `Err`. There is no nested
/// `Result` inside the success arm.
pub struct McpToolSuccess<T> {
    /// The value the tool produced. Returned to the caller verbatim.
    pub value: T,
    /// Approximate result size in bytes — the caller measures the
    /// response body it will ship to the MCP client.
    pub result_size_bytes: u32,
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
    /// 5. Runs the closure. The closure returns
    ///    `Result<McpToolSuccess<T>, E>`: the success arm carries the
    ///    tool value plus its caller-measured result size; the error
    ///    arm carries the typed error `E` (folded into the payload's
    ///    error string via `Debug`).
    /// 6. Assembles a [`McpToolPayload`] from the outcome plus the
    ///    caller-supplied `tool_name` / `args_size_bytes`, builds the
    ///    [`RequestAuditRecord`], and finalises through the
    ///    registration.
    /// 7. Returns an [`crate::AuditedResult`] carrier pairing the
    ///    outcome (`Ok(value)` / `Err(error)`) with the audit record.
    ///    The carrier's `audit` field is always populated — the
    ///    filtered / disabled paths carry the cheap default-filled
    ///    record marked
    ///    [`verter_audit::AuditCaptureState::FilteredNoop`] /
    ///    [`verter_audit::AuditCaptureState::AuditDisabled`].
    pub fn audit_mcp_tool_call<T, E, F>(
        self: &Arc<Self>,
        tool_name: &str,
        canonical_id: &str,
        args_size_bytes: u32,
        f: F,
    ) -> AuditedResult<T, E>
    where
        E: Debug,
        F: FnOnce(&Arc<Self>) -> Result<McpToolSuccess<T>, E>,
    {
        // Audit-disabled fast path: drive the closure with NO
        // RequestContextGuard installed. Producer-side
        // current_observer() returns None and the instrumentation
        // short-circuits. The carrier still carries a cheap
        // default-filled record marked `AuditDisabled`.
        if !self.config.audit_enabled {
            let request_id = self.next_request_id();
            let outcome = f(self);
            let parent_request_id =
                verter_scheduler::request_context::current_request_id().map(|id| id.to_string());
            let record = noop_mcp_record(
                request_id,
                canonical_id,
                parent_request_id,
                tool_name,
                args_size_bytes,
                verter_audit::AuditCaptureState::AuditDisabled,
            );
            return audited_mcp_outcome(outcome, record);
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

        // 5. Filtered kinds: return the cheap default-filled record.
        //    The tool's work still ran; no payload is collected.
        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
            let record = noop_mcp_record(
                request_id,
                canonical_id,
                ctx.parent_request_id.map(|id| id.to_string()),
                tool_name,
                args_size_bytes,
                verter_audit::AuditCaptureState::FilteredNoop,
            );
            return audited_mcp_outcome(outcome, record);
        }

        // 6. Assemble the payload and the envelope. The success arm
        //    contributes `result_size_bytes`; the error arm contributes
        //    the payload error string via `Debug`.
        let (result_size_bytes, error) = match &outcome {
            Ok(success) => (success.result_size_bytes, None),
            Err(err) => (0, Some(format!("{err:?}"))),
        };
        let payload = McpToolPayload {
            tool_name: tool_name.to_string(),
            args_size_bytes,
            result_size_bytes,
            error,
        };

        let timings = RequestTimingAudit {
            total_ms,
            ..RequestTimingAudit::default()
        };
        let store = RequestStoreAudit {
            cache_layers: crate::component_meta_audit::snapshot_cache_layers_from_tls(),
            bypass_diagnostics: crate::component_meta_audit::snapshot_bypass_diagnostics_from_tls(),
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
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        };

        let cloned = record.clone();
        registration.finalize(record);
        audited_mcp_outcome(outcome, cloned)
    }
}

/// Package an MCP tool outcome and its audit record into the
/// [`AuditedResult`] carrier. The success arm's
/// [`McpToolSuccess::value`] becomes the carrier value; the size fact
/// has already been folded into the record.
fn audited_mcp_outcome<T, E>(
    outcome: Result<McpToolSuccess<T>, E>,
    audit: RequestAuditRecord,
) -> AuditedResult<T, E> {
    match outcome {
        Ok(success) => AuditedResult::ok(success.value, audit),
        Err(error) => AuditedResult::err(error, audit),
    }
}

/// Build the cheap default-filled [`RequestAuditRecord`] returned on
/// the filtered / disabled MCP path. No per-request counters are
/// collected — the payload carries only the tool identity and args
/// size, and `capture_state` records why the full path was skipped.
fn noop_mcp_record(
    request_id: u64,
    canonical_id: &str,
    parent_request_id: Option<String>,
    tool_name: &str,
    args_size_bytes: u32,
    capture_state: verter_audit::AuditCaptureState,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::Mcp {
            tool: tool_name.to_string(),
        },
        parent_request_id,
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::Mcp(McpToolPayload {
            tool_name: tool_name.to_string(),
            args_size_bytes,
            result_size_bytes: 0,
            error: None,
        }),
        capture_state,
        trace_id: String::new(),
    }
}
