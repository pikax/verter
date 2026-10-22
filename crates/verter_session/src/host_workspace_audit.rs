#![deny(missing_docs)]
//! Production-path entry point for [`verter_audit::WorkspaceOp`]
//! emission.
//!
//! `VerterHost::audit_workspace_op` constructs an
//! [`crate::host_audit_runtime::AuditRequestRegistration`] for the
//! request, drives the workspace's [`verter_workspace::WorkspaceAccess::audit_op`]
//! producer, and finalises the resulting record through the
//! registration. The `Active` arm enters the host's active-request
//! registry; the `Noop` arm is returned when the configured
//! consumer filter rejects the
//! [`verter_audit::RequestKind::Workspace`] kind.
//!
//! The trait method itself does not enter the active-request
//! registry — that lifecycle is the registration's job. This wrapper
//! is the integration point that both lifecycles meet.

use std::sync::Arc;

use verter_audit::{RequestAuditRecord, RequestKind, WorkspaceOp};

use crate::VerterHost;
#[cfg(feature = "semantic-observe")]
use verter_type_engine::request_context::RequestContextGuard;

impl VerterHost {
    /// Drive a workspace [`WorkspaceOp`] under audit.
    ///
    /// The wrapper:
    /// 1. Stamps a fresh request id from the host's monotonic
    ///    counter.
    /// 2. Constructs a [`RequestContext`] keyed by
    ///    [`RequestKind::Workspace`].
    /// 3. Constructs an
    ///    [`crate::host_audit_runtime::AuditRequestRegistration`]
    ///    BEFORE installing the TLS guard, so the active-request
    ///    registry sees the slot for the duration of the
    ///    workspace traversal.
    /// 4. Installs the request-context guard so
    ///    [`verter_execution::request_context::current_request_id`]
    ///    returns the fresh id while
    ///    [`WorkspaceAccess::audit_op`] runs.
    /// 5. Calls `workspace.audit_op(op)` to walk live workspace
    ///    state and produce a [`RequestAuditRecord`] populated
    ///    with the op's `WorkspacePayload`.
    /// 6. Stamps the freshly-allocated request id onto the record
    ///    (the trait method default reads
    ///    `current_request_id()`; this wrapper guarantees the id
    ///    matches the one the registration is keyed by) and
    ///    finalises the registration with the record.
    /// 7. Returns the record. On the `Noop` arm the registration
    ///    short-circuits without inserting into the records store,
    ///    but the produced record is still returned to the
    ///    caller — `Noop` only suppresses the records-store
    ///    side effect, not the producer's output.
    #[must_use]
    pub fn audit_workspace_op(&self, op: WorkspaceOp) -> RequestAuditRecord {
        let request_id = self.next_request_id();
        let canonical_id_str: String = match &op {
            WorkspaceOp::AuditResolve { from, .. } => from.clone(),
            WorkspaceOp::DepGraphTraverse { root } => root.clone(),
            WorkspaceOp::ResolverWalk { .. } => String::new(),
        };

        let ctx = crate::capture::root_entry_context(
            self,
            request_id,
            Arc::<str>::from(canonical_id_str.as_str()),
            || RequestKind::Workspace { op: op.clone() },
            false,
            false,
            None,
            verter_execution::cancellation::CancellationToken::new(),
        );

        // Construct the registration BEFORE the TLS guard so the
        // active-request registry slot precedes the workspace
        // traversal. The `Active` arm enters the registry; the
        // `Noop` arm is returned when the consumer filter rejects
        // the kind.
        #[cfg(feature = "semantic-observe")]
        {
            let registration = Arc::new(crate::host_audit_runtime::AuditRequestRegistration::new(
                self,
                Arc::clone(&ctx),
            ));

            let _ctx_guard = RequestContextGuard::install(ctx);

            // Drive the workspace producer; the trait method reads
            // `current_request_id()` through the guard above so the
            // produced record's `request_id` already matches the
            // registration's. We re-stamp it defensively so the
            // contract holds regardless of how the trait method's
            // default body evolves.
            let mut record = self.workspace().audit_op(op);
            record.request_id = request_id;

            registration.finalize(record.clone());
            record
        }
        // Capture is compiled out: the workspace traversal is itself the
        // observation, so nothing runs and the record reports
        // unavailable capture.
        #[cfg(not(feature = "semantic-observe"))]
        {
            drop(ctx);
            unavailable_workspace_record(request_id, canonical_id_str, op)
        }
    }
}

/// The record `audit_workspace_op` returns from a build without
/// `semantic-observe`: the request identity and operation only, marked
/// [`verter_audit::AuditCaptureState::Unavailable`].
#[cfg(not(feature = "semantic-observe"))]
fn unavailable_workspace_record(
    request_id: u64,
    canonical_id: String,
    op: WorkspaceOp,
) -> RequestAuditRecord {
    let target_identity = if canonical_id.is_empty() {
        verter_audit::RequestTargetIdentity::NotApplicable
    } else {
        verter_audit::RequestTargetIdentity::registered(canonical_id.clone())
    };
    RequestAuditRecord {
        request_id,
        canonical_id,
        target_identity: Some(target_identity),
        kind: RequestKind::Workspace { op },
        parent_request_id: None,
        from_cache: false,
        timings: verter_audit::RequestTimingAudit::default(),
        memory: verter_audit::RequestMemoryAudit::default(),
        store: verter_audit::RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: verter_audit::RequestKindPayload::None,
        capture_state: crate::capture::uncaptured_state(false),
        trace_id: String::new(),
    }
}
